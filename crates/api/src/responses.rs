//! 本模块集中存放接口响应构造辅助函数。
//! 这里只包含无状态的小函数和响应头字符串拼装。
//! 不在这里放数据库访问、路由注册或业务处理器。
//! 保持字段、状态码、浏览器凭据属性与原实现一致。

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::AuthResponsePolicy;

pub(crate) const REFRESH_COOKIE_NAME: &str = "refresh_token";

pub(crate) fn is_safe_artifact_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

pub(crate) fn content_disposition_filename(profile_name: &str) -> String {
    let fallback = ascii_filename_fallback(profile_name);
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        fallback,
        percent_encode_header_value(profile_name)
    )
}

fn ascii_filename_fallback(profile_name: &str) -> String {
    let fallback = profile_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else if ch.is_ascii_whitespace() {
                '-'
            } else {
                '\0'
            }
        })
        .filter(|ch| *ch != '\0')
        .collect::<String>();
    if fallback.is_empty() {
        "XrayC".to_string()
    } else {
        fallback
    }
}

pub(crate) fn percent_encode_header_value(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .flat_map(|byte| match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![*byte as char]
            }
            byte => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

pub(crate) fn internal_error(
    error: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"success": false, "message": error.to_string()})),
    )
}

pub(crate) fn invalid_login() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"success": false, "message": "账号或密码错误"})),
    )
}

pub(crate) fn created_id_response(id: Uuid) -> Response {
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "success": true,
            "data": {"id": id}
        })),
    )
        .into_response()
}

pub(crate) fn validation_error(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({"success": false, "message": message})),
    )
        .into_response()
}

pub(crate) fn unauthorized_user() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"success": false, "message": "未登录或登录已过期"})),
    )
}

pub(crate) fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"success": false, "message": message})),
    )
}

pub(crate) fn not_found(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"success": false, "message": message})),
    )
}

pub(crate) fn conflict(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"success": false, "message": message})),
    )
}

pub(crate) fn unprocessable(
    error: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({"success": false, "message": error.to_string()})),
    )
}

pub(crate) fn login_locked(
    locked_until: chrono::DateTime<Utc>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "success": false,
            "message": "登录失败次数过多，请稍后再试",
            "locked_until": locked_until
        })),
    )
}

pub(crate) fn too_many_requests(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({"success": false, "message": message})),
    )
}

pub(crate) fn auth_success_response(
    user: &xrayc_db::AuthenticatedUser,
    access_token: String,
    refresh_token: Option<String>,
    subscription_token: Option<String>,
    policy: AuthResponsePolicy,
    message: &str,
) -> Response {
    let mut headers = HeaderMap::new();
    if let Some(refresh_token) = refresh_token {
        // 刷新令牌只允许通过只读浏览器凭据下发，前端不能读取明文。
        match HeaderValue::from_str(&refresh_cookie_header(
            &refresh_token,
            policy.refresh_token_ttl,
            policy.refresh_cookie_secure,
        )) {
            Ok(value) => {
                headers.insert(header::SET_COOKIE, value);
            }
            Err(error) => return internal_error(error).into_response(),
        }
    }

    (
        headers,
        Json(serde_json::json!({
            "success": true,
            "message": message,
            "data": {
                "access_token": access_token,
                "token_type": "Bearer",
                "expires_in": policy.access_token_ttl.num_seconds(),
                "subscription_token": subscription_token,
                "user": {
                    "id": user.id,
                    "account": user.email,
                    "name": if user.display_name.is_empty() {
                        user.email.clone()
                    } else {
                        user.display_name.clone()
                    },
                    "role": if user.is_admin { "admin" } else { "user" },
                    "is_admin": user.is_admin
                }
            }
        })),
    )
        .into_response()
}

pub(crate) fn refresh_cookie_header(token: &str, ttl: Duration, secure: bool) -> String {
    let secure_suffix = if secure { "; Secure" } else { "" };
    format!(
        "{REFRESH_COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}{}",
        ttl.num_seconds(),
        secure_suffix
    )
}

pub(crate) fn clear_refresh_cookie_header(secure: bool) -> String {
    let secure_suffix = if secure { "; Secure" } else { "" };
    format!("{REFRESH_COOKIE_NAME}=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0{secure_suffix}")
}

pub(crate) fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"success": false, "message": "权限不足"})),
    )
        .into_response()
}

pub(crate) fn forbidden_with_message(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"success": false, "message": message})),
    )
        .into_response()
}

pub(crate) fn payment_paused() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "success": false,
            "code": "payment_disabled",
            "message": "支付功能已暂停，接口保留待后续启用"
        })),
    )
}

pub(crate) fn unauthorized_payment() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"success": false, "message": "支付回调签名无效"})),
    )
}

pub(crate) fn unauthorized() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"success": false, "message": "agent token 无效"})),
    )
}
