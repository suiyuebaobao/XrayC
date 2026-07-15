/// 数据库测试分片 51。
// 本文件覆盖节点 3 地址(IP 直连)+ cf_cert_mode dns01 + cf_enabled 派生。
// 测试只使用示例域名、RFC 文档地址和测试 UUID,不写真实服务器/凭据。
// 节点新增 ip_direct_address 列;cf_cert_mode CHECK 放开到 dns01。
// cf_enabled 由 cf_domain 是否非空派生,不信任入参。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
// 父级 tests 模块提供 PgStore、输入类型、json 和 pg_test_guard。
// 读回断言以写入侧落库为唯一真相,符合跨层对齐红线。
// 不在这里写真实 token、订阅或代理凭据。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_access_nodes_table_has_ip_direct_and_dns01_cert_mode_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node ip_direct/dns01 migration test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 断言 ip_direct_address 列存在。
    let columns = sqlx::query_scalar::<_, String>(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'access_nodes'
          AND column_name = 'ip_direct_address'
        "#,
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(columns, vec!["ip_direct_address".to_string()]);

    // 断言 cf_cert_mode CHECK 放开到 dns01:插一条 dns01 成功。
    let node_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO access_nodes (id, name, public_host, public_port, agent_token_hash, cf_cert_mode)
        VALUES ($1, 'dns01-node', 'dns01.example.test', 443, 'hash-dns01', 'dns01')
        "#,
    )
    .bind(node_id)
    .execute(store.pool())
    .await
    .expect("dns01 cf_cert_mode should be accepted by CHECK");

    // 断言非法值仍被 CHECK 拒绝。
    let bad_id = uuid::Uuid::new_v4();
    let bad = sqlx::query(
        r#"
        INSERT INTO access_nodes (id, name, public_host, public_port, agent_token_hash, cf_cert_mode)
        VALUES ($1, 'bad-node', 'bad.example.test', 443, 'hash-bad', 'origin_ca')
        "#,
    )
    .bind(bad_id)
    .execute(store.pool())
    .await;
    assert!(bad.is_err(), "illegal cf_cert_mode should be rejected by CHECK");
}

#[tokio::test]
async fn test_node_persists_ip_direct_and_cf_cert_mode_and_derives_cf_enabled() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node ip_direct persist test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 建节点带 ip_direct_address + cf_domain,但不显式传 cf_enabled(默认 false)。
    // 期望:ip_direct_address 落库;cf_enabled 由 cf_domain 派生为 true;cf_cert_mode=dns01。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "three-addr-node".to_string(),
            public_host: "three-addr.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-three-addr-token".to_string(),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("admin@example.test".to_string()),
            cf_enabled: false,
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: Some("192.0.2.50".to_string()),
        })
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let node = find_node_in_routing(&routing, node_id);
    assert_eq!(node["ip_direct_address"].as_str(), Some("192.0.2.50"));
    assert_eq!(
        node["cf_enabled"].as_bool(),
        Some(true),
        "cf_enabled must be derived true when cf_domain present"
    );
    // cf_cert_mode 默认 reuse_direct(token-less 免 token);acme 不再当 dns01 判据。
    assert_eq!(node["cf_cert_mode"].as_str(), Some("reuse_direct"));

    // 只填 IP、无 cf_domain → cf_enabled 派生为 false。
    let ip_only_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "ip-only-node".to_string(),
            public_host: "ip-only.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-ip-only-token".to_string(),
            cert_domain: None,
            acme_email: None,
            cf_enabled: true,
            cf_domain: None,
            ip_direct_address: Some("192.0.2.60".to_string()),
        })
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let ip_node = find_node_in_routing(&routing, ip_only_id);
    assert_eq!(ip_node["ip_direct_address"].as_str(), Some("192.0.2.60"));
    assert_eq!(
        ip_node["cf_enabled"].as_bool(),
        Some(false),
        "cf_enabled must be derived false when cf_domain absent even if input cf_enabled=true"
    );
}

#[tokio::test]
async fn test_access_routing_json_node_includes_ip_direct_and_cert_mode() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node ip_direct read-model JSON test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "readmodel-3addr-node".to_string(),
            public_host: "readmodel-3addr.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-readmodel-3addr-token".to_string(),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("admin@example.test".to_string()),
            cf_enabled: false,
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: Some("192.0.2.70".to_string()),
        })
        .await
        .unwrap();

    // 控制面节点摘要 JSON 必须回带 ip_direct_address + cf_cert_mode,供前端 3 地址表单回显。
    let routing = store.access_routing_json().await.unwrap();
    let node = find_node_in_routing(&routing, node_id);
    assert_eq!(node["ip_direct_address"].as_str(), Some("192.0.2.70"));
    assert_eq!(node["cf_cert_mode"].as_str(), Some("reuse_direct"));
}

/// 测试辅助:在控制面路由 JSON 里按节点 ID 找出节点对象。
fn find_node_in_routing(routing: &serde_json::Value, node_id: uuid::Uuid) -> serde_json::Value {
    routing["access_nodes"]
        .as_array()
        .expect("access_nodes array present")
        .iter()
        .find(|node| node["id"].as_str() == Some(node_id.to_string().as_str()))
        .cloned()
        .expect("created node present in routing json")
}
