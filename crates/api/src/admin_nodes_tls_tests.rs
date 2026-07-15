//! 管理端中转节点 TLS 续期接口测试。
//! 本文件从 admin_nodes_tests 拆分，只覆盖单节点证书续期。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 续期接口必须只排队当前节点已上报的证书域名。
//! 测试不输出 token、服务器地址、密码或代理凭据。
//! 缺少 DATABASE_URL 时跳过，避免本地无数据库误报。
//! 请求和响应只使用 example.test 测试域名。
//! 文件前十行中文注释满足仓库规则。
//! 断言只检查状态码和脱敏响应。
//! 不改变生产 API 路由或业务语义。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_access_node_tls_renew_endpoint_queues_single_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API access node TLS renewal test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = Uuid::parse_str("00000000-0000-0000-0000-000000000201").unwrap();
    store
        .record_agent_tls_status(
            node_id,
            vec![xrayc_db::AgentTlsCertificateReport {
                domain: "tls-renew-api.example.test".to_string(),
                status: "valid".to_string(),
                not_before: Some("2026-06-01T00:00:00Z".to_string()),
                not_after: Some("2026-08-30T00:00:00Z".to_string()),
                days_remaining: Some(80),
                error_summary: String::new(),
            }],
            None,
        )
        .await
        .unwrap();
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

    let queued = request_json(
        app,
        Method::POST,
        &format!("/api/admin/access-nodes/{node_id}/tls/renew"),
        Some(admin_token),
        Value::Null,
    )
    .await;

    assert_eq!(queued.0, StatusCode::OK, "{}", queued.1);
    assert_eq!(queued.1["data"]["status"], "queued");
    assert_eq!(queued.1["data"]["domains"][0], "tls-renew-api.example.test");
    assert!(queued.1["data"]["request_id"].as_str().is_some());
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
