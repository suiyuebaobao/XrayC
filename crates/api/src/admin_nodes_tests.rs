//! 管理端中转节点接口测试。
//! 本文件覆盖中转节点新增、鉴权码和安装说明相关 HTTP 行为。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 新增中转节点必须由管理员显式填写鉴权码。
//! 后台不能自动生成不可回显的节点鉴权码。
//! 所有请求均使用 example.test 和随机名称。
//! 测试不输出 token、服务器地址、密码或代理凭据。
//! 缺少 DATABASE_URL 时跳过，避免本地无数据库误报。
//! 文件前十行中文注释满足仓库规则。
//! 断言只检查状态码和脱敏响应。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_create_access_node_requires_auth_code_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API access node auth code test");
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
        app,
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": format!("auth-code-required-{}", Uuid::new_v4().simple()),
            "public_host": "auth-code-required.example.test",
            "public_port": 443,
            "agent_token": "",
            "remark": ""
        }),
    )
    .await;

    assert_eq!(created.0, StatusCode::UNPROCESSABLE_ENTITY, "{}", created.1);
    assert!(created.1["message"]
        .as_str()
        .unwrap_or_default()
        .contains("鉴权码"));
}

#[tokio::test]
async fn test_pg_create_access_node_accepts_installed_agent_auth_code_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API installed agent auth code test");
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

    let installed_node_id = Uuid::new_v4();
    let installed_token = format!("installed-agent-token-{}", Uuid::new_v4().simple());
    let auth_code = format!("xrayc-agent-v1:{installed_node_id}:{installed_token}");

    let created = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": format!("installed-agent-{}", Uuid::new_v4().simple()),
            "public_host": "installed-agent.example.test",
            "public_port": 443,
            "agent_token": auth_code,
            "remark": "installed first"
        }),
    )
    .await;

    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    assert_eq!(
        created.1["data"]["id"].as_str().unwrap(),
        installed_node_id.to_string()
    );
    assert!(store
        .verify_agent_token(Some(installed_node_id), &installed_token)
        .await
        .unwrap());
}

#[tokio::test]
async fn test_pg_create_access_node_rebinds_installed_agent_auth_code_when_node_exists() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API installed agent rebind test");
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

    let installed_node_id = Uuid::new_v4();
    let old_token = format!("installed-agent-token-old-{}", Uuid::new_v4().simple());
    let old_auth_code = format!("xrayc-agent-v1:{installed_node_id}:{old_token}");
    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": "installed-agent-rebind-old",
            "public_host": "installed-agent-rebind-old.example.test",
            "public_port": 443,
            "agent_token": old_auth_code,
            "remark": "installed first"
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);

    let new_token = format!("installed-agent-token-new-{}", Uuid::new_v4().simple());
    let new_auth_code = format!("xrayc-agent-v1:{installed_node_id}:{new_token}");
    let rebound = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": "installed-agent-rebind-new",
            "public_host": "installed-agent-rebind-new.example.test",
            "public_port": 8443,
            "agent_token": new_auth_code,
            "remark": "installed rebound"
        }),
    )
    .await;

    assert_eq!(rebound.0, StatusCode::CREATED, "{}", rebound.1);
    assert_eq!(
        rebound.1["data"]["id"].as_str().unwrap(),
        installed_node_id.to_string()
    );
    assert!(store
        .verify_agent_token(Some(installed_node_id), &new_token)
        .await
        .unwrap());
    assert!(!store
        .verify_agent_token(Some(installed_node_id), &old_token)
        .await
        .unwrap());
    let (name, public_host, public_port, remark) =
        sqlx::query_as::<_, (String, String, i32, String)>(
            "SELECT name, public_host, public_port, remark FROM access_nodes WHERE id = $1",
        )
        .bind(installed_node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(name, "installed-agent-rebind-new");
    assert_eq!(public_host, "installed-agent-rebind-new.example.test");
    assert_eq!(public_port, 8443);
    assert_eq!(remark, "installed rebound");
}

#[tokio::test]
async fn test_pg_create_node_accepts_cf_fields_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API node CF fields create test");
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

    // 用安装态鉴权码拿到确定的节点 ID,便于直接读库断言 CF 字段落库。
    let installed_node_id = Uuid::new_v4();
    let installed_token = format!("installed-cf-token-{}", Uuid::new_v4().simple());
    let auth_code = format!("xrayc-agent-v1:{installed_node_id}:{installed_token}");

    let created = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": format!("cf-fields-{}", Uuid::new_v4().simple()),
            "public_host": "cf-fields.example.test",
            "public_port": 443,
            "agent_token": auth_code,
            "remark": "cf node",
            "cert_domain": "direct.example.test",
            "acme_email": "admin@example.test",
            "cf_enabled": true,
            "cf_domain": "cdn.example.test"
        }),
    )
    .await;

    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    let fields = store
        .access_node_cf_fields(installed_node_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fields.cert_domain.as_deref(), Some("direct.example.test"));
    assert_eq!(fields.acme_email.as_deref(), Some("admin@example.test"));
    assert!(fields.cf_enabled);
    assert_eq!(fields.cf_domain.as_deref(), Some("cdn.example.test"));
}

#[tokio::test]
async fn test_pg_install_guide_includes_node_cert_domain_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API install guide cert_domain test");
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

    // 创建启用 CF 且带直连证书域名的节点(此节点无入口,inferred 域名为空)。
    let installed_node_id = Uuid::new_v4();
    let installed_token = format!("installed-guide-token-{}", Uuid::new_v4().simple());
    let auth_code = format!("xrayc-agent-v1:{installed_node_id}:{installed_token}");
    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-nodes",
        Some(admin_token),
        json!({
            "name": format!("guide-cert-{}", Uuid::new_v4().simple()),
            "public_host": "guide-cert.example.test",
            "public_port": 443,
            "agent_token": auth_code,
            "remark": "",
            "cert_domain": "guide-direct.example.test",
            "acme_email": "ops@example.test",
            "cf_enabled": true,
            "cf_domain": "guide-cdn.example.test"
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);

    // 安装指南不显式传 tls_cert_domains 时,应自动并入节点 cert_domain。
    let guide = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(admin_token),
        json!({
            "access_node_id": installed_node_id,
            "control_plane_url": "https://panel.example.test"
        }),
    )
    .await;
    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    let environment_text = guide.1["data"]["environment_text"].as_str().unwrap();
    assert!(
        environment_text.contains("XRAYC_TLS_CERT_DOMAINS")
            && environment_text.contains("guide-direct.example.test"),
        "install guide should carry node cert_domain into XRAYC_TLS_CERT_DOMAINS: {environment_text}"
    );
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
