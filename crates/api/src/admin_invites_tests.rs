//! 管理端邀请码接口测试。
//! 本文件只覆盖 HTTP 层的管理员生成、列表和删除流程。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 邀请码只用于测试断言，不输出访问令牌或服务器敏感信息。
//! 管理员生成不受普通用户自助邀请码开关限制。
//! 删除接口只验证未使用邀请码，避免破坏邀请追责记录。
//! 缺少 DATABASE_URL 时自动跳过，方便无数据库环境构建。
//! 所有请求保持 JSON 格式，与前端管理页面一致。
//! 文件前十行中文注释满足仓库规则。
//! 新增断言必须继续避免泄露明文凭据。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_admin_invite_codes_crud_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API admin invite code test");
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

    let before = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/invite-codes",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(before.0, StatusCode::OK, "{}", before.1);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/invite-codes",
        Some(admin_token),
        json!({"count": 2}),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    let items = created.1["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item["is_used"] == false));
    let code = items[0]["code"].as_str().unwrap().to_string();

    let after_create = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/invite-codes",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(after_create.0, StatusCode::OK, "{}", after_create.1);
    assert!(after_create.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == code && item["inviter_email"].is_string()));

    let deleted = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/invite-codes/{code}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(deleted.0, StatusCode::OK, "{}", deleted.1);
    assert_eq!(deleted.1["data"]["deleted"], true);

    let after_delete = request_json(
        app,
        Method::GET,
        "/api/admin/invite-codes",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(after_delete.0, StatusCode::OK, "{}", after_delete.1);
    assert!(!after_delete.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == code));
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
