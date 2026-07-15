//! 运行态状态、快照和基础转换 helper。
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

pub(crate) fn runtime_metric_status(
    metric_line_count: i64,
    _expected_line_count: i64,
    latest_metric_at: Option<DateTime<Utc>>,
) -> &'static str {
    if metric_line_count <= 0 {
        return "no_data";
    }
    let Some(latest_metric_at) = latest_metric_at else {
        return "unknown";
    };
    if latest_metric_at < Utc::now() - Duration::minutes(10) {
        "stale"
    } else {
        "fresh"
    }
}

pub(crate) async fn line_runtime_context_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    access_line_id: Uuid,
) -> Result<LineRuntimeContextRow, DbError> {
    let context = sqlx::query_as::<_, LineRuntimeContextRow>(
        r#"
        SELECT access_node_id, exit_pool_id
        FROM access_lines
        WHERE id = $1
        "#,
    )
    .bind(access_line_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(DbError::AccessLineNotFound)?;
    if context.access_node_id != access_node_id {
        return Err(DbError::AccessLineNodeMismatch);
    }
    Ok(context)
}

pub(crate) async fn upsert_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    report: &TrafficReport,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO access_traffic_snapshots (
            access_line_id, xray_user_key, uplink_total, downlink_total,
            collected_at, access_node_id
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (access_line_id, xray_user_key) DO UPDATE SET
            uplink_total = EXCLUDED.uplink_total,
            downlink_total = EXCLUDED.downlink_total,
            collected_at = EXCLUDED.collected_at,
            access_node_id = EXCLUDED.access_node_id
        "#,
    )
    .bind(report.access_line_id)
    .bind(&report.xray_user_key)
    .bind(to_i64(report.uplink_total))
    .bind(to_i64(report.downlink_total))
    .bind(report.collected_at)
    .bind(access_node_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("static uuid must be valid")
}

pub(crate) fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(crate) fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn traffic_refresh_reason(line_enabled: bool, user_disabled: bool) -> &'static str {
    if !line_enabled {
        "traffic_access_line_disabled"
    } else if user_disabled {
        "traffic_user_disabled"
    } else {
        "traffic_access_line_unbound"
    }
}

pub(crate) fn endpoint_type(value: &str) -> EndpointType {
    match value {
        "socks" => EndpointType::Socks,
        "http" => EndpointType::Http,
        "vless" => EndpointType::Vless,
        "trojan" => EndpointType::Trojan,
        "shadowsocks" => EndpointType::Shadowsocks,
        "hysteria" => EndpointType::Hysteria,
        _ => EndpointType::Direct,
    }
}
