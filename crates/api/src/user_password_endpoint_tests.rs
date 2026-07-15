//! 已登录用户“邮箱验证码改密”接口测试。
//! 只覆盖 HTTP 层行为，邮箱验证码经 store 直接注入，绕过真实 SMTP。
//! 验证：无码/错码/过期码被拒、正确码改密成功、改后新密码可登录旧密码失败。
//! 验证：改密后该用户既有刷新令牌全部失效（旧会话需重登）、弱密码被拒。
//! 验证：发码接口限频（冷却期内二次发码 429）。
//! 缺少 DATABASE_URL 时跳过，避免无 PostgreSQL 环境误报。
//! 测试只用随机邮箱与 example.test 域名，不打印 token/密码等敏感信息。
//! 所有请求均使用 JSON，保持与前端调用一致。
//! 文件前十行中文注释满足仓库规则。
//! 改密语义以真实 PostgreSQL 行为为准，不用内存替身。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request};
use chrono::Duration;
use serde_json::{json, Value};
use tower::ServiceExt;

// 改密验证码与后端使用同一 scene，测试侧直接注入挑战，等价于“已收到邮件验证码”。
const CHANGE_PASSWORD_SCENE: &str = "change_password_email";

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
async fn test_pg_change_password_rejects_missing_and_wrong_code_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API change-password reject test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("api-chpwd-reject-{}@example.test", Uuid::new_v4().simple());
    store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = login_token(app.clone(), &email, "old-password-123")
        .await
        .unwrap();

    // 完全没有验证码字段 → 400。
    let no_code = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password",
        Some(&token),
        json!({"new_password": "new-strong-password-9"}),
    )
    .await;
    assert_eq!(no_code.0, StatusCode::BAD_REQUEST, "{}", no_code.1);

    // 错误验证码（挑战存在但码不对）→ 400，旧密码仍可登录。
    let challenge_id = store
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, "654321", 60)
        .await
        .unwrap();
    let wrong = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password",
        Some(&token),
        json!({
            "email_code_id": challenge_id,
            "email_code": "000000",
            "new_password": "new-strong-password-9"
        }),
    )
    .await;
    assert_eq!(wrong.0, StatusCode::BAD_REQUEST, "{}", wrong.1);
    assert!(store
        .authenticate_user(&email, "old-password-123")
        .await
        .unwrap()
        .is_some());

    // 过期验证码 → 400（ttl 注入 1 秒后等待过期）。
    let expiring = store
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, "123456", 1)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let expired = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password",
        Some(&token),
        json!({
            "email_code_id": expiring,
            "email_code": "123456",
            "new_password": "new-strong-password-9"
        }),
    )
    .await;
    assert_eq!(expired.0, StatusCode::BAD_REQUEST, "{}", expired.1);
    assert!(store
        .authenticate_user(&email, "old-password-123")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn test_pg_change_password_success_rotates_login_and_sessions_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API change-password success test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("api-chpwd-ok-{}@example.test", Uuid::new_v4().simple());
    let registered = store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let user_id = registered.user.id;
    // 模拟其它设备已有会话：预置一个刷新令牌，改密后必须失效。
    let other_session = store
        .create_refresh_token(user_id, Duration::days(7))
        .await
        .unwrap();

    let app = app(AppState::with_pg(store.clone()));
    let token = login_token(app.clone(), &email, "old-password-123")
        .await
        .unwrap();

    let challenge_id = store
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, "246810", 60)
        .await
        .unwrap();
    let changed = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password",
        Some(&token),
        json!({
            "email_code_id": challenge_id,
            "email_code": "246810",
            "new_password": "new-strong-password-9"
        }),
    )
    .await;
    assert_eq!(changed.0, StatusCode::OK, "{}", changed.1);
    assert_eq!(changed.1["success"], json!(true));

    // 新密码可登录、旧密码登录失败。
    assert!(login_token(app.clone(), &email, "new-strong-password-9")
        .await
        .is_some());
    assert!(login_token(app.clone(), &email, "old-password-123")
        .await
        .is_none());

    // 改密后既有刷新令牌失效，其它设备会话无法续期，必须重新登录。
    assert!(store
        .authenticate_refresh_token(&other_session)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_pg_change_password_rejects_weak_password_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API change-password weak test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("api-chpwd-weak-{}@example.test", Uuid::new_v4().simple());
    store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = login_token(app.clone(), &email, "old-password-123")
        .await
        .unwrap();

    let challenge_id = store
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, "246810", 60)
        .await
        .unwrap();
    let weak = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password",
        Some(&token),
        json!({
            "email_code_id": challenge_id,
            "email_code": "246810",
            "new_password": "短"
        }),
    )
    .await;
    assert_eq!(weak.0, StatusCode::BAD_REQUEST, "{}", weak.1);
    // 弱密码被拒后旧密码仍可用。
    assert!(login_token(app.clone(), &email, "old-password-123")
        .await
        .is_some());
}

#[tokio::test]
async fn test_pg_change_password_send_code_rate_limited_by_cooldown_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API change-password send-code cooldown test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    // 打开邮箱验证码并配置可用 SMTP 主机，使发码进入冷却逻辑分支。
    store
        .update_admin_auth_security_json(json!({
            "email_verification": {
                "enabled": true,
                "cooldown_seconds": 60,
                "smtp_host": "smtp.example.test",
                "smtp_port": 587,
                "smtp_username": "noreply@example.test",
                "smtp_from": "noreply@example.test",
                "smtp_password": "smtp-secret-placeholder"
            }
        }))
        .await
        .unwrap();

    let email = format!("api-chpwd-cool-{}@example.test", Uuid::new_v4().simple());
    store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = login_token(app.clone(), &email, "old-password-123")
        .await
        .unwrap();

    // 预置一个未消费的新发挑战，制造“冷却期内已发过”的状态。
    store
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, "246810", 60)
        .await
        .unwrap();
    let second = request_json(
        app.clone(),
        Method::POST,
        "/api/user/password/send-code",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(second.0, StatusCode::TOO_MANY_REQUESTS, "{}", second.1);
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
