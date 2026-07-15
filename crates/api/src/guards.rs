//! 本模块集中存放认证、登录防护和签名校验辅助函数。
//! 这里只处理 JWT、cookie、用户/admin/agent 授权和支付回调签名。
//! 不放业务 handler 主流程，避免拆分时改变路由和响应语义。
//! 所有响应文案、状态码和 token/cookie 处理沿用原实现。

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;
use xrayc_db::{DbError, PgStore};

use crate::responses::{forbidden, internal_error, unauthorized_payment};
use crate::state::AppState;

pub(crate) type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AccessClaims {
    pub(crate) sub: String,
    pub(crate) email: String,
    pub(crate) role: String,
    pub(crate) is_admin: bool,
    pub(crate) iat: usize,
    pub(crate) exp: usize,
}

impl AccessClaims {
    pub(crate) fn user_id(&self) -> Uuid {
        Uuid::parse_str(&self.sub).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoginGuardConfig {
    pub(crate) enabled: bool,
    pub(crate) failure_threshold: i32,
    pub(crate) lock_minutes: i32,
}

pub(crate) fn login_guard_config(value: &serde_json::Value) -> LoginGuardConfig {
    let guard = value
        .get("login_guard")
        .and_then(serde_json::Value::as_object);
    LoginGuardConfig {
        enabled: guard
            .and_then(|guard| guard.get("enabled"))
            .and_then(serde_json::Value::as_bool)
            .or_else(|| {
                value
                    .get("login_lock_enabled")
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or(true),
        failure_threshold: guard
            .and_then(|guard| guard.get("failure_threshold"))
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                value
                    .get("login_failure_threshold")
                    .and_then(serde_json::Value::as_i64)
            })
            .unwrap_or(5)
            .clamp(1, 100) as i32,
        lock_minutes: guard
            .and_then(|guard| guard.get("lock_minutes"))
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                value
                    .get("login_lock_minutes")
                    .and_then(serde_json::Value::as_i64)
            })
            .unwrap_or(5)
            .clamp(1, 1440) as i32,
    }
}

pub(crate) fn invite_required(value: &serde_json::Value) -> bool {
    value
        .get("require_invite_code")
        .or_else(|| value.get("invite_required"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn email_domain_allowed(value: &serde_json::Value, email: &str) -> bool {
    let Some(domain) = email
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim().to_ascii_lowercase())
    else {
        return true;
    };
    let allowed = value
        .get("email_verification")
        .and_then(|email| email.get("allowed_domains"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|value| value.trim().trim_start_matches('@').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    allowed.is_empty() || allowed.iter().any(|allowed| allowed == &domain)
}

pub(crate) async fn verify_required_challenge(
    pg: &PgStore,
    scene: &str,
    target: &str,
    challenge_id: Option<Uuid>,
    code: Option<&str>,
) -> bool {
    let (Some(challenge_id), Some(code)) = (challenge_id, code) else {
        return false;
    };
    if code.trim().is_empty() {
        return false;
    }
    pg.verify_auth_challenge(scene, target, challenge_id, code)
        .await
        .unwrap_or(false)
}

pub(crate) fn captcha_enabled_for_scene(value: &serde_json::Value, scene: &str) -> bool {
    let captcha = value.get("captcha").and_then(serde_json::Value::as_object);
    match scene {
        "register" => captcha
            .and_then(|captcha| captcha.get("register_enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "login" => captcha
            .and_then(|captcha| captcha.get("user_login_enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "admin_login" => captcha
            .and_then(|captcha| captcha.get("admin_login_enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn captcha_ttl_seconds(value: &serde_json::Value) -> i64 {
    value
        .get("captcha")
        .and_then(|captcha| captcha.get("ttl_seconds"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(60)
        .clamp(1, 600)
}

pub(crate) fn email_verification_enabled(value: &serde_json::Value) -> bool {
    value
        .get("email_verification")
        .and_then(|email| email.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn email_code_cooldown_seconds(value: &serde_json::Value) -> i64 {
    value
        .get("email_verification")
        .and_then(|email| email.get("cooldown_seconds"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(60)
        .clamp(1, 3600)
}

pub(crate) fn arithmetic_challenge() -> (String, String) {
    let seed = Uuid::new_v4().as_u128();
    let left = (seed % 19 + 1) as u32;
    let right = ((seed / 19) % 19 + 1) as u32;
    (format!("{left} + {right} = ?"), (left + right).to_string())
}

pub(crate) fn six_digit_code() -> String {
    format!("{:06}", (Uuid::new_v4().as_u128() % 1_000_000) as u32)
}

pub(crate) async fn send_email_code_via_smtp(
    security: &serde_json::Value,
    to_email: &str,
    code: &str,
) -> Result<(), String> {
    // 缺 email_verification 整块时沿用旧文案:配置未初始化。
    let email = security
        .get("email_verification")
        .ok_or_else(|| "SMTP 配置未初始化".to_string())?;
    // SMTP 连接/发件人解析下沉到共享 crate xrayc-backup(字段口径与旧实现逐字一致);
    // host/username/password 任一缺失即视为配置不完整,沿用旧文案。
    let smtp = xrayc_backup::smtp_config_from_email_verification(email)
        .ok_or_else(|| "SMTP 配置不完整，无法发送邮箱验证码".to_string())?;

    // 验证码邮件主题/正文与旧实现逐字一致,仍为纯文本(无附件,故末参传 None)。
    let body = format!("您的注册验证码是 {code}，1 分钟内有效。若非本人操作，请忽略本邮件。");
    xrayc_backup::send_email(
        &smtp,
        &[to_email.to_string()],
        "XrayC 注册邮箱验证码",
        &body,
        None,
    )
    .await
    // 发送失败错误摘要已在共享 crate 内脱敏(不含明文密码),这里沿用旧文案前缀。
    .map_err(|error| format!("邮箱验证码发送失败: {error}"))?;
    Ok(())
}

pub(crate) fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    // 当前处理器暴露的是原始浏览器凭据头，只解析需要的键，避免为了
    // 一个简单场景增加额外凭据依赖。
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, cookie_value) = cookie.trim().split_once('=')?;
                (cookie_name == name).then_some(cookie_value)
            })
        })
}

pub(crate) fn issue_access_token(
    user: &xrayc_db::AuthenticatedUser,
    jwt_secret: &str,
    ttl: Duration,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let role = if user.is_admin { "admin" } else { "user" };
    let claims = AccessClaims {
        sub: user.id.to_string(),
        email: user.email.clone(),
        role: role.to_string(),
        is_admin: user.is_admin,
        iat: now.timestamp() as usize,
        exp: (now + ttl).timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
}

pub(crate) fn decode_access_token(headers: &HeaderMap, jwt_secret: &str) -> Option<AccessClaims> {
    let token = bearer_token(headers)?;
    decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims)
}

pub(crate) fn require_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AccessClaims, Box<Response>> {
    decode_access_token(headers, &state.jwt_secret).ok_or_else(|| {
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"success": false, "message": "未登录或登录已过期"})),
            )
                .into_response(),
        )
    })
}

pub(crate) fn require_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AccessClaims, Box<Response>> {
    let claims = require_user(state, headers)?;
    if claims.is_admin {
        Ok(claims)
    } else {
        Err(Box::new(forbidden()))
    }
}

fn access_claims_error_response(error: DbError, require_admin: bool) -> Box<Response> {
    match error {
        DbError::UserNotFound | DbError::UserDisabled => Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"success": false, "message": "未登录或登录已过期"})),
            )
                .into_response(),
        ),
        DbError::AdminRequired if require_admin => Box::new(forbidden()),
        other => Box::new(internal_error(other).into_response()),
    }
}

pub(crate) async fn require_user_pg<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<(&'a PgStore, AccessClaims), Box<Response>> {
    let claims = require_user(state, headers)?;
    let pg = state.pg.as_ref().ok_or_else(|| {
        Box::new(
            (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({
                    "success": false,
                    "message": "用户业务接口需要 PostgreSQL 模式"
                })),
            )
                .into_response(),
        )
    })?;
    pg.authorize_access_claims(claims.user_id(), false)
        .await
        .map_err(|error| access_claims_error_response(error, false))?;
    Ok((pg, claims))
}

