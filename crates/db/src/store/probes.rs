//! 探测策略、状态机和探测摘要写入 helper。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::rows::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AccessProbePolicy {
    pub(crate) exit_auto_failover_enabled: bool,
    pub(crate) exit_failure_threshold: i64,
    pub(crate) exit_recovery_threshold: i64,
    pub(crate) exit_window_minutes: i64,
    pub(crate) exit_probe_interval_seconds: i64,
    pub(crate) probe_queue_batch_size: i64,
    pub(crate) max_pending_probe_tasks: i64,
    pub(crate) max_probe_task_delivery_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    Healthy,
    Offline,
}

pub(crate) struct ProbeStateTransition {
    pub(crate) effective_status: &'static str,
    pub(crate) changed: bool,
}

pub(crate) struct ProbeStateReport<'a> {
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) outcome: ProbeOutcome,
    pub(crate) probe_status: &'a str,
    pub(crate) latency_ms: Option<i32>,
    pub(crate) error_summary: String,
    pub(crate) probed_at: DateTime<Utc>,
}

pub(crate) async fn apply_exit_probe_state_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    policy: &AccessProbePolicy,
    report: ProbeStateReport<'_>,
) -> Result<Option<ProbeStateTransition>, DbError> {
    if !policy.exit_auto_failover_enabled {
        return Ok(None);
    }

    let previous = sqlx::query_as::<_, AccessExitProbeStateForUpdateRow>(
        r#"
        SELECT effective_status, consecutive_failures, consecutive_successes, last_probe_at
        FROM access_exit_probe_states
        WHERE access_node_id = $1 AND exit_endpoint_id = $2
        FOR UPDATE
        "#,
    )
    .bind(report.access_node_id)
    .bind(report.exit_endpoint_id)
    .fetch_optional(&mut **tx)
    .await?;

    if previous
        .as_ref()
        .and_then(|row| row.last_probe_at)
        .is_some_and(|last_probe_at| report.probed_at < last_probe_at)
    {
        return Ok(None);
    }

    let previous_status = previous
        .as_ref()
        .map(|row| row.effective_status.as_str())
        .unwrap_or("unknown");
    let mut failures = previous
        .as_ref()
        .map(|row| row.consecutive_failures.max(0))
        .unwrap_or_default();
    let mut successes = previous
        .as_ref()
        .map(|row| row.consecutive_successes.max(0))
        .unwrap_or_default();
    let within_window = previous
        .as_ref()
        .and_then(|row| row.last_probe_at)
        .is_some_and(|last_probe_at| {
            last_probe_at >= report.probed_at - Duration::minutes(policy.exit_window_minutes)
        });
    if !within_window {
        failures = 0;
        successes = 0;
    }
    let mut next_status = previous_status.to_string();

    match report.outcome {
        ProbeOutcome::Offline => {
            failures = failures.saturating_add(1);
            successes = 0;
            if i64::from(failures) >= policy.exit_failure_threshold {
                next_status = "offline".to_string();
            }
        }
        ProbeOutcome::Healthy => {
            successes = successes.saturating_add(1);
            failures = 0;
            if previous_status == "offline" && i64::from(successes) < policy.exit_recovery_threshold
            {
                next_status = previous_status.to_string();
            } else if i64::from(successes) >= policy.exit_recovery_threshold
                || previous_status == "unknown"
            {
                next_status = "healthy".to_string();
            }
        }
    }

    let changed = next_status != previous_status;
    sqlx::query(
        r#"
        INSERT INTO access_exit_probe_states (
            access_node_id, exit_endpoint_id, effective_status,
            consecutive_failures, consecutive_successes,
            last_probe_status, last_latency_ms, last_error_summary,
            last_probe_at, status_changed_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                CASE WHEN $10 THEN now() ELSE NULL END, now())
        ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
            effective_status = EXCLUDED.effective_status,
            consecutive_failures = EXCLUDED.consecutive_failures,
            consecutive_successes = EXCLUDED.consecutive_successes,
            last_probe_status = EXCLUDED.last_probe_status,
            last_latency_ms = EXCLUDED.last_latency_ms,
            last_error_summary = EXCLUDED.last_error_summary,
            last_probe_at = EXCLUDED.last_probe_at,
            status_changed_at = CASE
                WHEN $10 THEN now()
                ELSE access_exit_probe_states.status_changed_at
            END,
            updated_at = now()
        "#,
    )
    .bind(report.access_node_id)
    .bind(report.exit_endpoint_id)
    .bind(&next_status)
    .bind(failures)
    .bind(successes)
    .bind(report.probe_status)
    .bind(report.latency_ms)
    .bind(report.error_summary)
    .bind(report.probed_at)
    .bind(changed)
    .execute(&mut **tx)
    .await?;

    Ok(Some(ProbeStateTransition {
        effective_status: if next_status == "offline" {
            "offline"
        } else {
            "healthy"
        },
        changed,
    }))
}

pub(crate) async fn sync_exit_probe_summary_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
    outcome: ProbeOutcome,
    probe_status: &str,
    error_summary: String,
    probed_at: DateTime<Utc>,
    transition: Option<&ProbeStateTransition>,
) -> Result<(), DbError> {
    let resource_status = transition
        .map(|transition| transition.effective_status)
        .unwrap_or(match outcome {
            ProbeOutcome::Healthy => "healthy",
            ProbeOutcome::Offline => "degraded",
        });
    let endpoint_error = match outcome {
        ProbeOutcome::Healthy => String::new(),
        ProbeOutcome::Offline => truncate_error_summary(&error_summary, 512),
    };

    sqlx::query(
        r#"
        UPDATE exit_endpoints
        SET last_probe_at = $2,
            last_probe_status = $3
        WHERE id = $1
          AND (last_probe_at IS NULL OR last_probe_at <= $2)
        "#,
    )
    .bind(exit_endpoint_id)
    .bind(probed_at)
    .bind(probe_status)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE exit_resources r
        SET status = $4,
            last_probe_at = $2,
            last_probe_status = $3,
            last_endpoint_error = $5
        FROM exit_endpoints e
        WHERE e.id = $1
          AND r.id = e.exit_resource_id
          AND (r.last_probe_at IS NULL OR r.last_probe_at <= $2)
        "#,
    )
    .bind(exit_endpoint_id)
    .bind(probed_at)
    .bind(probe_status)
    .bind(resource_status)
    .bind(endpoint_error)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub(crate) async fn users_assigned_to_exit_endpoint_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    exit_endpoint_id: Uuid,
) -> Result<Vec<Uuid>, DbError> {
    let user_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT uea.user_id
        FROM user_exit_assignments uea
        JOIN access_lines l ON l.id = uea.access_line_id
        WHERE uea.exit_endpoint_id = $1
          AND l.access_node_id = $2
        ORDER BY uea.user_id
        "#,
    )
    .bind(exit_endpoint_id)
    .bind(access_node_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(user_ids)
}

pub(crate) fn probe_status_to_outcome(value: &str) -> Option<ProbeOutcome> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ok" | "up" | "online" | "success" | "healthy" => Some(ProbeOutcome::Healthy),
        "fail" | "failed" | "down" | "offline" | "error" | "timeout" | "unhealthy" => {
            Some(ProbeOutcome::Offline)
        }
        _ => None,
    }
}
