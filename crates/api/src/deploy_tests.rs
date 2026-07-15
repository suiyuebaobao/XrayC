//! Agent 安装说明接口回归测试。
//! 当前安装说明只负责让 access-agent 上线，不承诺逻辑入口端口监听。
//! 用户级限速会把订阅入口渲染为用户运行时端口。
//! 因此未显式传入 expected_listen_ports 时，安装说明必须保持为空。
//! 测试使用 PostgreSQL 真实迁移和 demo 管理员登录。
//! 所有请求只使用 example.test 和测试 token，不输出私有部署信息。
//! 路由层断言防止后台再次自动推导 access_lines.listen_port。
//! 显式 expected_listen_ports 仍由校验函数和接口原样保留。
//! 文件前十行中文注释满足仓库规则。
//! 不在测试日志中输出服务器、密码或其它敏感信息。

use super::deploy_install::extract_installed_agent_auth_code_from_install_output;
use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;
use xrayc_db::AdminAccessNodeGroupEntryInput;

#[tokio::test]
async fn test_pg_install_guide_supports_agent_first_without_access_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API install guide read-only test");
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

    let guide = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(admin_token),
        json!({
            "control_plane_url": "https://panel.example.test"
        }),
    )
    .await;

    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    assert!(guide.1["data"].get("access_node_created").is_none());
    assert!(!guide.1["data"]["environment_text"]
        .as_str()
        .unwrap()
        .contains("XRAYC_NODE_ID"));
    assert!(guide.1["data"]["environment_text"]
        .as_str()
        .unwrap()
        .contains("XRAYC_DEPLOY_ARTIFACT_TOKEN"));
    assert!(guide.1["data"]["steps"].to_string().contains("节点鉴权码"));
}

#[tokio::test]
async fn test_pg_install_guide_creates_trackable_deployment_task_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API install task tracking test");
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

    let guide = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(admin_token),
        json!({
            "control_plane_url": "https://panel.example.test",
            "tls_cert_domains": ["node.example.test"]
        }),
    )
    .await;
    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    let task_id = guide.1["data"]["task"]["id"].as_str().unwrap();
    assert_eq!(guide.1["data"]["task"]["status"], "waiting_for_server");
    assert_eq!(guide.1["data"]["task"]["kind"], "agent_install");
    assert!(guide.1["data"]["task"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|step| step["status"] == "current"));

    let tasks = request_json(
        app,
        Method::GET,
        "/api/admin/deployment-tasks",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(tasks.0, StatusCode::OK, "{}", tasks.1);
    assert!(tasks.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["id"] == task_id));
}

#[tokio::test]
async fn test_pg_install_guide_does_not_infer_logic_listen_ports_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API install guide runtime port test");
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
            name: format!("install-guide-runtime-{}", Uuid::new_v4().simple()),
            public_host: "install-guide-runtime.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-placeholder-value".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "install-guide-runtime-resource".to_string(),
                    endpoint_name: "install-guide-runtime-endpoint".to_string(),
                    region_code: "TEST".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "install-guide-runtime.example.test".to_string(),
                    port: 35_299,
                    outbound_config: json!({}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let line_group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("install-guide-runtime-group-{}", Uuid::new_v4().simple()),
            country_code: "TEST".to_string(),
            icon: "TEST".to_string(),
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
    store
        .replace_admin_line_group_lines(line_group_id, vec![exit_endpoint_id])
        .await
        .unwrap();
    store
        .create_admin_access_node_group_entries(
            node_id,
            vec![AdminAccessNodeGroupEntryInput {
                name: "install-guide-runtime-entry".to_string(),
                exit_endpoint_id,
                listen_host: "install-guide-runtime.example.test".to_string(),
                listen_port: 35_301,
                protocol: "vless".to_string(),
                network_mode: "tcp".to_string(),
                inbound_config: json!({}),
                xhttp_mode: "auto".to_string(),
                enabled: true,
            }],
        )
        .await
        .unwrap();

    let guide = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(admin_token),
        json!({
            "access_node_id": node_id,
            "control_plane_url": "https://panel.example.test",
            "force_reinstall": true
        }),
    )
    .await;

    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    let envs = guide.1["data"]["environment"].as_array().unwrap();
    let expected_ports = envs
        .iter()
        .find(|item| item["name"] == "XRAYC_EXPECTED_LISTEN_PORTS")
        .and_then(|item| item["value"].as_str())
        .unwrap();
    assert_eq!(expected_ports, "");
    let env_text = guide.1["data"]["environment_text"].as_str().unwrap();
    assert!(env_text.contains("XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG='true'"));
    assert!(!env_text.contains("XRAYC_DEPLOY_OVERWRITE_XRAY_CONFIG="));
    assert!(!guide.1["data"]["environment_text"]
        .as_str()
        .unwrap()
        .contains("35301"));
}

