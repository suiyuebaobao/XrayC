//! 管理端入口管理接口测试。
//! 本文件覆盖入口创建、入口出口绑定和分组绑定节点路由。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 缺少 DATABASE_URL 时跳过，避免无数据库环境误报。
//! 请求体只使用 RFC 示例 IP 和 example.test 域名。
//! 断言只检查状态码、ID 和非敏感绑定关系。
//! 真实服务器、代理凭据和订阅 token 不写入测试输出。
//! 前端入口管理页面依赖这些路由完成主路径闭环。
//! 文件前十行中文注释满足仓库规则。
//! 维护时不要在此加入私有测试资产。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_access_entry_binding_node_api_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API access entry binding node test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
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

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("api-entry-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.11".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("api-entry-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created_exit = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "api-entry-resource".to_string(),
                    endpoint_name: "api-entry-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.11".to_string(),
                    port: 38_191,
                    outbound_config: json!({"username": "api-user", "password": "api-pass"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = created_exit["created_lines"][0]["exit_endpoint_id"]
        .as_str()
        .unwrap();
    let group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("api-entry-group-{}", Uuid::new_v4().simple()),
            country_code: "US".to_string(),
            icon: String::new(),
            group_level: None,
            parent_group_id: None,
            sort_weight: Some(100),
            billing_multiplier: None,
            enabled: Some(true),
            dedicated_rules: None,
            rule_set_bindings: None,
        })
        .await
        .unwrap();

    let created_entry = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-entries",
        Some(admin_token),
        json!({
            "access_node_id": node_id,
            "name": "api-entry-vless-ws",
            "listen_host": "",
            // CF 入口端口必须落在 CF_SUPPORTED_HTTPS_PORTS 白名单内(否则护栏返回 422)。
            "listen_port": 8443,
            "protocol": "vless",
            "transport": "ws",
            "security": "tls",
            "server_name": "cdn-entry.example.test",
            "ws_path": "/vless-ws",
            "ws_host": "cdn-entry.example.test",
            "cdn_enabled": true,
            "cdn_provider": "cloudflare",
            "cdn_hostname": "cdn-entry.example.test",
            "cdn_server": "cdn-entry.example.test",
            "enabled": true,
            "sort_weight": 100
        }),
    )
    .await;
    assert_eq!(created_entry.0, StatusCode::CREATED, "{}", created_entry.1);
    let entry_id = created_entry.1["data"]["id"].as_str().unwrap();

    let listed_entries = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/access-entries",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(listed_entries.0, StatusCode::OK, "{}", listed_entries.1);
    assert!(listed_entries.1["data"]["access_entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == entry_id && item["cdn_provider"] == "cloudflare"));

    let created_binding = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/access-entries/{entry_id}/exit-bindings"),
        Some(admin_token),
        json!({
            "exit_endpoint_id": exit_endpoint_id,
            "name": "api-entry-binding-node",
            "enabled": true,
            "sort_weight": 100,
            "remark": "api test"
        }),
    )
    .await;
    assert_eq!(
        created_binding.0,
        StatusCode::CREATED,
        "{}",
        created_binding.1
    );
    let binding_id = created_binding.1["data"]["id"].as_str().unwrap();

    let listed_bindings = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/access-entry-exit-bindings",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(listed_bindings.0, StatusCode::OK, "{}", listed_bindings.1);
    assert!(listed_bindings.1["data"]["access_entry_exit_bindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == binding_id && item["access_entry_id"] == entry_id));

    let replaced = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/line-groups/{group_id}/binding-nodes"),
        Some(admin_token),
        json!({"binding_node_ids": [binding_id]}),
    )
    .await;
    assert_eq!(replaced.0, StatusCode::OK, "{}", replaced.1);
    assert_eq!(replaced.1["data"]["count"], 1);

    let updated_entry = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-entries/{entry_id}"),
        Some(admin_token),
        json!({
            "access_node_id": node_id,
            "name": "api-entry-vless-ws-updated",
            "listen_host": "",
            "listen_port": 2053,
            "protocol": "vless",
            "transport": "ws",
            "security": "tls",
            "server_name": "cdn-entry.example.test",
            "ws_path": "/vless-ws",
            "ws_host": "cdn-entry.example.test",
            "cdn_enabled": true,
            "cdn_provider": "cloudflare",
            "cdn_hostname": "cdn-entry.example.test",
            "cdn_server": "cdn-entry.example.test",
            "enabled": true,
            "sort_weight": 120
        }),
    )
    .await;
    assert_eq!(updated_entry.0, StatusCode::OK, "{}", updated_entry.1);

    let updated_binding = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-entry-exit-bindings/{binding_id}"),
        Some(admin_token),
        json!({
            "exit_endpoint_id": exit_endpoint_id,
            "name": "api-entry-binding-node-updated",
            "enabled": false,
            "sort_weight": 120,
            "remark": "updated"
        }),
    )
    .await;
    assert_eq!(updated_binding.0, StatusCode::OK, "{}", updated_binding.1);

    let deleted_binding = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/access-entry-exit-bindings/{binding_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(deleted_binding.0, StatusCode::OK, "{}", deleted_binding.1);

    let deleted_entry = request_json(
        app,
        Method::DELETE,
        &format!("/api/admin/access-entries/{entry_id}"),
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(deleted_entry.0, StatusCode::OK, "{}", deleted_entry.1);
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
