//! 审计记录模块。
//! 本文件由原 API 入口按路由域拆分而来。
//! 只移动 handler 与相关 helper，不改变路由、字段和状态码。
//! 模块保持 crate 内可见，供 lib.rs 路由装配使用。
//! 响应体、cookie、token 与审计摘要沿用原实现。
//! 数据库访问仍通过既有 PgStore 方法完成。
//! 内存模式回退逻辑保持原有分支。
//! 新增代码控制在 500 行以内便于审阅。
//! 中文注释位于文件前十行满足仓库约束。
//! 请勿在此写入部署主机、密钥或其它敏感信息。

use super::*;
pub(crate) fn json_uuid_field(value: &serde_json::Value, field: &str) -> Option<Uuid> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

pub(crate) async fn record_admin_audit(
    pg: &PgStore,
    claims: &AccessClaims,
    headers: &HeaderMap,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    request_summary: serde_json::Value,
) {
    record_admin_audit_with_result(
        pg,
        claims,
        headers,
        AdminAuditRecord {
            action,
            resource_type,
            resource_id,
            request_summary,
            result: "success",
        },
    )
    .await;
}

pub(crate) async fn record_user_audit(
    pg: &PgStore,
    claims: &AccessClaims,
    headers: &HeaderMap,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    request_summary: serde_json::Value,
) {
    if let Err(error) = pg
        .record_audit_log(AuditLogInput {
            actor_user_id: claims.user_id(),
            actor_email: claims.email.clone(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id,
            client_ip: rate_limit_identity_hash(headers),
            request_summary,
            result: "success".to_string(),
        })
        .await
    {
        tracing::warn!(
            %action,
            %resource_type,
            %error,
            "用户审计日志写入失败"
        );
    }
}

pub(crate) async fn record_system_audit(
    pg: &PgStore,
    headers: &HeaderMap,
    actor_user_id: Uuid,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    request_summary: serde_json::Value,
) {
    if let Err(error) = pg
        .record_audit_log(AuditLogInput {
            actor_user_id,
            actor_email: "system:payment_callback".to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id,
            client_ip: rate_limit_identity_hash(headers),
            request_summary,
            result: "success".to_string(),
        })
        .await
    {
        tracing::warn!(
            %action,
            %resource_type,
            %error,
            "系统审计日志写入失败"
        );
    }
}

pub(crate) async fn record_admin_audit_with_result(
    pg: &PgStore,
    claims: &AccessClaims,
    headers: &HeaderMap,
    record: AdminAuditRecord<'_>,
) {
    // 审计摘要必须由调用方显式传入，避免把密码、令牌或第三方代理凭据
    // 原样写入数据库。
    if let Err(error) = pg
        .record_audit_log(AuditLogInput {
            actor_user_id: claims.user_id(),
            actor_email: claims.email.clone(),
            action: record.action.to_string(),
            resource_type: record.resource_type.to_string(),
            resource_id: record.resource_id,
            client_ip: rate_limit_identity_hash(headers),
            request_summary: record.request_summary,
            result: record.result.to_string(),
        })
        .await
    {
        tracing::warn!(
            action = %record.action,
            resource_type = %record.resource_type,
            %error,
            "管理审计日志写入失败"
        );
    }
}

// 字段加密轮换处理器移除后暂无调用方，保留通用错误分类供后续审计复用。
#[allow(dead_code)]
pub(crate) fn db_error_category(error: &DbError) -> &'static str {
    match error {
        DbError::Sqlx(_) => "database",
        DbError::Migrate(_) => "migration",
        DbError::Subscription(_) => "subscription",
        DbError::InvalidInput(_) => "invalid_input",
        DbError::InvalidAgentPayload(_) => "invalid_agent_payload",
        DbError::InvalidPaymentCallback(_) => "invalid_payment_callback",
        DbError::AccessLineNotFound => "access_line_not_found",
        DbError::UserNotFound => "user_not_found",
        DbError::AccessLineNodeMismatch => "access_line_node_mismatch",
        DbError::PasswordHash(_) => "password_hash",
        DbError::InvalidEmail => "invalid_email",
        DbError::WeakPassword => "weak_password",
        DbError::EmailExists => "email_exists",
        DbError::UserDisabled => "user_disabled",
        DbError::AdminRequired => "admin_required",
        DbError::InvalidInviteCode => "invalid_invite_code",
        DbError::InviteGenerationDisabled => "invite_generation_disabled",
        DbError::InviteQuotaExceeded => "invite_quota_exceeded",
        DbError::InvalidAuthChallenge => "invalid_auth_challenge",
        DbError::DefaultPlanNotFound => "default_plan_not_found",
        DbError::RedeemCodeInvalid => "redeem_code_invalid",
        DbError::RedeemCodeNotFound => "redeem_code_not_found",
        DbError::RedeemCodeUsed => "redeem_code_used",
        DbError::RedeemCodeExpired => "redeem_code_expired",
        DbError::RedeemCountOutOfRange => "redeem_count_out_of_range",
    }
}

// 支付当前作为预留能力，默认关闭时所有写入式支付入口都必须短路。
