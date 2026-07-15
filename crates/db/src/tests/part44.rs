/// 数据库测试分片 44。
// 本文件覆盖入口绑定多个出口的 PostgreSQL 兼容性。
// 一个入口绑定多个出口时，运行绑定记录必须逐条生成。
// 分片通过父模块 include 聚合，共用测试 helper 和 imports。
// 测试只使用隔离 PostgreSQL，不访问远端节点。
// DATABASE_URL 缺失时只输出脱敏跳过原因。
// 断言不依赖真实服务器地址或私有凭据。
// 后续多出口绑定测试优先追加到本分片。
// 本文件不定义独立模块，避免重复导入测试上下文。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_access_entry_allows_two_exit_bindings_on_one_entry_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL multi-exit entry binding test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("entry-multi-exit-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.22".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("entry-multi-exit-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![
                    AdminLocalExitLineInput {
                        resource_name: "entry-multi-exit-resource-a".to_string(),
                        endpoint_name: "entry-multi-exit-endpoint-a".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.22".to_string(),
                        port: 38_221,
                        outbound_config: json!({"username": "entry-user-a", "password": "entry-pass-a"}),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    },
                    AdminLocalExitLineInput {
                        resource_name: "entry-multi-exit-resource-b".to_string(),
                        endpoint_name: "entry-multi-exit-endpoint-b".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.23".to_string(),
                        port: 38_222,
                        outbound_config: json!({"username": "entry-user-b", "password": "entry-pass-b"}),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    },
                ],
            },
        )
        .await
        .unwrap();
    let first_exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let second_exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][1]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "one-entry-two-exits".to_string(),
            listen_host: String::new(),
            listen_port: 44_388,
            protocol: "vless".to_string(),
            transport: "tcp".to_string(),
            security: "reality".to_string(),
            server_name: "entry-multi-exit.example.test".to_string(),
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
        })
        .await
        .unwrap();
    let first_binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id: first_exit_endpoint_id,
                name: "one-entry-exit-a".to_string(),
                enabled: true,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();
    let second_binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id: second_exit_endpoint_id,
                name: "one-entry-exit-b".to_string(),
                enabled: true,
                sort_weight: 110,
                remark: String::new(),
            },
        )
        .await
        .unwrap();

    let runtime_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM access_lines WHERE id IN ($1, $2)")
            .bind(first_binding_id)
            .bind(second_binding_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(runtime_count, 2);
}
