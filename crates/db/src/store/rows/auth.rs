//! 数据库查询行模型 auth。
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
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) xray_user_key: String,
    pub(crate) access_credential: String,
    pub(crate) disabled: bool,
    pub(crate) rate_limit_bps: Option<i64>,
    pub(crate) rate_limit_up_bps: Option<i64>,
    pub(crate) rate_limit_down_bps: Option<i64>,
}

#[derive(Debug, FromRow)]
pub(crate) struct SubscriptionRow {
    pub(crate) user_id: Uuid,
    pub(crate) plan_id: Uuid,
    pub(crate) active: bool,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) used_bytes: i64,
    pub(crate) limit_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct TokenRow {
    pub(crate) token: String,
    pub(crate) user_id: Uuid,
}

#[derive(Debug, FromRow)]
pub(crate) struct SnapshotRow {
    pub(crate) access_line_id: Uuid,
    pub(crate) xray_user_key: String,
    pub(crate) uplink_total: i64,
    pub(crate) downlink_total: i64,
    pub(crate) collected_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct BillingLineRow {
    pub(crate) access_node_id: Uuid,
    pub(crate) exit_pool_id: Uuid,
    pub(crate) enabled: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct BillingUserRow {
    pub(crate) id: Uuid,
    pub(crate) disabled: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct BillingSubscriptionRow {
    pub(crate) used_bytes: i64,
    pub(crate) limit_bytes: i64,
    pub(crate) billing_multiplier: f64,
}

#[cfg(test)]
#[derive(Debug, FromRow)]
pub(crate) struct LedgerAuditRow {
    pub(crate) delta_total: i64,
    pub(crate) billed_uplink: i64,
    pub(crate) billed_downlink: i64,
    pub(crate) billed_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct AuthUserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) password_hash: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) display_name: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct AuthChallengeRow {
    pub(crate) id: Uuid,
    pub(crate) target_hash: String,
    pub(crate) code_hash: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct RefreshUserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) display_name: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct RefreshTokenUserRow {
    pub(crate) refresh_token_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) email: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) display_name: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct DefaultPlanRow {
    pub(crate) id: Uuid,
    pub(crate) traffic_limit_bytes: i64,
}
