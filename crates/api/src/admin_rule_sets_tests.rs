//! 管理端订阅规则库接口测试。
//! 本文件覆盖规则库 CRUD HTTP 路由和管理员鉴权链路。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 缺少 DATABASE_URL 时跳过，避免本地无数据库误报。
//! 规则库只保存订阅规则，不保存服务器、代理账号或出口凭据。
//! 断言只检查状态码和非敏感响应字段。
//! 删除绑定保护由数据库分片覆盖，这里只验证路由可用。
//! 新增规则库接口应优先在本文件补充回归用例。
//! 文件前十行中文注释满足仓库规则。
//! 所有请求体均使用 example.test 域名。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_subscription_rule_set_crud_api_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API subscription rule set CRUD test");
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

    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/subscription-rule-sets",
        Some(admin_token),
        json!({
            "name": format!("api-rule-set-{}", Uuid::new_v4().simple()),
            "description": "api test",
            "enabled": true,
            "rules": ["DOMAIN-SUFFIX,example.test,PROXY"]
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    let rule_set_id = created.1["data"]["id"].as_str().unwrap();

    let listed = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/subscription-rule-sets",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(listed.0, StatusCode::OK, "{}", listed.1);
    assert!(listed.1["data"]["rule_sets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == rule_set_id));

    let updated = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/subscription-rule-sets/{rule_set_id}"),
        Some(admin_token),
        json!({
            "name": "api-rule-set-updated",
            "description": "updated",
            "enabled": false,
            "rules": ["DOMAIN-SUFFIX,updated.example.test,DIRECT"]
        }),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);

    let deleted = request_json(
        app,
        Method::DELETE,
        &format!("/api/admin/subscription-rule-sets/{rule_set_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(deleted.0, StatusCode::OK, "{}", deleted.1);
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
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    (status, value)
}