pub(crate) async fn require_admin_pg_with_claims<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<(&'a PgStore, AccessClaims), Box<Response>> {
    let claims = require_admin(state, headers)?;
    let pg = state.pg.as_ref().ok_or_else(|| {
        Box::new(
            (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({
                    "success": false,
                    "message": "管理写入接口需要 PostgreSQL 模式"
                })),
            )
                .into_response(),
        )
    })?;
    pg.authorize_access_claims(claims.user_id(), true)
        .await
        .map_err(|error| access_claims_error_response(error, true))?;
    Ok((pg, claims))
}

pub(crate) async fn require_admin_pg<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<&'a PgStore, Box<Response>> {
    let claims = require_admin(state, headers)?;
    let pg = state.pg.as_ref().ok_or_else(|| {
        Box::new(
            (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({
                    "success": false,
                    "message": "管理写入接口需要 PostgreSQL 模式"
                })),
            )
                .into_response(),
        )
    })?;
    pg.authorize_access_claims(claims.user_id(), true)
        .await
        .map_err(|error| access_claims_error_response(error, true))?;
    Ok(pg)
}

pub(crate) async fn require_agent_token(
    pg: &PgStore,
    reported_node_id: Option<Uuid>,
    headers: &HeaderMap,
) -> Result<bool, DbError> {
    let Some(token) = bearer_token(headers) else {
        return Ok(false);
    };
    pg.verify_agent_token(reported_node_id, token).await
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

pub(crate) fn verify_payment_callback_signature(
    headers: &HeaderMap,
    body: &[u8],
) -> Option<Response> {
    let secret = std::env::var("PAYMENT_CALLBACK_SECRET").unwrap_or_default();
    if secret.is_empty() {
        return Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "success": false,
                    "message": "支付回调密钥未配置"
                })),
            )
                .into_response(),
        );
    }
    let Some(signature) = headers
        .get("x-xrayc-signature")
        .or_else(|| headers.get("x-signature"))
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().strip_prefix("sha256=").unwrap_or(value.trim()))
    else {
        return Some(unauthorized_payment().into_response());
    };
    let Ok(signature_bytes) = hex::decode(signature) else {
        return Some(unauthorized_payment().into_response());
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return Some(internal_error("支付回调密钥无效").into_response());
    };
    mac.update(body);
    if mac.verify_slice(&signature_bytes).is_err() {
        return Some(unauthorized_payment().into_response());
    }
    None
}
