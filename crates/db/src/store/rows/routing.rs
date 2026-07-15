//! 数据库查询行模型 routing。
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
pub(crate) struct AccessLineRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) access_node_id: Uuid,
    pub(crate) line_group_id: Option<Uuid>,
    pub(crate) exit_endpoint_id: Option<Uuid>,
    pub(crate) exit_pool_id: Uuid,
    pub(crate) listen_host: String,
    pub(crate) listen_port: i32,
    pub(crate) protocol: String,
    pub(crate) transport: String,
    pub(crate) user_uuid: String,
    pub(crate) server_name: String,
    pub(crate) public_key: String,
    pub(crate) short_id: String,
    pub(crate) flow: String,
    pub(crate) inbound_config: Value,
    pub(crate) udp_enabled: bool,
    pub(crate) udp_packet_encoding: String,
    pub(crate) xhttp_path: String,
    pub(crate) xhttp_host: String,
    pub(crate) xhttp_mode: String,
    pub(crate) region_code: String,
    pub(crate) region_name: String,
    pub(crate) region_flag: String,
    pub(crate) enabled: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineGroupRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) country_code: String,
    pub(crate) icon: String,
    pub(crate) exit_pool_id: Option<Uuid>,
    pub(crate) sort_weight: i32,
    pub(crate) billing_multiplier: f64,
    pub(crate) enabled: bool,
    pub(crate) dedicated_rules: Value,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineGroupLineRow {
    pub(crate) line_group_id: Uuid,
    pub(crate) exit_endpoint_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineGroupBindingNodeRow {
    pub(crate) line_group_id: Uuid,
    pub(crate) entry_exit_binding_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineGroupRuleSetBindingRow {
    pub(crate) line_group_id: Uuid,
    pub(crate) rule_set_id: Uuid,
    pub(crate) rule_set_name: String,
    pub(crate) binding_enabled: bool,
    pub(crate) rule_set_enabled: bool,
    pub(crate) position: i32,
    pub(crate) rules: Value,
}

#[derive(Debug, FromRow)]
pub(crate) struct PlanLineGroupRow {
    pub(crate) plan_id: Uuid,
    pub(crate) line_group_id: Uuid,
    pub(crate) billing_multiplier: f64,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminPlanLineGroupRow {
    pub(crate) plan_id: Uuid,
    pub(crate) line_group_id: Uuid,
    pub(crate) sort_weight: i32,
    pub(crate) billing_multiplier: f64,
    pub(crate) enabled: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct PlanRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) is_default: bool,
    pub(crate) traffic_limit_bytes: i64,
    pub(crate) rate_limit_bps: i64,
    pub(crate) rate_limit_up_bps: Option<i64>,
    pub(crate) rate_limit_down_bps: Option<i64>,
    pub(crate) billing_multiplier: f64,
    pub(crate) price_cents: i64,
    pub(crate) currency: String,
    pub(crate) duration_days: i32,
    pub(crate) sort_weight: i32,
    pub(crate) default_line_group_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminPlanRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) is_default: bool,
    pub(crate) traffic_limit_bytes: i64,
    pub(crate) rate_limit_bps: i64,
    pub(crate) rate_limit_up_bps: Option<i64>,
    pub(crate) rate_limit_down_bps: Option<i64>,
    pub(crate) billing_multiplier: f64,
    pub(crate) enabled: bool,
    pub(crate) price_cents: i64,
    pub(crate) currency: String,
    pub(crate) duration_days: i32,
    pub(crate) sort_weight: i32,
    pub(crate) is_deleted: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) default_line_group_id: Option<Uuid>,
}
