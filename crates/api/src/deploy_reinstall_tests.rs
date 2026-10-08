//! 一键安装健壮性回归测试:就绪等待超时降级 + 重装熔断。
//! 非致命就绪等待由脚本转为警告；真正失败或回退即使曾打印鉴权码也不能宣称安装成功。
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
async fn test_pg_one_click_install_rejects_failed_script_even_with_auth_code_when_database_url_is_set(
) {
    // 失败脚本可能已经恢复旧实例，鉴权码只证明曾执行到某个步骤。
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

    let mut failed = false;
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
            if task["status"] == "failed" {
                failed = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    std::env::remove_var("DEPLOY_ARTIFACT_TOKEN");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_STDERR");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_FAILED");
    assert!(failed, "脚本失败必须如实失败，不得仅凭鉴权码登记成功");
    let registered =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM access_nodes WHERE id=$1)")
            .bind(installed_node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(!registered);
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
