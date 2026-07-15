//! 一键安装健壮性回归测试:就绪等待超时降级 + 重装熔断。
//! 缺陷②:进度回报跳过/就绪等待超时让脚本非 0 退出但鉴权码已输出,不得据此判失败。
//! 缺陷②:同一 SSH 目标短时间内强制重装次数达上限必须熔断,杜绝自伤式重装风暴。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录,DATABASE_URL 缺失时跳过。
//! 一键安装 SSH 走 cfg(test) 假输出,通过 XRAYC_ONE_CLICK_INSTALL_FAKE_* 注入退出码/输出。
//! 所有请求只用 example.test 与 RFC5737 占位 IP,不输出私有部署信息或真实凭据。
//! 从 deploy_tests 拆出,保持单文件不超 550 行硬上限(CLAUDE.md §8)。
//! request_json 辅助在本文件内自带,避免跨测试模块耦合。
//! 文件前十行中文注释满足仓库规则。
//! 不在测试日志中输出服务器、密码或其它敏感信息。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn test_pg_one_click_install_succeeds_when_script_exits_nonzero_but_emits_auth_code_when_database_url_is_set(
) {
    // 缺陷②回归:就绪等待超时/进度回报跳过会让安装脚本以非 0 退出,但容器已 running、
    // 鉴权码已先输出。成功判定以"鉴权码解析到"(agent 会凭心跳收敛)为准,不得据脚本非 0 退出判 failed,
    // 否则调用方据 failed 强制重装会把快装好的 agent 清掉重来 → 自伤式重装风暴。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping one-click nonzero-exit downgrade test");
        return;
    };
    let installed_node_id = Uuid::new_v4();
    let ssh_pass = ["unit", "ssh", "password", "never", "store"].join("-");
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    // 模拟脚本就绪等待超时:鉴权码已输出,但脚本非 0 退出(stderr 含进度回报跳过)。
    std::env::set_var(
        "XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT",
        format!(
            "deployment completed\n节点鉴权码：xrayc-agent-v1:{installed_node_id}:unit-agent-token\n"
        ),
    );
    std::env::set_var(
        "XRAYC_ONE_CLICK_INSTALL_FAKE_STDERR",
        "[xrayc-access-deploy] deployment progress report skipped\n",
    );
    std::env::set_var("XRAYC_ONE_CLICK_INSTALL_FAKE_FAILED", "1");

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

    let queued = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-nodes/one-click-install",
        Some(admin_token),
        json!({
            "name": "one-click-downgrade-node",
            "public_host": "one-click-downgrade.example.test",
            "public_port": 443,
            "ssh_host": "192.0.2.11",
            "ssh_port": 22,
            "ssh_user": "root",
            "ssh_password": ssh_pass,
            "control_plane_url": "https://panel.example.test",
            "force_reinstall": false
        }),
    )
    .await;
    assert_eq!(queued.0, StatusCode::OK, "{}", queued.1);

    let mut succeeded = false;
    for _ in 0..40 {
        let tasks = request_json(
            app.clone(),
            Method::GET,
            "/api/admin/deployment-tasks",
            Some(admin_token),
            Value::Null,
        )
        .await;
        assert_eq!(tasks.0, StatusCode::OK, "{}", tasks.1);
        if let Some(task) = tasks.1["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["safe_metadata"]["access_node_name"] == "one-click-downgrade-node")
        {
            // 关键断言:脚本非 0 退出 + 鉴权码已输出,任务最终必须 succeeded,绝不 failed。
            assert_ne!(
                task["status"], "failed",
                "鉴权码已解析到时绝不能判 failed(否则触发自伤式重装): {task}"
            );
            if task["status"] == "succeeded" {
                succeeded = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        succeeded,
        "脚本非 0 退出但已输出鉴权码时,一键安装任务应降级为 succeeded 并登记节点"
    );

    std::env::remove_var("DEPLOY_ARTIFACT_TOKEN");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_STDERR");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_FAILED");
}

#[tokio::test]
async fn test_pg_one_click_install_reinstall_circuit_breaker_when_database_url_is_set() {
    // 缺陷②重装熔断回归:同一 SSH 目标短时间内强制重装次数达上限后必须熔断拒绝,
    // 不允许"见 failed 就无条件 forced-reinstall"反复清掉快装好的 agent 导致节点永不收敛。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping reinstall circuit breaker test");
        return;
    };
    let ssh_pass = ["unit", "ssh", "password", "never", "store"].join("-");
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    std::env::set_var(
        "XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT",
        format!(
            "deployment completed\n节点鉴权码：xrayc-agent-v1:{}:unit-agent-token\n",
            Uuid::new_v4()
        ),
    );
    // 收紧上限到 2 次便于断言;窗口足够长覆盖本测试。
    std::env::set_var("XRAYC_ONE_CLICK_REINSTALL_LIMIT", "2");
    std::env::set_var("XRAYC_ONE_CLICK_REINSTALL_WINDOW_SECONDS", "600");

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

    // 用本测试专属的占位 SSH IP,避免与其它测试任务串台计数。
    let ssh_host = "192.0.2.222";
    let install_once = |name: &str| {
        let app = app.clone();
        let body = json!({
            "name": name,
            "public_host": format!("{name}.example.test"),
            "public_port": 443,
            "ssh_host": ssh_host,
            "ssh_port": 22,
            "ssh_user": "root",
            "ssh_password": ssh_pass.clone(),
            "control_plane_url": "https://panel.example.test",
            "force_reinstall": false
        });
        async move {
            request_json(
                app,
                Method::POST,
                "/api/admin/access-nodes/one-click-install",
                Some(admin_token),
                body,
            )
            .await
        }
    };

    // 前 2 次(达上限)应通过排队。
    let first = install_once("breaker-node-1").await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    let second = install_once("breaker-node-2").await;
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);

    // 第 3 次同一 SSH 目标必须被熔断拒绝(已达 2 次上限)。
    let third = install_once("breaker-node-3").await;
    assert_ne!(
        third.0,
        StatusCode::OK,
        "同一 SSH 目标超过强制重装上限后必须熔断: {}",
        third.1
    );
    assert!(
        third.1.to_string().contains("熔断"),
        "熔断拒绝必须给出明确提示: {}",
        third.1
    );

    std::env::remove_var("DEPLOY_ARTIFACT_TOKEN");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT");
    std::env::remove_var("XRAYC_ONE_CLICK_REINSTALL_LIMIT");
    std::env::remove_var("XRAYC_ONE_CLICK_REINSTALL_WINDOW_SECONDS");
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
