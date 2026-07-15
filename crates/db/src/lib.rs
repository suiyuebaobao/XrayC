//! 本文件实现 XrayC V2 的 PostgreSQL 持久化层。
//! 本 crate 负责 SQLx 连接池、迁移、演示种子数据和读模型。
//! 这里保留 PgStore 类型、事务流程、SQL 查询和持久化结构。
//! 纯校验函数已拆到 validation 模块，避免主文件继续膨胀。
//! 通用 JSON 和数值辅助函数已拆到 helpers 模块。
//! 读模型 JSON 与 Xray 渲染 helper 已拆到专门模块。
//! 审计摘要脱敏逻辑和公开 DTO 类型继续由 crate root 汇总。
//! PgStore 大方法已按业务域拆入 store 模块，入口路径不变。
//! 后续新增数据库业务应放在对应 store 子模块或事务 helper 中。
//! 本文件仍是 db crate 的公开入口，避免调用方导入路径变化。

#![allow(unused_imports)]

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex as StdMutex, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use thiserror::Error;
use uuid::Uuid;
use xrayc_core::{
    generate_clash_yaml_with_options, AccessExitProbeState, AccessLine, AccessNode, EndpointType,
    ExitEndpoint, ExitPool, LineGroup, LineGroupRuleSetBinding, NodeDomainView, Plan,
    PlanLineGroup, StoreData, SubscriptionError, SubscriptionOptions, SubscriptionToken,
    TrafficReport, TrafficReportResult, TrafficSnapshot, User, UserAccessLineAssignment,
    UserExitAssignment, UserSubscription,
};
#[cfg(test)]
use xrayc_xray_config::{AccessConfig as XrayAccessConfig, AccessProtocol as XrayAccessProtocol};

mod admin_read_models;
mod audit_sanitize;
pub mod cloudflare_ranges;
mod heartbeat_read_model;
mod helpers;
mod protocol_guardrails;
mod read_models;
mod settings_backup_json;
mod settings_json;
mod settings_payment_json;
mod shadowsocks_keys;
mod store;
mod subscription_read_models;
mod types;
mod types_worker;
mod validation;
mod xray_protocol;
mod xray_render;
mod xray_render_cert;
mod xray_render_local;

use admin_read_models::{
    admin_audit_log_json, admin_collection_json, admin_invite_code_json, admin_plan_json,
    admin_user_json, is_successful_payment_status, ledger_totals_json,
    operations_ledger_ranking_row_json, operations_ledger_ranking_totals_json, order_json,
    payment_callback_response, redeem_code_json, user_invite_code_json,
};
#[cfg(test)]
pub(crate) use audit_sanitize::public_audit_summary;
use audit_sanitize::stored_audit_summary;
pub use heartbeat_read_model::{heartbeat_json, heartbeat_json_with_probe_policy};
pub use helpers::bytes_to_gb;
use helpers::{
    normalize_udp_packet_encoding, normalized_filter, payload_client_ip_hash, payload_i32,
    payload_i32_any, payload_items_with_root_field, payload_optional_i32, payload_optional_time,
    payload_required_text, payload_session_status, payload_text, payload_time, payload_u64,
    payload_uuid, safe_client_ip_summary, truncate_error_summary,
};
pub use read_models::{
    access_routing_json, exit_pools_json, operations_ledger_ranking_json, operations_summary_json,
};
use settings_json::{
    access_probe_policy_from_setting, database_backup_policy_from_setting,
    default_access_operations_setting, default_auth_security_setting,
    default_sales_landing_setting, default_subscription_setting,
    normalize_access_operations_setting, normalize_subscription_setting, public_auth_security,
    traffic_log_retention_policy_from_setting,
};
// 支付配置默认值、脱敏函数与 write-only 路径需要在 store/tests 跨模块可见，
// 按 merge_json 同样用 pub(crate) use 暴露。
use protocol_guardrails::*;
pub(crate) use settings_backup_json::{
    default_backup_config, normalize_backup_config, public_backup_config,
};
pub(crate) use settings_payment_json::{
    default_payment_setting, public_payment_settings, PAYMENT_WRITE_ONLY_PATHS,
};
pub(crate) use store::json_util::merge_json;
pub use store::node_metrics::NodeRuntimeMetricsReport;
pub(crate) use store::probes::AccessProbePolicy;
pub(crate) use store::rows::*;
pub use store::security::agent_token_hash;
pub use subscription_read_models::{
    user_subscription_json, user_subscription_json_for_user,
    user_subscription_json_for_user_with_probe_policy,
};
pub use types::*;
pub use types_worker::*;
use validation::*;
#[cfg(test)]
pub(crate) use xray_protocol::{access_protocol_for_line, exit_endpoint_tag};
pub(crate) use xray_protocol::{config_text, non_empty_text, vless_security};
use xray_render::{
    access_config_hash, build_access_config_with_probe_policy, empty_access_config_hash,
    selected_node_id,
};

