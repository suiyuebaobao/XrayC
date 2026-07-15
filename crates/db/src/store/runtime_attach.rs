//! 读模型运行态指标挂载逻辑。
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
use std::collections::HashMap;
use uuid::Uuid;

impl PgStore {
    pub(crate) async fn attach_latest_line_runtime(
        &self,
        mut value: Value,
    ) -> Result<Value, DbError> {
        let metrics = sqlx::query_as::<_, LineMetricLatestRow>(
            r#"
            SELECT DISTINCT ON (access_line_id)
                m.access_line_id,
                COALESCE(m.access_node_id, l.access_node_id) AS access_node_id,
                COALESCE(m.exit_pool_id, l.exit_pool_id) AS exit_pool_id,
                m.online_users, m.active_connections, m.unique_client_ips,
                m.uplink_rate_bps, m.downlink_rate_bps, m.collected_at
            FROM access_line_metric_snapshots m
            JOIN access_lines l ON l.id = m.access_line_id
            ORDER BY m.access_line_id, m.collected_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| (row.access_line_id, row))
        .collect::<HashMap<_, _>>();
        let probes = sqlx::query_as::<_, LineProbeLatestRow>(
            r#"
            SELECT DISTINCT ON (access_line_id)
                access_line_id, status, latency_ms, error_summary, probed_at
            FROM access_line_probes
            ORDER BY access_line_id, probed_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| (row.access_line_id, row))
        .collect::<HashMap<_, _>>();

        if let Some(lines) = value.get_mut("access_lines").and_then(Value::as_array_mut) {
            for line in lines {
                let Some(access_line_id) = line
                    .get("uuid")
                    .or_else(|| line.get("id"))
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                else {
                    continue;
                };
                let Some(object) = line.as_object_mut() else {
                    continue;
                };
                if let Some(metric) = metrics.get(&access_line_id) {
                    object.insert("online_users".to_string(), json!(metric.online_users));
                    object.insert(
                        "active_connections".to_string(),
                        json!(metric.active_connections),
                    );
                    object.insert(
                        "unique_client_ips".to_string(),
                        json!(metric.unique_client_ips),
                    );
                    object.insert("uplink_rate_bps".to_string(), json!(metric.uplink_rate_bps));
                    object.insert(
                        "downlink_rate_bps".to_string(),
                        json!(metric.downlink_rate_bps),
                    );
                    object.insert(
                        "metric_collected_at".to_string(),
                        json!(metric.collected_at),
                    );
                    object.insert(
                        "metric_status".to_string(),
                        json!(runtime_metric_status(1, 1, Some(metric.collected_at))),
                    );
                }
                if let Some(probe) = probes.get(&access_line_id) {
                    object.insert("latency_ms".to_string(), json!(probe.latency_ms));
                    object.insert("probe_status".to_string(), json!(probe.status));
                    object.insert("last_probe_at".to_string(), json!(probe.probed_at));
                    object.insert(
                        "probe_error_summary".to_string(),
                        json!(probe.error_summary),
                    );
                    if object.get("status").and_then(Value::as_str) == Some("enabled")
                        && probe.status != "healthy"
                    {
                        object.insert("status".to_string(), json!("degraded"));
                    }
                }
            }
        }
        Ok(value)
    }

    pub(crate) async fn attach_operations_runtime(
        &self,
        mut value: Value,
    ) -> Result<Value, DbError> {
        let totals = sqlx::query_as::<_, RuntimeTotalsRow>(
            r#"
            SELECT
                COUNT(*)::BIGINT AS metric_line_count,
                SUM(online_users)::BIGINT AS online_users,
                SUM(active_connections)::BIGINT AS active_connections,
                SUM(unique_client_ips)::BIGINT AS unique_client_ips,
                MAX(collected_at) AS latest_metric_at
            FROM (
                SELECT DISTINCT ON (access_line_id)
                    access_line_id, online_users, active_connections, unique_client_ips, collected_at
                FROM access_line_metric_snapshots
                ORDER BY access_line_id, collected_at DESC
            ) latest
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        if let Some(object) = value.as_object_mut() {
            let expected_line_count = object
                .get("access_line_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let runtime_metric_status = runtime_metric_status(
                totals.metric_line_count,
                expected_line_count,
                totals.latest_metric_at,
            );
            if runtime_metric_status == "fresh" || runtime_metric_status == "stale" {
                object.insert("online_users".to_string(), json!(totals.online_users));
                object.insert(
                    "active_connections".to_string(),
                    json!(totals.active_connections),
                );
                object.insert(
                    "unique_client_ips".to_string(),
                    json!(totals.unique_client_ips),
                );
            } else {
                object.insert("online_users".to_string(), Value::Null);
                object.insert("active_connections".to_string(), Value::Null);
                object.insert("unique_client_ips".to_string(), Value::Null);
            }
            object.insert(
                "runtime_metric_status".to_string(),
                json!(runtime_metric_status),
            );
            object.insert("metric_status".to_string(), json!(runtime_metric_status));
            object.insert(
                "runtime_metric_line_count".to_string(),
                json!(totals.metric_line_count),
            );
            object.insert(
                "runtime_metric_expected_line_count".to_string(),
                json!(expected_line_count),
            );
            object.insert(
                "latest_metric_at".to_string(),
                json!(totals.latest_metric_at),
            );
            object.insert(
                "traffic_health".to_string(),
                self.operations_traffic_health_json().await?,
            );
        }
        Ok(value)
    }
}
