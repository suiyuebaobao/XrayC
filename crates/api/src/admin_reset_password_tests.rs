//! 管理员重置用户密码接口测试。
//! 只覆盖 HTTP 层行为：admin 直接设新密码，无需邮箱验证码（admin 本身即授权）。
//! 验证：非 admin/未登录被拒(403/401)、admin 重置后新密码可登录旧密码失败旧令牌失效。
//! 验证：弱密码被拒(400)、用户不存在返回 404，且响应/审计绝不出现明文密码。
//! 缺少 DATABASE_URL 时跳过，避免无 PostgreSQL 环境误报。
//! 测试只用随机邮箱与 example.test 域名，不打印 token/密码等敏感信息。
//! 所有请求均使用 JSON，保持与前端调用一致。
//! 复用 set_user_password_and_revoke_sessions 的强度校验与会话失效语义。
//! 文件前十行中文注释满足仓库规则。
//! 重置语义以真实 PostgreSQL 行为为准，不用内存替身。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request};
use chrono::Duration;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn login_token(app: Router, account: &str, password: &str) -> Option<String> {
    let login = request_json(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": account, "password": password}),
    )
    .await;
    if login.0 != StatusCode::OK {
        return None;
    }
    login.1["data"]["access_token"]
        .as_str()
        .map(|value| value.to_string())
}

#[tokio::test]
async fn test_pg_admin_reset_password_rejects_unauthorized_and_non_admin_when_database_url_is_set()
{
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin reset-password auth test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("admin-reset-auth-{}@example.test", Uuid::new_v4().simple());
    let registered = store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let user_id = registered.user.id;
    let app = app(AppState::with_pg(store));

    // 未登录（无 Bearer）→ 401。
    let anon = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/users/{user_id}/reset-password"),
        None,
        json!({"new_password": "new-strong-password-9"}),
    )
    .await;
    assert_eq!(anon.0, StatusCode::UNAUTHORIZED, "{}", anon.1);

    // 普通用户 token → 403（管理写入需管理员）。
    let user_token = login_token(app.clone(), &email, "old-password-123")
        .await
        .unwrap();
    let forbidden = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/users/{user_id}/reset-password"),
        Some(&user_token),
        json!({"new_password": "new-strong-password-9"}),
    )
    .await;
    assert_eq!(forbidden.0, StatusCode::FORBIDDEN, "{}", forbidden.1);
}

#[tokio::test]
async fn test_pg_admin_reset_password_rotates_login_and_sessions_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin reset-password success test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("admin-reset-ok-{}@example.test", Uuid::new_v4().simple());
    let registered = store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let user_id = registered.user.id;
    // 模拟目标用户已有会话：预置刷新令牌，重置后必须失效。
    let other_session = store
        .create_refresh_token(user_id, Duration::days(7))
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));

    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();
    let reset = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/users/{user_id}/reset-password"),
        Some(&admin_token),
        json!({"new_password": "new-strong-password-9"}),
    )
    .await;
    assert_eq!(reset.0, StatusCode::OK, "{}", reset.1);
    assert_eq!(reset.1["success"], json!(true));
    // 响应体绝不回显任何密码明文。
    assert!(!reset.1.to_string().contains("new-strong-password-9"));

    // 新密码可登录、旧密码登录失败。
    assert!(login_token(app.clone(), &email, "new-strong-password-9")
        .await
        .is_some());
    assert!(login_token(app.clone(), &email, "old-password-123")
        .await
        .is_none());

    // 重置后目标用户既有刷新令牌失效，旧会话需重新登录。
    assert!(store
        .authenticate_refresh_token(&other_session)
        .await
        .unwrap()
        .is_none());

    // 审计日志已记录该次重置，且摘要不含明文密码。
    let logs = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/audit-logs?page=1&page_size=20",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(logs.0, StatusCode::OK, "{}", logs.1);
    let logs_text = logs.1.to_string();
    assert!(logs_text.contains("user.reset_password"));
    assert!(!logs_text.contains("new-strong-password-9"));
}

#[tokio::test]
async fn test_pg_admin_reset_password_rejects_weak_password_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin reset-password weak test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("admin-reset-weak-{}@example.test", Uuid::new_v4().simple());
    store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let user_id = store
        .authenticate_user(&email, "old-password-123")
        .await
        .unwrap()
        .unwrap()
        .id;
    let app = app(AppState::with_pg(store.clone()));

    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();
    let weak = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/users/{user_id}/reset-password"),
        Some(&admin_token),
        json!({"new_password": "短"}),
    )
    .await;
    assert_eq!(weak.0, StatusCode::BAD_REQUEST, "{}", weak.1);
    // 弱密码被拒后旧密码仍可用。
    assert!(login_token(app.clone(), &email, "old-password-123")
        .await
        .is_some());
}

#[tokio::test]
async fn test_pg_admin_reset_password_user_not_found_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin reset-password not-found test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store));

    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();
    let missing_id = Uuid::new_v4();
    let not_found = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/users/{missing_id}/reset-password"),
        Some(&admin_token),
        json!({"new_password": "new-strong-password-9"}),
    )
    .await;
    assert_eq!(not_found.0, StatusCode::NOT_FOUND, "{}", not_found.1);
}

// 与其它接口测试一致的 JSON 请求 helper：可选 Bearer，空 body 用 Body::empty。
async fn request_json(
    app: Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .oneshot(
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (status, value)
}
