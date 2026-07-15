/// 数据库测试分片 34。
// 本文件覆盖中转节点新增表单的端口和备注字段。
// 端口用于中转节点默认入口端口，备注用于管理员内部记录。
// 鉴权码仍只保存 hash，不在读模型中回显。
// PostgreSQL 用例仅在 DATABASE_URL 存在时运行。
// 测试使用示例域名，不包含真实服务器资产。
// 新增中转节点基础字段时优先补到本分片。
// helper 和导入由 tests/mod.rs 统一提供。
// 文件保持短小，避免影响源码长度门禁。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_access_node_keeps_public_port_and_remark_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL access node port remark test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "relay-node-with-port-remark".to_string(),
            public_host: "relay-port-remark.example.test".to_string(),
            public_port: 24_443,
            agent_token: "test-relay-node-auth-code".to_string(),
            remark: "备用香港入口".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let stored = sqlx::query_as::<_, (String, i32, String)>(
        "SELECT public_host, public_port, remark FROM access_nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(stored.0, "relay-port-remark.example.test");
    assert_eq!(stored.1, 24_443);
    assert_eq!(stored.2, "备用香港入口");

    let control_plane = store.access_routing_json().await.unwrap();
    let nodes = control_plane["access_nodes"].as_array().unwrap();
    let node = nodes
        .iter()
        .find(|item| item["id"].as_str() == Some(&node_id.to_string()))
        .unwrap();
    assert_eq!(node["public_port"], 24_443);
    assert_eq!(node["remark"], "备用香港入口");
    assert!(node.get("agent_token").is_none());
}
