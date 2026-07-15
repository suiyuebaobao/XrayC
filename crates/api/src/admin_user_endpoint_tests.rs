//! 管理端用户 CRUD 接口测试。
//! 本文件只覆盖 HTTP 层行为，不绕过路由直接调用数据库方法。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 断言只检查用户字段、状态码和登录结果，不打印 token。
//! 新增用户使用随机邮箱，测试结束通过删除接口清理。
//! 删除用户后再验证账号不可登录，避免只测 UI 假删除。
//! 缺少 DATABASE_URL 时跳过，避免无数据库环境误报。
//! 所有测试请求均使用 JSON，保持与前端调用一致。
//! 文件前十行中文注释满足仓库规则。
//! 不在测试日志中输出服务器、密码或其它敏感信息。

use super::*;
use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Method, Request};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_admin_user_devices_route_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin user devices test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    store
        .record_subscription_pull_event_for_user(
            user_id,
            "203.0.113.8",
            "sha256:ddbea5471056690e5b1dcfe0c39ffca2f91ee81710048d066e2d53c7d012e14e",
            "mihomo-test",
        )
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let admin_login = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "admin", "password": xrayc_db::DEMO_ADMIN_PASSWORD}),
    )
    .await;
    assert_eq!(admin_login.0, StatusCode::OK, "{}", admin_login.1);
    let admin_token = admin_login.1["data"]["access_token"].as_str().unwrap();

    let devices = request_json(
        app,
        Method::GET,
        &format!("/api/admin/users/{user_id}/devices?page=1&page_size=20"),
        Some(admin_token),
        Value::Null,
    )
    .await;

    assert_eq!(devices.0, StatusCode::OK, "{}", devices.1);
    assert_eq!(devices.1["data"]["total"], 1);
    assert_eq!(devices.1["data"]["items"][0]["client_ip"], "203.0.113.8");
    assert_eq!(devices.1["data"]["items"][0]["source"], "subscription_pull");

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pg_subscription_download_records_pull_ip_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API subscription pull event test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/sub/demo-token")
                .header("x-real-ip", "203.0.113.9")
                .header(header::USER_AGENT, "mihomo-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscription_pull_events WHERE user_id = $1 AND client_ip = '203.0.113.9'",
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_pg_subscription_download_ignores_forwarded_ip_from_untrusted_peer() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API subscription trusted peer test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let peer: SocketAddr = "198.51.100.7:51234".parse().unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/sub/demo-token")
                .header("x-real-ip", "203.0.113.200")
                .header(header::USER_AGENT, "mihomo-test")
                .extension(ConnectInfo(peer))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let spoofed_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscription_pull_events WHERE user_id = $1 AND client_ip = '203.0.113.200'",
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let peer_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscription_pull_events WHERE user_id = $1 AND client_ip = '198.51.100.7'",
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(spoofed_count, 0);
    assert_eq!(peer_count, 1);
}

