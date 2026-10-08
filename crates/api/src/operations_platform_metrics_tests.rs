//! 监控中心·平台指标端点与心跳节点指标入库测试(阶段 C+D)。
//! 覆盖 GET /api/admin/access-operations/platform-metrics:非 admin→403、admin→200 且契约完整。
//! 覆盖 POST /api/agent/access/heartbeat 带 node_metrics→落库 node_runtime_metrics(真实查表有行)。
//! 测试用真实 PostgreSQL 迁移 + demo 种子,缺 DATABASE_URL 时跳过避免本地无库误报。
//! 心跳鉴权用 demo agent token,平台指标用 demo 管理员/普通用户 JWT。
//! 断言只检查状态码、契约字段与脱敏数值,不输出 token/地址/凭据。
//! 平台指标只读宿主 /proc 与 statvfs + 库存储,不改任何业务数据。
//! 节点指标入库行为向后兼容:无 node_metrics 心跳不写时序表。
//! 文件前十行中文注释满足仓库源码头部约束。
//! 不改变生产 API 路由或既有响应语义。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

const DEMO_NODE_ID: &str = "00000000-0000-0000-0000-000000000201";

#[tokio::test]
async fn test_pg_platform_metrics_forbidden_for_non_admin() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping platform metrics non-admin test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store));

    // 普通(非管理员)用户登录,拿到 access token。
    let user_login = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "demo@example.test", "password": xrayc_db::DEMO_USER_PASSWORD}),
    )
    .await;
    assert_eq!(user_login.0, StatusCode::OK, "{}", user_login.1);
    let user_token = user_login.1["data"]["access_token"].as_str().unwrap();

    // 非管理员访问平台指标必须被 403 拒绝(管理员鉴权红线)。
    let forbidden = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/access-operations/platform-metrics",
        Some(user_token),
        Value::Null,
    )
    .await;
    assert_eq!(forbidden.0, StatusCode::FORBIDDEN, "{}", forbidden.1);

    // 未登录访问被 401 拒绝。
    let unauth = request_json(
        app,
        Method::GET,
        "/api/admin/access-operations/platform-metrics",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(unauth.0, StatusCode::UNAUTHORIZED, "{}", unauth.1);
}

#[tokio::test]
async fn test_pg_platform_metrics_returns_contract_for_admin() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping platform metrics admin test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store));

    let admin_token = admin_token(app.clone()).await;

    let resp = request_json(
        app,
        Method::GET,
        "/api/admin/access-operations/platform-metrics",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(resp.0, StatusCode::OK, "{}", resp.1);
    let data = &resp.1["data"];

    // 顶层契约字段齐全。
    assert!(
        data["generated_at"].is_string(),
        "generated_at 缺失: {data}"
    );

    // CPU:千分比为整数且在 [0,100000]。
    let cpu_pct = data["cpu"]["pct_milli"]
        .as_i64()
        .expect("cpu.pct_milli 为整数");
    assert!(
        (0..=100_000).contains(&cpu_pct),
        "cpu.pct_milli 越界: {cpu_pct}"
    );

    // 内存:total_bytes>0,used<=total,千分比互补落在合理范围。
    let mem = &data["memory"];
    let mem_total = mem["total_bytes"]
        .as_i64()
        .expect("memory.total_bytes 为整数");
    let mem_used = mem["used_bytes"]
        .as_i64()
        .expect("memory.used_bytes 为整数");
    assert!(mem_total > 0, "memory.total_bytes 应为正: {mem}");
    assert!(mem_used <= mem_total, "memory.used 不应超过 total: {mem}");
    assert!(mem["used_pct_milli"].is_i64(), "memory.used_pct_milli 缺失");
    assert!(mem["free_pct_milli"].is_i64(), "memory.free_pct_milli 缺失");

    // 磁盘:契约字段齐全,host_mounted 为布尔,mount_point 为字符串。
    let disk = &data["disk"];
    let disk_total = disk["total_bytes"]
        .as_i64()
        .expect("disk.total_bytes 为整数");
    let disk_used = disk["used_bytes"].as_i64().expect("disk.used_bytes 为整数");
    assert!(disk_total > 0, "disk.total_bytes 应为正: {disk}");
    assert!(disk_used <= disk_total, "disk.used 不应超过 total: {disk}");
    assert!(disk["used_pct_milli"].is_i64(), "disk.used_pct_milli 缺失");
    assert!(disk["free_pct_milli"].is_i64(), "disk.free_pct_milli 缺失");
    assert!(disk["mount_point"].is_string(), "disk.mount_point 缺失");
    assert!(disk["host_mounted"].is_boolean(), "disk.host_mounted 缺失");

    // 数据库:total_bytes>0,tables 为数组且每项有 name/bytes。
    let database = &data["database"];
    let db_total = database["total_bytes"]
        .as_i64()
        .expect("database.total_bytes 为整数");
    assert!(db_total > 0, "database.total_bytes 应为正: {database}");
    let tables = database["tables"]
        .as_array()
        .expect("database.tables 为数组");
    assert!(!tables.is_empty(), "database.tables 不应为空");
    for table in tables {
        assert!(table["name"].is_string(), "table.name 缺失: {table}");
        assert!(table["bytes"].is_i64(), "table.bytes 缺失: {table}");
    }
}