#[tokio::test]
async fn test_pg_install_guide_installs_xray_runtime_core_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API install guide runtime core test");
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
            name: format!("install-guide-xray-{}", Uuid::new_v4().simple()),
            public_host: "install-guide-xray.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-placeholder-value".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let guide = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes/install-guide",
        Some(admin_token),
        json!({
            "access_node_id": node_id,
            "control_plane_url": "https://panel.example.test"
        }),
    )
    .await;

    assert_eq!(guide.0, StatusCode::OK, "{}", guide.1);
    let env_text = guide.1["data"]["environment_text"].as_str().unwrap();
    assert!(!env_text.contains("XRAYC_RUNTIME_CORE="));
    assert!(!env_text.contains("XRAYC_CORE_TYPE="));
    assert!(env_text.contains("XRAYC_XRAY_CONFIG_PATH='/etc/xray/config.json'"));
    // BUG-E 补全(对照部署脚本 651819f):手动安装引导也绝不能 emit XRAYC_XRAY_TEST_COMMAND——
    // 该命令含运行时 $XRAYC_XRAY_CONFIG,经 env_file 注入会被 compose v2 插空(-config "")导致
    // agent xray -test 必败、节点永不收敛;改由 agent config.rs 内置等价默认接管(agent 自身展开)。
    assert!(!env_text.contains("XRAYC_XRAY_TEST_COMMAND="));
    // sing-box 内核已移除，安装环境变量里不得再出现任何 sing-box 字段。
    assert!(!env_text.contains("XRAYC_SING_BOX_CONFIG_PATH="));
    assert!(!env_text.contains("XRAYC_SING_BOX_TEST_COMMAND="));
    assert!(!env_text.contains("XRAYC_XRAY_RELOAD_COMMAND="));
    assert!(!env_text.contains("XRAYC_XRAY_START_COMMAND="));
    assert!(!env_text.contains("XRAYC_XRAY_STOP_COMMAND="));
    assert!(!env_text.contains("XRAYC_SING_BOX_START_COMMAND="));
    assert!(!env_text.contains("XRAYC_SING_BOX_STOP_COMMAND="));
}

#[tokio::test]
async fn test_pg_one_click_install_creates_task_and_rebinds_installed_node_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping API one-click install test");
        return;
    };
    let installed_node_id = Uuid::new_v4();
    let ssh_pass = ["unit", "ssh", "password", "never", "store"].join("-");
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    std::env::set_var(
        "XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT",
        format!(
            "deployment completed\n节点鉴权码：xrayc-agent-v1:{installed_node_id}:unit-agent-token\n"
        ),
    );

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
            "name": "one-click-node",
            "public_host": "one-click.example.test",
            "public_port": 443,
            "remark": "created by one-click test",
            "ssh_host": "192.0.2.10",
            "ssh_port": 22,
            "ssh_user": "root",
            "ssh_password": ssh_pass,
            "control_plane_url": "https://panel.example.test",
            "install_dir": "/opt/xrayc/access-agent",
            "compose_project": "xrayc-access",
            "tls_cert_domains": ["node.example.test"],
            "acme_email": "ops@example.test",
            "force_reinstall": false
        }),
    )
    .await;

    assert_eq!(queued.0, StatusCode::OK, "{}", queued.1);
    assert_eq!(queued.1["data"]["status"], "queued");
    assert_eq!(queued.1["data"]["task"]["kind"], "agent_install");
    assert_eq!(
        queued.1["data"]["task"]["status"], "running",
        "one-click install must start immediately instead of waiting for manual execution"
    );
    assert!(
        queued.1["data"]["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("自动连接服务器、上传并执行脚本"),
        "one-click install response must make clear that the platform uploads and runs the script automatically"
    );
    assert_eq!(
        queued.1["data"]["task"]["safe_metadata"]["tls_cert_domain_count"],
        1
    );
    assert_eq!(
        queued.1["data"]["task"]["safe_metadata"]["tls_cert_email_present"],
        true
    );
    assert_eq!(
        queued.1["data"]["task"]["safe_metadata"]["force_reinstall"], true,
        "one-click install must force reinstall even when the client sends false"
    );
    assert!(!queued.1.to_string().contains(&ssh_pass));

    let mut created_node_found = false;
    for _ in 0..20 {
        let listed_nodes = request_json(
            app.clone(),
            Method::GET,
            "/api/admin/access-nodes",
            Some(admin_token),
            Value::Null,
        )
        .await;
        assert_eq!(listed_nodes.0, StatusCode::OK, "{}", listed_nodes.1);
        created_node_found = listed_nodes.1["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| {
                node["id"] == installed_node_id.to_string()
                    && node["name"] == "one-click-node"
                    && node["public_host"] == "one-click.example.test"
            });
        if created_node_found {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        created_node_found,
        "one-click installed node was not created"
    );

    let tasks = request_json(
        app,
        Method::GET,
        "/api/admin/deployment-tasks",
        Some(admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(tasks.0, StatusCode::OK, "{}", tasks.1);
    let task_text = tasks.1.to_string();
    assert!(task_text.contains("one_click"));
    assert!(!task_text.contains(&ssh_pass));
    let task = tasks.1["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["safe_metadata"]["access_node_name"] == "one-click-node")
        .expect("one-click task should be listed");
    assert_eq!(task["safe_metadata"]["mode"], "one_click");
    assert_eq!(
        task["safe_metadata"]["public_host"],
        "one-click.example.test"
    );
    let steps = task["steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|step| step["key"] == "ssh_connect" && step["status"] == "done"),
        "ssh_connect should be marked done after successful install: {steps:?}"
    );
    assert!(
        steps
            .iter()
            .any(|step| step["key"] == "node_registered" && step["status"] == "done"),
        "node_registered should be marked done after successful install: {steps:?}"
    );

    std::env::remove_var("DEPLOY_ARTIFACT_TOKEN");
    std::env::remove_var("XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT");
}

#[test]
fn test_one_click_install_extracts_auth_code_from_stderr_logs() {
    let node_id = Uuid::new_v4();
    let stdout = "download completed\n";
    let stderr =
        format!("[xrayc-access-deploy] 节点鉴权码：xrayc-agent-v1:{node_id}:stderr-agent-token\n");

    let auth_code = extract_installed_agent_auth_code_from_install_output(stdout, &stderr)
        .expect("auth code should be parsed from deploy script stderr logs");

    assert_eq!(
        auth_code,
        format!("xrayc-agent-v1:{node_id}:stderr-agent-token")
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
