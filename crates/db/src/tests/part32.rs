/// 数据库测试分片 32。
// 本文件覆盖本机出口服务自动补齐协议材料的回归用例。
// 重点验证本机 VLESS Reality 不需要管理员手填 public_key。
// 后端必须在 DB 写入边界补齐 public_key、private_key 和 short_id。
// 测试只使用示例域名和测试 UUID，不包含真实服务器信息。
// PostgreSQL 用例仅在 DATABASE_URL 存在时运行。
// 新增本机出口协议默认值时优先补到本分片。
// 所有 helper 和导入由父级 tests/mod.rs 统一提供。
// 文件保持短小，避免影响源码长度门禁。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_local_vless_reality_auto_generates_keys_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local VLESS Reality auto key test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-vless-reality-auto-node".to_string(),
            public_host: "local-vless-reality-auto.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-local-vless-reality-auto-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-vless-reality-auto-resource".to_string(),
                    endpoint_name: "local-vless-reality-auto-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "local-vless-reality-auto.example.test".to_string(),
                    port: 39_143,
                    outbound_config: json!({
                        "uuid": "00000000-0000-4000-8000-000000000143",
                        "security": "reality",
                        "server_name": "local-vless-reality-auto.example.test"
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("local VLESS Reality should auto-fill key material");

    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .expect("created endpoint id"),
    )
    .unwrap();

    let raw_config = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT outbound_config FROM exit_endpoints WHERE id = $1",
    )
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let config = raw_config;

    assert_eq!(config["security"], "reality");
    assert_eq!(config["server_name"], "www.cloudflare.com");
    assert!(config["public_key"]
        .as_str()
        .is_some_and(|value| value.len() >= 32));
    assert!(config["private_key"]
        .as_str()
        .is_some_and(|value| value.len() >= 32));
    assert!(config["short_id"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
}

#[tokio::test]
async fn test_pg_local_vless_short_id_implies_reality_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local VLESS implicit Reality test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-vless-implicit-reality-node".to_string(),
            public_host: "local-vless-implicit-reality.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-local-vless-implicit-reality-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-vless-implicit-reality-resource".to_string(),
                    endpoint_name: "local-vless-implicit-reality-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "local-vless-implicit-reality.example.test".to_string(),
                    port: 39_144,
                    outbound_config: json!({
                        "uuid": "00000000-0000-4000-8000-000000000144",
                        "server_name": "local-vless-implicit-reality.example.test",
                        "short_id": "abc12345"
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("local VLESS short_id should imply Reality and auto-fill keys");

    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .expect("created endpoint id"),
    )
    .unwrap();

    let raw_config = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT outbound_config FROM exit_endpoints WHERE id = $1",
    )
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let config = raw_config;

    assert_eq!(config["security"], "reality");
    assert_eq!(config["short_id"], "abc12345");
    assert!(config["public_key"]
        .as_str()
        .is_some_and(|value| value.len() >= 32));
    assert!(config["private_key"]
        .as_str()
        .is_some_and(|value| value.len() >= 32));
}