#[tokio::test]
async fn test_pg_heartbeat_records_node_metrics() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping heartbeat node_metrics test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = Uuid::parse_str(DEMO_NODE_ID).unwrap();

    // 入库前清空该节点时序行,保证断言只看本次心跳写入的新行。
    sqlx::query("DELETE FROM node_runtime_metrics WHERE access_node_id = $1")
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pool = store.pool().clone();
    let app = app(AppState::with_pg(store));

    // 带 node_metrics 的心跳:用 demo agent token 鉴权。
    let heartbeat = heartbeat_request(
        app,
        node_id,
        Some(json!({
            "cpu_pct_milli": 42_000u32,
            "mem_used_bytes": 6_291_456_000u64,
            "mem_total_bytes": 8_388_608_000u64,
            "disk_used_bytes": 10_000_000_000u64,
            "disk_total_bytes": 50_000_000_000u64,
            "collected_at_unix": 1_700_000_000i64
        })),
    )
    .await;
    assert_eq!(heartbeat.0, StatusCode::OK, "{}", heartbeat.1);

    // 真实查表:该节点应恰好落一行,且字段与上报一致。
    let row: (i32, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT cpu_pct_milli, mem_used_bytes, mem_total_bytes,
                  disk_used_bytes, disk_total_bytes
           FROM node_runtime_metrics
           WHERE access_node_id = $1
           ORDER BY collected_at DESC
           LIMIT 1"#,
    )
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .expect("心跳应写入一行 node_runtime_metrics");
    assert_eq!(row.0, 42_000, "cpu_pct_milli 不一致");
    assert_eq!(row.1, 6_291_456_000, "mem_used_bytes 不一致");
    assert_eq!(row.2, 8_388_608_000, "mem_total_bytes 不一致");
    assert_eq!(row.3, 10_000_000_000, "disk_used_bytes 不一致");
    assert_eq!(row.4, 50_000_000_000, "disk_total_bytes 不一致");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM node_runtime_metrics WHERE access_node_id = $1")
            .bind(node_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "带 node_metrics 的心跳应恰好写入一行");
}

#[tokio::test]
async fn test_pg_heartbeat_without_node_metrics_writes_no_row() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping heartbeat backward-compat test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = Uuid::parse_str(DEMO_NODE_ID).unwrap();

    sqlx::query("DELETE FROM node_runtime_metrics WHERE access_node_id = $1")
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pool = store.pool().clone();
    let app = app(AppState::with_pg(store));

    // 不带 node_metrics 的心跳:向后兼容,不写时序表。
    let heartbeat = heartbeat_request(app, node_id, None).await;
    assert_eq!(heartbeat.0, StatusCode::OK, "{}", heartbeat.1);

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM node_runtime_metrics WHERE access_node_id = $1")
            .bind(node_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "无 node_metrics 的心跳不应写入时序表");
}

/// demo 管理员登录拿 access token。
async fn admin_token(app: Router) -> String {
    let admin_login = request_json(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "admin", "password": xrayc_db::DEMO_ADMIN_PASSWORD}),
    )
    .await;
    assert_eq!(admin_login.0, StatusCode::OK, "{}", admin_login.1);
    admin_login.1["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 用 demo agent token 发一条心跳;node_metrics 可选。
async fn heartbeat_request(
    app: Router,
    node_id: Uuid,
    node_metrics: Option<Value>,
) -> (StatusCode, Value) {
    let mut body = json!({ "node_id": node_id.to_string() });
    if let Some(metrics) = node_metrics {
        body["node_metrics"] = metrics;
    }
    let builder = Request::builder()
        .method(Method::POST)
        .uri("/api/agent/access/heartbeat")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", xrayc_db::DEMO_AGENT_TOKEN),
        )
        .header(header::CONTENT_TYPE, "application/json");
    let response = app
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
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

#[tokio::test]
async fn test_pg_system_info_requires_admin_and_returns_reported_versions() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    store
        .register_worker_version("0.1.0", "fixture-release", 60)
        .await
        .unwrap();
    let app = app(AppState::with_pg(store));
    let unauth = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/system-info",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(unauth.0, StatusCode::UNAUTHORIZED);
    let user = request_json(
        app.clone(),
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account":"demo@example.test","password":xrayc_db::DEMO_USER_PASSWORD}),
    )
    .await;
    let forbidden = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/system-info",
        user.1["data"]["access_token"].as_str(),
        Value::Null,
    )
    .await;
    assert_eq!(forbidden.0, StatusCode::FORBIDDEN);
    let token = admin_token(app.clone()).await;
    let response = request_json(
        app,
        Method::GET,
        "/api/admin/system-info",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(response.0, StatusCode::OK);
    assert!(response.1["data"]["api"]["release_id"].is_string());
    assert_eq!(
        response.1["data"]["workers"][0]["release_id"],
        "fixture-release"
    );
    assert_eq!(response.1["data"]["workers"][0]["fresh"], true);
    assert!(!response.1.to_string().contains(xrayc_db::DEMO_AGENT_TOKEN));
}
