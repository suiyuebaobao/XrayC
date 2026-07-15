// 数据库测试分片 68（网络模式合并值 + Reality/域名直连入口护栏回归）。
// 本文件覆盖两条口径:
// ① 本机出口网络模式接受逗号合并值 tcp,udp（仅 SS/SOCKS 允许),只建 1 条线路、
//    stream_config.network 落库为 tcp,udp,不再逐 mode 拆成两条。
// ② 入口护栏防御纵深:node_domain_id 指向 kind=direct 直连域名 且 security=reality 时拒绝创建。
// 测试只用 RFC 5737 文档 IP、示例域名(example.test)与隔离 PostgreSQL,不访问远端节点。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过(与 §9 测试要求一致)。
// 读回断言以落库(exit_endpoints.stream_config / create_admin_access_entry 返回)为唯一真相。
// 节点/域名/入口创建复用 PgStore 公开写入口,不造不存在的 helper。
// 本头部满足前十行中文注释约束。

/// 构造一条本机出口 SS 线路输入,网络模式由调用方给(用于验合并值 tcp,udp)。
fn ss_local_exit_line_input(network_mode: &str) -> AdminLocalExitLineInput {
    AdminLocalExitLineInput {
        resource_name: "nm-merge-resource".to_string(),
        endpoint_name: "nm-merge-endpoint".to_string(),
        region_code: "US".to_string(),
        outbound_type: "shadowsocks".to_string(),
        network_mode: network_mode.to_string(),
        host: "198.51.100.68".to_string(),
        port: 38_268,
        // SS 出口需要 method + password,fill 侧会补默认 method;给齐避免协议字段护栏拦。
        outbound_config: json!({
            "method": "aes-128-gcm",
            "password": "merge-secret"
        }),
        stream_config: json!({}),
        probe_config: json!({}),
        enabled: true,
        node_domain_id: None,
    }
}

/// 构造一个 VLESS+Reality 入口输入(TCP 模式,Reality 仅 TCP/XHTTP/gRPC)。
/// node_domain_id 由调用方覆盖,用于验「指向 direct 域名时被拒」。
fn vless_reality_entry_input(node_id: Uuid) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: "nm-reality-entry".to_string(),
        listen_host: String::new(),
        listen_port: 443,
        protocol: "vless".to_string(),
        transport: "tcp".to_string(),
        security: "reality".to_string(),
        server_name: "www.cloudflare.com".to_string(),
        ws_path: String::new(),
        ws_host: String::new(),
        cdn_enabled: false,
        cdn_provider: String::new(),
        cdn_hostname: String::new(),
        cdn_server: String::new(),
        enabled: true,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

#[tokio::test]
async fn test_local_exit_accepts_merged_tcp_udp_network() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping merged tcp,udp local-exit network mode test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 纯 IP 节点即可:SS 免证书,合并 tcp,udp 不依赖域名。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("nm-merge-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.68".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("nm-merge-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();

    // SS 本机出口选 tcp,udp:应被接受、只建 1 条线路(不再拆成 2 条)。
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![ss_local_exit_line_input("tcp,udp")],
            },
        )
        .await
        .unwrap();

    let created_lines = created["created_lines"].as_array().unwrap();
    assert_eq!(created_lines.len(), 1, "SS 选 tcp,udp 应只建 1 条本机出口线路");
    assert_eq!(
        created_lines[0]["network_mode"].as_str().unwrap(),
        "tcp,udp",
        "合并网络模式应规范化为 tcp,udp"
    );

    let exit_endpoint_id =
        Uuid::parse_str(created_lines[0]["exit_endpoint_id"].as_str().unwrap()).unwrap();
    // 落库读回:stream_config.network 必须是合并值 tcp,udp(透传给 Xray SS inbound 的 settings.network)。
    let network: String =
        sqlx::query_scalar("SELECT stream_config->>'network' FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(network, "tcp,udp", "stream_config.network 应落库为合并值 tcp,udp");
}

#[tokio::test]
async fn test_reality_entry_rejected_on_direct_domain() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping reality-on-direct-domain entry guard test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 节点配直连证书域名,并显式加一个 kind=direct 的 node_domain 供入口选中。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("nm-reality-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.69".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("nm-reality-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("ops@example.test".to_string()),
            ip_direct_address: Some("203.0.113.69".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    // 节点带 cert_domain 创建时已自动同步出一个 kind=direct 主域名;取它的 id 给入口选中
    //（再 add 同名会冲突,故直接查回库里那条 direct 域名）。
    let direct_domain_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM node_domains WHERE access_node_id = $1 AND kind = 'direct' LIMIT 1",
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();

    // VLESS+Reality 入口指向该 direct 域名 → 域名直连不支持 Reality,必须被拒。
    let mut input = vless_reality_entry_input(node_id);
    input.node_domain_id = Some(direct_domain_id);
    let err = store.create_admin_access_entry(input).await.unwrap_err();
    assert!(
        matches!(err, DbError::InvalidAgentPayload(_)),
        "Reality + direct 域名入口应被 InvalidAgentPayload 拒绝,实际: {err:?}"
    );
}

#[tokio::test]
async fn test_local_exit_http_rejects_udp_carriage() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping http-udp local-exit guard test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("nm-http-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.78".to_string(),
            public_port: 443,
            agent_token: format!("nm-http-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();

    // HTTP 代理纯 TCP:选 udp 承载必须被后端兜底拒(前端也已不给该选项)。
    let mut input = ss_local_exit_line_input("udp");
    input.outbound_type = "http".to_string();
    input.outbound_config = json!({});
    let err = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput { lines: vec![input] },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, DbError::InvalidAgentPayload(_)),
        "HTTP + udp 承载应被 InvalidAgentPayload 拒绝,实际: {err:?}"
    );
}