#[tokio::test]
async fn test_pg_admin_users_crud_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin user CRUD test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store));

    let admin_login = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "admin", "password": xrayc_db::DEMO_ADMIN_PASSWORD}),
    )
    .await;
    assert_eq!(admin_login.0, StatusCode::OK, "{}", admin_login.1);
    let admin_token = admin_login.1["data"]["access_token"].as_str().unwrap();
    let admin_id = admin_login.1["data"]["user"]["id"].as_str().unwrap();
    let email = format!("admin-created-{}@example.test", Uuid::new_v4().simple());
    let batch_email = format!("admin-batch-{}@example.test", Uuid::new_v4().simple());
    let single_email = format!("admin-single-{}@example.test", Uuid::new_v4().simple());
    let partial_email = format!("admin-partial-{}@example.test", Uuid::new_v4().simple());
    let password = "created123";

    let delete_current_admin = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/users/{admin_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(delete_current_admin.0, StatusCode::BAD_REQUEST);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users",
        Some(admin_token),
        json!({
            "email": email,
            "password": password,
            "is_admin": false,
            "disabled": false
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::OK, "{}", created.1);
    let user_id = created.1["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(created.1["data"]["email"], email);
    assert_eq!(created.1["data"]["status"], "active");

    let listed = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users?keyword={email}&page=1&page_size=20"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(listed.0, StatusCode::OK, "{}", listed.1);
    assert_eq!(listed.1["data"]["total"], 1);
    assert!(listed.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|user| user["id"] == user_id && user["email"] == email));
    assert!(!listed.1.to_string().contains("password"));

    let detail = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users/{user_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(detail.0, StatusCode::OK, "{}", detail.1);
    assert_eq!(detail.1["data"]["email"], email);

    let user_login = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": email, "password": password}),
    )
    .await;
    assert_eq!(user_login.0, StatusCode::OK, "{}", user_login.1);

    let updated = request_json(
        app.clone(),
        Method::PATCH,
        &format!("/api/admin/users/{user_id}"),
        Some(admin_token),
        json!({"disabled": true}),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);
    assert_eq!(updated.1["data"]["status"], "disabled");

    let disabled_list = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users?status=disabled&role=user&keyword={email}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(disabled_list.0, StatusCode::OK, "{}", disabled_list.1);
    assert_eq!(disabled_list.1["data"]["total"], 1);

    let single_created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users",
        Some(admin_token),
        json!({
            "email": single_email,
            "password": password,
            "is_admin": false,
            "disabled": false
        }),
    )
    .await;
    assert_eq!(single_created.0, StatusCode::OK, "{}", single_created.1);
    let single_user_id = single_created.1["data"]["id"].as_str().unwrap().to_string();

    let single_deleted = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/users/{single_user_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(single_deleted.0, StatusCode::OK, "{}", single_deleted.1);
    assert_eq!(single_deleted.1["data"]["deleted"], true);

    let single_login_after_delete = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": single_email, "password": password}),
    )
    .await;
    assert_eq!(single_login_after_delete.0, StatusCode::UNAUTHORIZED);

    let batch_created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users",
        Some(admin_token),
        json!({
            "email": batch_email,
            "password": password,
            "is_admin": false,
            "disabled": false
        }),
    )
    .await;
    assert_eq!(batch_created.0, StatusCode::OK, "{}", batch_created.1);
    let batch_user_id = batch_created.1["data"]["id"].as_str().unwrap().to_string();

    let partial_created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users",
        Some(admin_token),
        json!({
            "email": partial_email,
            "password": password,
            "is_admin": false,
            "disabled": false
        }),
    )
    .await;
    assert_eq!(partial_created.0, StatusCode::OK, "{}", partial_created.1);
    let partial_user_id = partial_created.1["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let failed_batch = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users/batch-delete",
        Some(admin_token),
        json!({"user_ids": [partial_user_id, Uuid::new_v4().to_string()]}),
    )
    .await;
    assert_eq!(failed_batch.0, StatusCode::NOT_FOUND, "{}", failed_batch.1);

    let partial_after_failed_batch = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users?keyword={partial_email}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(
        partial_after_failed_batch.0,
        StatusCode::OK,
        "{}",
        partial_after_failed_batch.1
    );
    assert_eq!(partial_after_failed_batch.1["data"]["total"], 1);

    let deleted = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/users/batch-delete",
        Some(admin_token),
        json!({"user_ids": [user_id, batch_user_id, partial_user_id]}),
    )
    .await;
    assert_eq!(deleted.0, StatusCode::OK, "{}", deleted.1);
    assert_eq!(deleted.1["data"]["deleted_count"], 3);
    assert_eq!(deleted.1["data"]["items"].as_array().unwrap().len(), 3);

    let after_deleted = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users?keyword={email}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(after_deleted.0, StatusCode::OK, "{}", after_deleted.1);
    assert_eq!(after_deleted.1["data"]["total"], 0);

    let after_batch_deleted = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/users?keyword={batch_email}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(
        after_batch_deleted.0,
        StatusCode::OK,
        "{}",
        after_batch_deleted.1
    );
    assert_eq!(after_batch_deleted.1["data"]["total"], 0);

    let deleted_login = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": email, "password": password}),
    )
    .await;
    assert_eq!(deleted_login.0, StatusCode::UNAUTHORIZED);
}

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
