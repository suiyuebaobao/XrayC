//! 数据库查询行模型 users_orders。
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
pub(crate) struct SubscriptionUserinfoRow {
    pub(crate) used_bytes: i64,
    pub(crate) limit_bytes: i64,
    pub(crate) expires_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserProfileRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) display_name: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserUsageRow {
    pub(crate) billed_bytes: i64,
    pub(crate) real_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminUserListRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) display_name: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) plan_id: Option<Uuid>,
    pub(crate) plan_name: Option<String>,
    pub(crate) subscription_active: Option<bool>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) used_bytes: i64,
    pub(crate) limit_bytes: i64,
    pub(crate) plan_rate_limit_bps: i64,
    pub(crate) user_rate_limit_bps: Option<i64>,
    pub(crate) user_rate_limit_up_bps: Option<i64>,
    pub(crate) user_rate_limit_down_bps: Option<i64>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminUserStateRow {
    pub(crate) email: String,
    pub(crate) disabled: bool,
    pub(crate) is_admin: bool,
    pub(crate) rate_limit_bps: Option<i64>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminUserPlanRow {
    pub(crate) id: Uuid,
    pub(crate) traffic_limit_bytes: i64,
    pub(crate) duration_days: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct PlanCheckoutRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) price_cents: i64,
    pub(crate) currency: String,
    pub(crate) duration_days: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct PlanRedeemSeedRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) traffic_limit_bytes: i64,
    pub(crate) duration_days: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct OrderListRow {
    pub(crate) id: Uuid,
    pub(crate) order_no: String,
    pub(crate) user_id: Uuid,
    pub(crate) user_email: String,
    pub(crate) plan_name: String,
    pub(crate) amount_cents: i64,
    pub(crate) currency: String,
    pub(crate) status: String,
    pub(crate) payment_address: String,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) paid_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) duration_days: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct PaymentOrderRow {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) plan_id: Uuid,
    pub(crate) plan_name: String,
    pub(crate) amount_cents: i64,
    pub(crate) currency: String,
    pub(crate) status: String,
    pub(crate) payment_address: String,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) traffic_limit_bytes: i64,
    pub(crate) duration_days: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct RedeemCodeRow {
    pub(crate) code: String,
    pub(crate) plan_id: Uuid,
    pub(crate) plan_name: String,
    pub(crate) traffic_bytes: Option<i64>,
    pub(crate) duration_days: Option<i32>,
    pub(crate) is_used: bool,
    pub(crate) used_by_user_id: Option<Uuid>,
    pub(crate) used_by_email: Option<String>,
    pub(crate) used_at: Option<DateTime<Utc>>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct RedeemApplyRow {
    pub(crate) code: String,
    pub(crate) plan_id: Uuid,
    pub(crate) plan_name: String,
    pub(crate) traffic_bytes: i64,
    pub(crate) duration_days: i32,
    pub(crate) is_used: bool,
    pub(crate) expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct InviteCodeStateRow {
    pub(crate) code: String,
    pub(crate) is_used: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserInviteCodeRow {
    pub(crate) code: String,
    pub(crate) is_used: bool,
    pub(crate) used_by_user_id: Option<Uuid>,
    pub(crate) used_by_email_snapshot: String,
    pub(crate) used_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminInviteCodeRow {
    pub(crate) code: String,
    pub(crate) inviter_user_id: Option<Uuid>,
    pub(crate) inviter_email_snapshot: String,
    pub(crate) is_used: bool,
    pub(crate) used_by_user_id: Option<Uuid>,
    pub(crate) used_by_email_snapshot: String,
    pub(crate) used_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct LoginGuardStateRow {
    pub(crate) failure_count: i32,
    pub(crate) expires_at: DateTime<Utc>,
}
