//! Agent 指标、会话和探测上报。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

type ProbeItem<'a> = (Uuid, DateTime<Utc>, &'a Value);

fn latest_probe_items<'a>(
    items: Vec<&'a Value>,
    id_field: &str,
) -> Result<Vec<ProbeItem<'a>>, DbError> {
    let mut latest = HashMap::<Uuid, (DateTime<Utc>, &'a Value)>::new();
    for item in items {
        let item_id = payload_uuid(item, id_field)?;
        let item_time = payload_time(item, &["probed_at_unix"], &["probed_at"]);
        let should_replace = latest
            .get(&item_id)
            .is_none_or(|(current_time, _)| item_time >= *current_time);
        if should_replace {
            latest.insert(item_id, (item_time, item));
        }
    }
    Ok(latest
        .into_iter()
        .map(|(item_id, (item_time, item))| (item_id, item_time, item))
        .collect())
}

impl PgStore {
    pub async fn record_agent_metrics(
        &self,
        access_node_id: Uuid,
        payload: &Value,
    ) -> Result<usize, DbError> {
        let samples = payload_items_with_root_field(
            payload,
            &["metrics", "samples", "snapshots", "lines"],
            "access_line_id",
        );
        if samples.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;
        let mut written = 0_usize;
        for sample in samples {
            let access_line_id = payload_uuid(sample, "access_line_id")?;
            let line_context =
                line_runtime_context_in_tx(&mut tx, access_node_id, access_line_id).await?;
            sqlx::query(
                r#"
                INSERT INTO access_line_metric_snapshots (
                    access_node_id, exit_pool_id, access_line_id,
                    online_users, active_connections, unique_client_ips,
                    uplink_rate_bps, downlink_rate_bps, collected_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(line_context.access_node_id)
            .bind(line_context.exit_pool_id)
            .bind(access_line_id)
            .bind(payload_i32(sample, "online_users"))
            .bind(payload_i32_any(
                sample,
                &[
                    "active_connections",
                    "active_connection_count",
                    "connection_count",
                ],
            ))
            .bind(payload_i32_any(
                sample,
                &[
                    "unique_client_ips",
                    "unique_client_ip_count",
                    "distinct_client_ip_count",
                    "client_ip_count",
                ],
            ))
            .bind(to_i64(payload_u64(sample, "uplink_rate_bps")))
            .bind(to_i64(payload_u64(sample, "downlink_rate_bps")))
            .bind(payload_time(
                sample,
                &["collected_at_unix", "captured_at_unix"],
                &["collected_at", "captured_at"],
            ))
            .execute(&mut *tx)
            .await?;
            written += 1;
        }
        tx.commit().await?;
        Ok(written)
    }

    pub async fn record_agent_probes(
        &self,
        access_node_id: Uuid,
        payload: &Value,
    ) -> Result<Value, DbError> {
        let probe_policy = self.access_probe_policy().await?;
        let line_probes = latest_probe_items(
            payload_items_with_root_field(
                payload,
                &["line_probes", "access_line_probes", "lines"],
                "access_line_id",
            ),
            "access_line_id",
        )?;
        let exit_probes = latest_probe_items(
            payload_items_with_root_field(
                payload,
                &["exit_probes", "access_exit_probes", "exits"],
                "exit_endpoint_id",
            ),
            "exit_endpoint_id",
        )?;
        let mut tx = self.pool.begin().await?;
        let mut line_count = 0_usize;
        for (access_line_id, probed_at, probe) in line_probes {
            ensure_line_belongs_to_node_in_tx(&mut tx, access_node_id, access_line_id).await?;
            let probe_status = payload_text(probe, &["status"], "unknown", 64);
            let latency_ms = payload_optional_i32(probe, "latency_ms");
            let error_summary = payload_text(probe, &["error_summary", "error"], "", 512);
            let inserted = sqlx::query(
                r#"
                INSERT INTO access_line_probes (
                    access_line_id, status, latency_ms, error_summary, probed_at
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(access_line_id)
            .bind(&probe_status)
            .bind(latency_ms)
            .bind(&error_summary)
            .bind(probed_at)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            line_count += inserted as usize;
        }

        let mut exit_count = 0_usize;
        let mut users_to_resync = HashSet::new();
        for (exit_endpoint_id, payload_probed_at, probe) in exit_probes {
            ensure_exit_endpoint_exists_in_tx(&mut tx, exit_endpoint_id).await?;
            let probe_status = payload_text(probe, &["status"], "unknown", 64);
            let latency_ms = payload_optional_i32(probe, "latency_ms");
            let error_summary = payload_text(probe, &["error_summary", "error"], "", 512);
            let probed_at = payload_probed_at;
            let inserted = sqlx::query(
                r#"
                INSERT INTO access_exit_probes (
                    access_node_id, exit_endpoint_id, status,
                    latency_ms, error_summary, probed_at
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(access_node_id)
            .bind(exit_endpoint_id)
            .bind(&probe_status)
            .bind(latency_ms)
            .bind(&error_summary)
            .bind(probed_at)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted == 0 {
                continue;
            }
            let transition = if let Some(outcome) = probe_status_to_outcome(&probe_status) {
                apply_exit_probe_state_in_tx(
                    &mut tx,
                    &probe_policy,
                    ProbeStateReport {
                        access_node_id,
                        exit_endpoint_id,
                        outcome,
                        probe_status: &probe_status,
                        latency_ms,
                        error_summary: error_summary.clone(),
                        probed_at,
                    },
                )
                .await?
            } else {
                None
            };
            if let Some(outcome) = probe_status_to_outcome(&probe_status) {
                sync_exit_probe_summary_in_tx(
                    &mut tx,
                    exit_endpoint_id,
                    outcome,
                    &probe_status,
                    error_summary,
                    probed_at,
                    transition.as_ref(),
                )
                .await?;
            }
            if let Some(transition) = transition {
                let affected_pool_ids = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT DISTINCT m.exit_pool_id
                    FROM exit_pool_members m
                    JOIN access_lines l ON l.exit_pool_id = m.exit_pool_id
                    WHERE m.exit_endpoint_id = $1
                      AND l.access_node_id = $2
                    ORDER BY m.exit_pool_id
                    "#,
                )
                .bind(exit_endpoint_id)
                .bind(access_node_id)
                .fetch_all(&mut *tx)
                .await?;

                // 探测状态按 access_node -> exit_endpoint 组合生效，不能把某个
                // 中转节点的抖动写成全局出口池状态，否则共享出口会误伤其他节点。
                if transition.changed {
                    mark_access_node_dirty_in_tx(
                        &mut tx,
                        access_node_id,
                        "agent_exit_probe_state_changed",
                    )
                    .await?;
                }

                if transition.effective_status == "offline" {
                    // 当前用户出口映射不能继续停留在当前中转节点已判定 offline 的端点上；
                    // 提交探测事务后会逐个同步这些用户的出口分配。
                    for exit_pool_id in affected_pool_ids {
                        let user_ids = sqlx::query_scalar::<_, Uuid>(
                            r#"
                            SELECT DISTINCT uea.user_id
                            FROM user_exit_assignments uea
                            JOIN access_lines l ON l.id = uea.access_line_id
                            WHERE uea.exit_pool_id = $1
                              AND uea.exit_endpoint_id = $2
                              AND l.access_node_id = $3
                            ORDER BY uea.user_id
                            "#,
                        )
                        .bind(exit_pool_id)
                        .bind(exit_endpoint_id)
                        .bind(access_node_id)
                        .fetch_all(&mut *tx)
                        .await?;
                        users_to_resync.extend(user_ids);
                    }
                }
            }
            exit_count += 1;
        }
        tx.commit().await?;

        for user_id in users_to_resync {
            self.sync_user_exit_assignments(user_id).await?;
        }

        Ok(json!({
            "accepted": true,
            "line_probes": line_count,
            "exit_probes": exit_count
        }))
    }

    pub async fn record_admin_exit_endpoint_probe_request(
        &self,
        exit_endpoint_id: Uuid,
    ) -> Result<Value, DbError> {
        let mut tx = self.pool.begin().await?;
        ensure_exit_endpoint_exists_in_tx(&mut tx, exit_endpoint_id).await?;
        let access_node_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH target_nodes AS (
                SELECT DISTINCT l.access_node_id
                FROM access_lines l
                JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
                WHERE m.exit_endpoint_id = $1
                UNION
                SELECT r.access_node_id
                FROM exit_endpoints e
                JOIN exit_resources r ON r.id = e.exit_resource_id
                WHERE e.id = $1
                  AND r.ownership = 'self_hosted'
                  AND r.access_node_id IS NOT NULL
            )
            SELECT n.id
            FROM target_nodes t
            JOIN access_nodes n ON n.id = t.access_node_id
            WHERE n.agent_token_hash <> ''
            ORDER BY n.id
            "#,
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut *tx)
        .await?;

        for access_node_id in &access_node_ids {
            sqlx::query(
                r#"
                INSERT INTO access_exit_probes (
                    access_node_id, exit_endpoint_id, status,
                    latency_ms, error_summary, probed_at
                )
                SELECT $1, $2, 'queued', NULL, 'admin requested probe', now()
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM access_exit_probes p
                    WHERE p.access_node_id = $1
                      AND p.exit_endpoint_id = $2
                      AND p.status = 'queued'
                      AND NOT EXISTS (
                          SELECT 1
                          FROM access_exit_probes r
                          WHERE r.access_node_id = p.access_node_id
                            AND r.exit_endpoint_id = p.exit_endpoint_id
                            AND r.status <> 'queued'
                            AND r.probed_at >= p.probed_at
                      )
                )
                "#,
            )
            .bind(access_node_id)
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
            mark_access_node_dirty_in_tx(
                &mut tx,
                *access_node_id,
                "admin_requested_exit_endpoint_probe",
            )
            .await?;
        }
        tx.commit().await?;
        Ok(json!({
            "exit_endpoint_id": exit_endpoint_id,
            "queued_access_nodes": access_node_ids
        }))
    }
}