pub const DEMO_AGENT_TOKEN: &str = "demo-agent-token";
pub const DEMO_ADMIN_PASSWORD: &str = "admin123456";
pub const DEMO_USER_PASSWORD: &str = "demo123456";
const DEFAULT_SUBSCRIPTION_TOKEN_TTL_DAYS: i64 = 3650;
const ADMIN_AUDIT_LOG_MAX_PAGE_SIZE: i64 = 200;
const ADMIN_ORDER_MAX_PAGE_SIZE: i64 = 200;
const SCHEDULED_EXIT_PROBE_QUEUE_LOCK_ID: i64 = 202_605_200_001;
const PROBE_TASK_LEASE_SECONDS: i64 = 60;
const DATABASE_BACKUP_LOCK_ID: i64 = 202_606_090_001;

pub struct DatabaseBackupLockGuard {
    tx: Option<sqlx::Transaction<'static, sqlx::Postgres>>,
}

impl DatabaseBackupLockGuard {
    pub async fn release(mut self) -> Result<(), DbError> {
        if let Some(tx) = self.tx.take() {
            tx.rollback().await?;
        }
        Ok(())
    }
}

fn subscription_token_ttl_days() -> i64 {
    std::env::var("SUBSCRIPTION_TOKEN_TTL_DAYS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_SUBSCRIPTION_TOKEN_TTL_DAYS)
        .clamp(1, DEFAULT_SUBSCRIPTION_TOKEN_TTL_DAYS)
}

fn subscription_token_expires_at() -> DateTime<Utc> {
    Utc::now() + Duration::days(subscription_token_ttl_days())
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Subscription(#[from] SubscriptionError),
    #[error("中转入口不存在")]
    AccessLineNotFound,
    #[error("用户不存在")]
    UserNotFound,
    #[error("中转入口不属于当前中转节点")]
    AccessLineNodeMismatch,
    #[error("密码哈希处理失败: {0}")]
    PasswordHash(String),
    #[error("邮箱格式无效")]
    InvalidEmail,
    #[error("密码长度不能少于 8 位")]
    WeakPassword,
    #[error("邮箱已注册")]
    EmailExists,
    #[error("账号已禁用")]
    UserDisabled,
    #[error("管理员权限已撤销")]
    AdminRequired,
    #[error("邀请码无效或已使用")]
    InvalidInviteCode,
    #[error("当前不允许用户自助生成邀请码")]
    InviteGenerationDisabled,
    #[error("邀请码生成数量已达到上限")]
    InviteQuotaExceeded,
    #[error("验证码无效或已过期")]
    InvalidAuthChallenge,
    #[error("基础套餐不存在")]
    DefaultPlanNotFound,
    #[error("Agent 上报数据无效: {0}")]
    InvalidAgentPayload(String),
    #[error("数据无效: {0}")]
    InvalidInput(String),
    #[error("支付回调无效: {0}")]
    InvalidPaymentCallback(String),
    // 兑换码是终端用户可见的业务错误,必须用干净专属消息;
    // 不能复用 InvalidAgentPayload(其 Display 会带 "Agent 上报数据无效:" 前缀)。
    #[error("兑换码无效")]
    RedeemCodeInvalid,
    #[error("兑换码不存在")]
    RedeemCodeNotFound,
    #[error("兑换码已使用")]
    RedeemCodeUsed,
    #[error("兑换码已过期")]
    RedeemCodeExpired,
    #[error("兑换码生成数量必须在 1 到 100 之间")]
    RedeemCountOutOfRange,
}

#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
}

#[cfg(test)]
mod tests;
