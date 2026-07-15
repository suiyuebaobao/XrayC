//! Worker 组合出口探测任务维护。
//! 本文件从 maintenance 拆出 queued 探测任务超时和排队逻辑。
//! 原 SQL、事务锁和状态推进语义保持不变。
//! run_worker_maintenance 仍负责统一调用这些方法。
//! 多 Worker 并发仍依赖 PostgreSQL advisory lock。
//! 探测状态变化仍会标记相关中转节点 dirty。
//! 用户出口重算集合仍由调用方在事务后处理。
//! 新增探测策略字段时应同步这里的 SQL 绑定。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 本头部满足前十行中文注释约束。

use super::dirty::*;
use super::existence::*;
use super::probes::*;
use super::rows::*;
use crate::*;
use std::collections::HashSet;
use uuid::Uuid;

impl PgStore {
    pub(crate) async fn timeout_expired_access_exit_probe_tasks_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(u64, HashSet<Uuid>), DbError> {
        let policy = self.access_probe_policy().await?;
        let locked = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
            .bind(SCHEDULED_EXIT_PROBE_QUEUE_LOCK_ID)
            .fetch_one(&mut **tx)
            .await?;
        if !locked {
            return Ok((0, HashSet::new()));
        }

        let rows = sqlx::query_as::<_, TimedOutProbeTaskRow>(
            r#"
            WITH expired AS (
                SELECT p.id
                FROM access_exit_probes p
                WHERE p.status = 'queued'
                  AND p.task_lease_expires_at IS NOT NULL
                  AND p.task_lease_expires_at <= now()
                  AND p.task_delivery_count >= $1
                  AND NOT EXISTS (
                      SELECT 1
                      FROM access_exit_probes r
                      WHERE r.access_node_id = p.access_node_id
                        AND r.exit_endpoint_id = p.exit_endpoint_id
                        AND r.status <> 'queued'
                        AND r.probed_at >= p.probed_at
                  )
                ORDER BY p.task_lease_expires_at ASC, p.probed_at ASC, p.id ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            ),
            updated AS (
                UPDATE access_exit_probes p
                SET status = 'timeout',
                    latency_ms = NULL,
                    error_summary = 'scheduled probe task timed out after repeated delivery',
                    probed_at = now(),
                    task_lease_expires_at = NULL
                FROM expired
                WHERE p.id = expired.id
                RETURNING p.access_node_id, p.exit_endpoint_id, p.probed_at
            )
            SELECT access_node_id, exit_endpoint_id, probed_at
            FROM updated
            ORDER BY probed_at ASC, exit_endpoint_id ASC
            "#,
        )
        .bind(policy.max_probe_task_delivery_count)
        .bind(policy.probe_queue_batch_size)
        .fetch_all(&mut **tx)
        .await?;

        let mut users_to_resync = HashSet::new();
        for row in &rows {
            let error_summary =
                "scheduled probe task timed out after repeated delivery".to_string();
            let transition = apply_exit_probe_state_in_tx(
                tx,
                &policy,
                ProbeStateReport {
                    access_node_id: row.access_node_id,
                    exit_endpoint_id: row.exit_endpoint_id,
                    outcome: ProbeOutcome::Offline,
                    probe_status: "timeout",
                    latency_ms: None,
                    error_summary: error_summary.clone(),
                    probed_at: row.probed_at,
                },
            )
            .await?;
            sync_exit_probe_summary_in_tx(
                tx,
                row.exit_endpoint_id,
                ProbeOutcome::Offline,
                "timeout",
                error_summary,
                row.probed_at,
                transition.as_ref(),
            )
            .await?;
            if let Some(transition) = transition {
                if transition.changed {
                    mark_access_node_dirty_in_tx(
                        tx,
                        row.access_node_id,
                        "worker_exit_probe_task_timeout",
                    )
                    .await?;
                }
                if transition.effective_status == "offline" {
                    let affected_users = users_assigned_to_exit_endpoint_in_tx(
                        tx,
                        row.access_node_id,
                        row.exit_endpoint_id,
                    )
                    .await?;
                    if !affected_users.is_empty() && !transition.changed {
                        mark_access_node_dirty_in_tx(
                            tx,
                            row.access_node_id,
                            "worker_exit_probe_timeout_resync",
                        )
                        .await?;
                    }
                    users_to_resync.extend(affected_users);
                }
            }
        }

        Ok((rows.len() as u64, users_to_resync))
    }

