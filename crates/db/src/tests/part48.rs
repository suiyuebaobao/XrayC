// 数据库测试分片 48。
// 本文件覆盖同节点入口 listen_port 冲突校验。
// Xray 同端口无法多协议入站,同节点同端口入口会让 reload 失败回滚。
// 因此入口创建/更新写入前必须校验:同节点 enabled 入口端口唯一。
// 冲突判定与协议无关,只看 access_node_id + listen_port + enabled。
// 测试只用 RFC 文档地址、示例域名和测试 UUID,不涉及真实资产。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
// 父级 tests 模块提供 PgStore、输入类型、Uuid 与 pg_test_guard。
// 读回与报错断言以写入侧落库为唯一真相,符合跨层对齐红线。
// 本头部满足前十行中文注释约束。

/// 构造一个纯直连 Reality 入口输入。
/// 用 vless/tcp/reality 组合,免证书,便于聚焦端口冲突校验本身。
fn reality_entry_input(
    node_id: Uuid,
    name: &str,
    listen_port: u16,
    enabled: bool,
) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: String::new(),
        listen_port,
        protocol: "vless".to_string(),
        transport: "tcp".to_string(),
        security: "reality".to_string(),
        server_name: "port-conflict.example.test".to_string(),
        ws_path: String::new(),
        ws_host: String::new(),
        cdn_enabled: false,
        cdn_provider: String::new(),
        cdn_hostname: String::new(),
        cdn_server: String::new(),
        enabled,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

/// 新建一个最小可用的中转节点,返回其 id。
async fn create_port_conflict_node(store: &PgStore, label: &str) -> Uuid {
    store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("port-conflict-{label}-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.48".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("port-conflict-{label}-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn test_create_entry_rejects_duplicate_port_on_same_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL duplicate listen_port reject test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = create_port_conflict_node(&store, "dup").await;
    // 先建一个 enabled 的 8443 入口,占用端口。
    store
        .create_admin_access_entry(reality_entry_input(node_id, "dup-first", 8443, true))
        .await
        .unwrap();

    // 同节点再建 8443 入口应当被拒绝。
    let err = store
        .create_admin_access_entry(reality_entry_input(node_id, "dup-second", 8443, true))
        .await
        .expect_err("same-node duplicate listen_port should be rejected");
    let message = err.to_string();
    assert!(
        message.contains("端口已被占用"),
        "error should mention 端口已被占用, got: {message}"
    );
    assert!(
        message.contains("8443"),
        "error should mention the conflicting port, got: {message}"
    );
}

#[tokio::test]
async fn test_create_entry_allows_same_port_different_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL same-port-different-node allow test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_a = create_port_conflict_node(&store, "node-a").await;
    let node_b = create_port_conflict_node(&store, "node-b").await;
    // 两个不同节点用同一端口 8443,互不冲突,都应成功。
    store
        .create_admin_access_entry(reality_entry_input(node_a, "diff-node-a", 8443, true))
        .await
        .unwrap();
    store
        .create_admin_access_entry(reality_entry_input(node_b, "diff-node-b", 8443, true))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_create_entry_allows_port_of_disabled_entry_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL disabled-entry port allow test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = create_port_conflict_node(&store, "disabled").await;
    // 占用方 enabled=false,不计入端口占用。
    store
        .create_admin_access_entry(reality_entry_input(node_id, "disabled-holder", 8443, false))
        .await
        .unwrap();
    // 因此同端口的 enabled 入口仍可创建成功。
    store
        .create_admin_access_entry(reality_entry_input(node_id, "enabled-claimant", 8443, true))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_update_entry_port_conflict_excludes_self_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL update port conflict test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = create_port_conflict_node(&store, "update").await;
    // 节点上有两个 enabled 入口:8443 与 9443。
    let entry_8443 = store
        .create_admin_access_entry(reality_entry_input(node_id, "update-8443", 8443, true))
        .await
        .unwrap();
    store
        .create_admin_access_entry(reality_entry_input(node_id, "update-9443", 9443, true))
        .await
        .unwrap();

    // 更新自身入口为同样的 8443(端口未变),不应报冲突。
    store
        .update_admin_access_entry(
            entry_8443,
            reality_entry_input(node_id, "update-8443", 8443, true),
        )
        .await
        .unwrap();

    // 把 8443 入口改成 9443(已被另一个 enabled 入口占用)→ 冲突。
    let err = store
        .update_admin_access_entry(
            entry_8443,
            reality_entry_input(node_id, "update-8443", 9443, true),
        )
        .await
        .expect_err("updating to a port held by another enabled entry should be rejected");
    let message = err.to_string();
    assert!(
        message.contains("端口已被占用"),
        "error should mention 端口已被占用, got: {message}"
    );
    assert!(
        message.contains("9443"),
        "error should mention the conflicting port, got: {message}"
    );
}
