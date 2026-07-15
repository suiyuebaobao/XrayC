//! 数据库查询行模型 operations。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use crate::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub(crate) struct LineMetricLatestRow {
    pub(crate) access_line_id: Uuid,
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_pool_id: Uuid,
    pub(crate) online_users: i32,
    pub(crate) active_connections: i32,
    pub(crate) unique_client_ips: i32,
    pub(crate) uplink_rate_bps: i64,
    pub(crate) downlink_rate_bps: i64,
    pub(crate) collected_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineLedgerTotalsRow {
    pub(crate) ledger_count: i64,
    pub(crate) delta_uplink: i64,
    pub(crate) delta_downlink: i64,
    pub(crate) delta_total: i64,
    pub(crate) billed_uplink: i64,
    pub(crate) billed_downlink: i64,
    pub(crate) billed_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineLedgerWindowRow {
    pub(crate) window_start: DateTime<Utc>,
    pub(crate) ledger_count: i64,
    pub(crate) delta_uplink: i64,
    pub(crate) delta_downlink: i64,
    pub(crate) delta_total: i64,
    pub(crate) billed_uplink: i64,
    pub(crate) billed_downlink: i64,
    pub(crate) billed_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct OperationsLedgerRankingRow {
    pub(crate) access_line_id: Uuid,
    pub(crate) access_line_name: String,
    pub(crate) access_node_name: String,
    pub(crate) region_code: String,
    pub(crate) listen_host: String,
    pub(crate) listen_port: i32,
    pub(crate) ledger_count: i64,
    pub(crate) delta_uplink: i64,
    pub(crate) delta_downlink: i64,
    pub(crate) delta_total: i64,
    pub(crate) billed_uplink: i64,
    pub(crate) billed_downlink: i64,
    pub(crate) billed_bytes: i64,
    pub(crate) latest_collected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct OperationsLedgerRankingTotalsRow {
    pub(crate) access_line_count: i64,
    pub(crate) ledger_count: i64,
    pub(crate) delta_uplink: i64,
    pub(crate) delta_downlink: i64,
    pub(crate) delta_total: i64,
    pub(crate) billed_uplink: i64,
    pub(crate) billed_downlink: i64,
    pub(crate) billed_bytes: i64,
    pub(crate) latest_collected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineProbeLatestRow {
    pub(crate) access_line_id: Uuid,
    pub(crate) status: String,
    pub(crate) latency_ms: Option<i32>,
    pub(crate) error_summary: String,
    pub(crate) probed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AccessLineSessionRow {
    pub(crate) id: Uuid,
    pub(crate) access_line_id: Uuid,
    pub(crate) xray_user_key: String,
    pub(crate) client_ip_hash: String,
    pub(crate) active_connection_count: i32,
    pub(crate) status: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) last_seen_at: DateTime<Utc>,
    pub(crate) user_id: Option<Uuid>,
    pub(crate) email: Option<String>,
}

#[derive(Debug, FromRow)]
pub(crate) struct RuntimeTotalsRow {
    pub(crate) metric_line_count: i64,
    pub(crate) online_users: Option<i64>,
    pub(crate) active_connections: Option<i64>,
    pub(crate) unique_client_ips: Option<i64>,
    pub(crate) latest_metric_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct TrafficHealthWindowRow {
    pub(crate) today_real_bytes: i64,
    pub(crate) today_billed_bytes: i64,
    pub(crate) week_real_bytes: i64,
    pub(crate) week_billed_bytes: i64,
    pub(crate) month_real_bytes: i64,
    pub(crate) month_billed_bytes: i64,
    pub(crate) total_real_bytes: i64,
    pub(crate) total_billed_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct TrafficHealthEntityRow {
    pub(crate) entity_id: Uuid,
    pub(crate) entity_name: String,
    pub(crate) today_real_bytes: i64,
    pub(crate) week_real_bytes: i64,
    pub(crate) month_real_bytes: i64,
    pub(crate) month_billed_bytes: i64,
    pub(crate) total_real_bytes: i64,
    pub(crate) total_billed_bytes: i64,
    pub(crate) peak_hour_real_bytes: i64,
    pub(crate) low_hour_real_bytes: i64,
    pub(crate) average_daily_real_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct TrafficHealthChartRow {
    pub(crate) entity_id: Uuid,
    pub(crate) window_start: DateTime<Utc>,
    pub(crate) real_bytes: i64,
    pub(crate) billed_bytes: i64,
}