    pub(crate) async fn queue_due_access_exit_probe_tasks_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<u64, DbError> {
        let policy = self.access_probe_policy().await?;
        // 多 Worker 部署时只允许一个维护事务排队，避免同一出口组合重复生成 queued 任务。
        let locked = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
            .bind(SCHEDULED_EXIT_PROBE_QUEUE_LOCK_ID)
            .fetch_one(&mut **tx)
            .await?;
        if !locked {
            return Ok(0);
        }
        let result = sqlx::query(
            r#"
            WITH candidate_pairs AS (
                SELECT DISTINCT l.access_node_id, m.exit_endpoint_id
                FROM access_lines l
                JOIN access_nodes n ON n.id = l.access_node_id
                JOIN exit_pools p ON p.id = l.exit_pool_id
                JOIN exit_pool_members m ON m.exit_pool_id = p.id
                JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
                JOIN exit_resources r ON r.id = e.exit_resource_id
                WHERE l.enabled = TRUE
                  AND p.enabled = TRUE
                  AND e.enabled = TRUE
                  AND r.enabled = TRUE
                  AND n.agent_token_hash <> ''
                UNION
                SELECT DISTINCT r.access_node_id, e.id AS exit_endpoint_id
                FROM exit_endpoints e
                JOIN exit_resources r ON r.id = e.exit_resource_id
                JOIN access_nodes n ON n.id = r.access_node_id
                WHERE r.ownership = 'self_hosted'
                  AND r.access_node_id IS NOT NULL
                  AND e.enabled = TRUE
                  AND r.enabled = TRUE
                  AND n.agent_token_hash <> ''
            ),
            pending_tasks AS (
                SELECT p.access_node_id, p.exit_endpoint_id, count(*)::BIGINT AS pending_count
                FROM access_exit_probes p
                WHERE p.status = 'queued'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM access_exit_probes r
                      WHERE r.access_node_id = p.access_node_id
                        AND r.exit_endpoint_id = p.exit_endpoint_id
                        AND r.status <> 'queued'
                        AND r.probed_at >= p.probed_at
                  )
                GROUP BY p.access_node_id, p.exit_endpoint_id
            ),
            pending_by_node AS (
                SELECT access_node_id, count(*)::BIGINT AS pending_count
                FROM pending_tasks
                GROUP BY access_node_id
            ),
            latest_results AS (
                SELECT DISTINCT ON (access_node_id, exit_endpoint_id)
                       access_node_id, exit_endpoint_id, probed_at
                FROM access_exit_probes
                WHERE status <> 'queued'
                ORDER BY access_node_id, exit_endpoint_id, probed_at DESC
            ),
            due_pairs AS (
                SELECT c.access_node_id,
                       c.exit_endpoint_id,
                       COALESCE(n.pending_count, 0) AS node_pending_count,
                       row_number() OVER (
                           PARTITION BY c.access_node_id
                           ORDER BY COALESCE(l.probed_at, 'epoch'::timestamptz), c.exit_endpoint_id
                       ) AS node_rank
                FROM candidate_pairs c
                LEFT JOIN pending_tasks t
                  ON t.access_node_id = c.access_node_id
                 AND t.exit_endpoint_id = c.exit_endpoint_id
                LEFT JOIN pending_by_node n ON n.access_node_id = c.access_node_id
                LEFT JOIN latest_results l
                  ON l.access_node_id = c.access_node_id
                 AND l.exit_endpoint_id = c.exit_endpoint_id
                WHERE t.exit_endpoint_id IS NULL
                  AND (
                      l.probed_at IS NULL
                      OR l.probed_at <= now() - ($1::BIGINT * interval '1 second')
                  )
                  AND COALESCE(n.pending_count, 0) < $3
            ),
            limited_pairs AS (
                SELECT access_node_id, exit_endpoint_id
                FROM due_pairs
                WHERE node_rank <= GREATEST($3 - node_pending_count, 0)
                ORDER BY node_pending_count ASC, node_rank ASC, exit_endpoint_id
                LIMIT $2
            )
            INSERT INTO access_exit_probes (
                access_node_id, exit_endpoint_id, status,
                latency_ms, error_summary, probed_at
            )
            SELECT access_node_id, exit_endpoint_id, 'queued', NULL,
                   'scheduled probe', now()
            FROM limited_pairs
            "#,
        )
        .bind(policy.exit_probe_interval_seconds)
        .bind(policy.probe_queue_batch_size)
        .bind(policy.max_pending_probe_tasks)
        .execute(&mut **tx)
        .await?;
        Ok(result.rows_affected())
    }
}
