//! 数据库查询行模型 assignments。
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
pub(crate) struct UserAccessLineAssignmentRow {
    pub(crate) line_group_id: Uuid,
    pub(crate) access_line_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserExitAssignmentRow {
    pub(crate) user_id: Uuid,
    pub(crate) access_line_id: Uuid,
    pub(crate) exit_pool_id: Uuid,
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) assigned_at: DateTime<Utc>,
    pub(crate) failover_reason: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct AccessExitProbeStateRow {
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) effective_status: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct AccessExitProbeStateForUpdateRow {
    pub(crate) effective_status: String,
    pub(crate) consecutive_failures: i32,
    pub(crate) consecutive_successes: i32,
    pub(crate) last_probe_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct PendingProbeTaskRow {
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) requested_at: DateTime<Utc>,
    pub(crate) outbound_type: String,
    pub(crate) host: String,
    pub(crate) port: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct TimedOutProbeTaskRow {
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) probed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AssignmentSubscriptionRow {
    pub(crate) plan_id: Uuid,
    pub(crate) remaining_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct AssignmentPolicyRow {
    pub(crate) line_group_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct EligibleAccessLineRow {
    pub(crate) id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct AssignedAccessLineExitPoolRow {
    pub(crate) access_line_id: Uuid,
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_endpoint_id: Option<Uuid>,
    pub(crate) exit_pool_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct LineRuntimeContextRow {
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_pool_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct ExistingExitAssignmentRow {
    pub(crate) exit_endpoint_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct EligibleExitEndpointRow {
    pub(crate) exit_endpoint_id: Uuid,
    pub(crate) weight: i32,
    pub(crate) priority: i32,
}
