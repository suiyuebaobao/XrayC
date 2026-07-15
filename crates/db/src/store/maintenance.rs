//! Worker 维护任务和探测任务调度。
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
use super::maintenance_usage_rollups::roll_up_and_prune_usage_ledgers_in_tx;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn run_worker_maintenance(
        &self,
        runtime_retention_days: i64,
    ) -> Result<WorkerMaintenanceResult, DbError> {
        let runtime_retention_days = runtime_retention_days.clamp(1, 3650);
        let traffic_log_retention = self.traffic_log_retention_policy().await?;
        let mut tx = self.pool.begin().await?;

        let default_plan = sqlx::query_as::<_, (Uuid, i64, i32)>(
            r#"
            SELECT id, traffic_limit_bytes, duration_days
            FROM plans
            WHERE is_default = TRUE AND enabled = TRUE AND is_deleted = FALSE
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(default_plan) = default_plan else {
            // 无默认套餐(如刚装机 api 尚未 bootstrap 完):本轮跳过套餐周期维护,
            // 下一轮再跑,不报 error(避免启动竞态刷误导性 DefaultPlanNotFound)。
            tx.rollback().await?;
            return Ok(WorkerMaintenanceResult::default());
        };
        let default_duration_days = i64::from(default_plan.2.max(1));

        // 到期订阅统一回落到基础套餐；基础套餐自身到期则开启下一个免费周期。
        let expired_subscriptions = sqlx::query_scalar::<_, i64>(
            r#"
            WITH expired_users AS (
                SELECT user_id
                FROM user_subscriptions
                WHERE active = TRUE AND expires_at <= now()
                FOR UPDATE
            ),
            reset_subscriptions AS (
                UPDATE user_subscriptions s
                SET plan_id = $1,
                    active = TRUE,
                    expires_at = now() + ($3::BIGINT * interval '1 day'),
                    used_bytes = 0,
                    limit_bytes = $2,
                    updated_at = now()
                FROM expired_users e
                WHERE s.user_id = e.user_id
                RETURNING s.user_id
            ),
            deleted_access AS (
                DELETE FROM user_access_line_assignments ula
                USING reset_subscriptions d
                WHERE ula.user_id = d.user_id
                RETURNING 1
            ),
            deleted_exit AS (
                DELETE FROM user_exit_assignments uea
                USING reset_subscriptions d
                WHERE uea.user_id = d.user_id
                RETURNING 1
            )
            SELECT COUNT(*)::BIGINT FROM reset_subscriptions
            "#,
        )
        .bind(default_plan.0)
        .bind(default_plan.1)
        .bind(default_duration_days)
        .fetch_one(&mut *tx)
        .await?;

        if expired_subscriptions > 0 {
            mark_all_nodes_dirty_in_tx(&mut tx, "worker_expired_subscription").await?;
        }

        let expired_orders = sqlx::query(
            r#"
            UPDATE orders
            SET status = 'expired',
                updated_at = now()
            WHERE status = 'pending' AND expires_at <= now()
            "#,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        // 刷新凭证保留已撤销记录 7 天，便于排查异常登录和退出行为。
        let expired_refresh_tokens = sqlx::query(
            r#"
            DELETE FROM refresh_tokens
            WHERE expires_at <= now()
               OR (revoked_at IS NOT NULL AND revoked_at <= now() - interval '7 days')
            "#,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        // 验证码只能短时有效，已过期或已使用的挑战不应长期占用表空间。
        let expired_auth_challenges = sqlx::query(
            r#"
            DELETE FROM auth_challenges
            WHERE expires_at <= now() OR used_at IS NOT NULL
            "#,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let expired_login_guards = sqlx::query(
            r#"
            DELETE FROM login_guard_states
            WHERE expires_at <= now()
              AND (locked_until IS NULL OR locked_until <= now())
            "#,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let (timed_out_exit_probe_tasks, probe_users_to_resync) = self
            .timeout_expired_access_exit_probe_tasks_in_tx(&mut tx)
            .await?;
        let queued_exit_probe_tasks = self
            .queue_due_access_exit_probe_tasks_in_tx(&mut tx)
            .await?;

        let pruned_metric_snapshots = sqlx::query(
            r#"
            DELETE FROM access_line_metric_snapshots
            WHERE collected_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        // 监控中心·节点运行态指标时序按同口径裁剪,过期采样不长期占用表空间。
        sqlx::query(
            r#"
            DELETE FROM node_runtime_metrics
            WHERE collected_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?;

        // 运行快照只服务最新 counter 基线和运营观测，超过保留期后可清理。
        let pruned_traffic_snapshots = sqlx::query(
            r#"
            DELETE FROM access_traffic_snapshots
            WHERE collected_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let pruned_user_sessions = sqlx::query(
            r#"
            DELETE FROM access_user_sessions
            WHERE last_seen_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let pruned_user_session_events = sqlx::query(
            r#"
            DELETE FROM access_user_session_events
            WHERE observed_at <= now() - ($1::BIGINT * interval '1 day')
               OR created_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let pruned_line_probes = sqlx::query(
            r#"
            DELETE FROM access_line_probes
            WHERE probed_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let pruned_exit_probes = sqlx::query(
            r#"
            DELETE FROM access_exit_probes
            WHERE probed_at <= now() - ($1::BIGINT * interval '1 day')
            "#,
        )
        .bind(runtime_retention_days)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let pruned_usage_ledgers = if traffic_log_retention.prune_enabled {
            roll_up_and_prune_usage_ledgers_in_tx(&mut tx, traffic_log_retention).await?
        } else {
            0
        };

        tx.commit().await?;
        for user_id in probe_users_to_resync {
            self.sync_user_exit_assignments(user_id).await?;
        }
        Ok(WorkerMaintenanceResult {
            expired_subscriptions: expired_subscriptions as u64,
            expired_orders,
            expired_refresh_tokens,
            expired_auth_challenges,
            expired_login_guards,
            timed_out_exit_probe_tasks,
            queued_exit_probe_tasks,
            pruned_metric_snapshots,
            pruned_traffic_snapshots,
            pruned_user_sessions,
            pruned_user_session_events,
            pruned_line_probes,
            pruned_exit_probes,
            pruned_usage_ledgers,
        })
    }
}
