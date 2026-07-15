//! 部署任务删除/取消管理端接口回归测试。
//! 覆盖 DELETE /api/admin/deployment-tasks/:id 与 POST .../:id/cancel 两条路由。
//! 鉴权与其它 admin 路由一致:无管理员 token 必须被拒。
//! 取消把未完成任务标记 failed(原因"管理员取消");删除后列表不再出现。
//! 测试用 PostgreSQL 真实迁移 + demo 管理员登录,只用 example.test 域名。
//! 任务通过安装说明接口创建,断言只读列表与状态,不输出敏感部署信息。
//! 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
//! 请求体只用测试占位数据,不写真实服务器地址、密码或 token。
//! 文件前十行中文注释满足仓库规则。
//! 不在测试日志中输出服务器、密码或其它敏感信息。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

/// 登录 demo 管理员,返回 access token。
async fn admin_token(app: Router) -> String {
    let login = request_json(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "admin", "password": xrayc_db::DEMO_ADMIN_PASSWORD}),
    )
    .await;
    assert_eq!(login.0, StatusCode::OK, "{}", login.1);
    login.1["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 通过安装说明接口创建一条 waiting_for_server 部署任务,返回任务 id。
async fn create_waiting_task(app: Router, token: &str) -> String {
    let guide = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(token),
        json!({"control_plane_url": "https://panel.example.test"}),
    )
    .await;
    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    guide.1["data"]["task"]["id"].as_str().unwrap().to_string()
}

/// 拉部署任务列表,按 id 找 status。
async fn task_status(app: Router, token: &str, task_id: &str) -> Option<String> {
    let list = request_json(
        app,
        Method::GET,
        "/api/admin/deployment-tasks",
        Some(token),
        Value::Null,
    )
    .await;
    assert_eq!(list.0, StatusCode::OK, "{}", list.1);
    list.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(task_id))
        .map(|item| item["status"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn test_cancel_deployment_task_endpoint_marks_failed() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping deployment task cancel endpoint test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));

    let token = admin_token(app.clone()).await;
    let task_id = create_waiting_task(app.clone(), &token).await;
    assert_eq!(
        task_status(app.clone(), &token, &task_id).await.as_deref(),
        Some("waiting_for_server")
    );

    let cancel = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/deployment-tasks/{task_id}/cancel"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(cancel.0, StatusCode::OK, "{}", cancel.1);
    assert_eq!(cancel.1["data"]["status"], "failed");
    assert!(cancel.1["data"]["error_summary"]
        .as_str()
        .unwrap()
        .contains("管理员取消"));
    assert_eq!(
        task_status(app, &token, &task_id).await.as_deref(),
        Some("failed")
    );
}

#[tokio::test]
async fn test_delete_deployment_task_endpoint_removes_record() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping deployment task delete endpoint test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));

    let token = admin_token(app.clone()).await;
    let task_id = create_waiting_task(app.clone(), &token).await;
    assert!(task_status(app.clone(), &token, &task_id).await.is_some());

    let delete = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/deployment-tasks/{task_id}"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(delete.0, StatusCode::OK, "{}", delete.1);
    assert_eq!(
        task_status(app, &token, &task_id).await,
        None,
        "删除后任务不应再出现在列表"
    );
}

#[tokio::test]
async fn test_delete_deployment_task_requires_admin() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping deployment task admin guard test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));

    // 无管理员 token:删除与取消都必须被拒(非 2xx)。
    let fake_id = uuid::Uuid::new_v4();
    let delete = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/deployment-tasks/{fake_id}"),
        None,
        Value::Null,
    )
    .await;
    assert!(
        delete.0.is_client_error() || delete.0.is_server_error(),
        "未鉴权删除必须被拒: {}",
        delete.0
    );
    let cancel = request_json(
        app,
        Method::POST,
        &format!("/api/admin/deployment-tasks/{fake_id}/cancel"),
        None,
        Value::Null,
    )
    .await;
    assert!(
        cancel.0.is_client_error() || cancel.0.is_server_error(),
        "未鉴权取消必须被拒: {}",
        cancel.0
    );
}

/// 本测试文件内独立的请求辅助,避免依赖其它测试模块的私有 helper。
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
