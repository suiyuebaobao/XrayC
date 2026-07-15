// 数据库测试拆分片段三十九。
// 本文件从 part24 拆出部署监听端口测试。
// 使用 include 聚合，测试函数名保持不变。
// 拆分只为满足源码长度门禁。
// 不改变 PostgreSQL 初始化和断言逻辑。
// 新增相关测试时优先保持单文件低于上限。
// 测试仍按 DATABASE_URL 存在与否决定是否执行。
// 输出只包含脱敏跳过原因。
// 本文件不定义独立模块层级。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_deploy_listen_ports_excludes_hy2_udp_entry_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL deploy listen port test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "deploy-listen-owner".to_string(),
                public_host: "deploy-listen.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-deploy-listen-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "deploy-listen-resource".to_string(),
                        endpoint_name: "deploy-listen-endpoint".to_string(),
                        region_code: "TEST".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "deploy-listen.example.test".to_string(),
                        port: 35_099,
                        outbound_config: json!({
                            "username": "local-user",
                            "password": "local-pass",
                        }),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    }],
                },
            )
            .await
            .unwrap();
        let exit_endpoint_id = uuid::Uuid::parse_str(
            created["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let line_group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("deploy-listen-group-{}", uuid::Uuid::new_v4().simple()),
                country_code: "TEST".to_string(),
                icon: "🌐".to_string(),
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
                vec![
                    AdminAccessNodeGroupEntryInput {
                        name: "deploy-listen-vless".to_string(),
                        exit_endpoint_id,
                        listen_host: "deploy-listen.example.test".to_string(),
                        listen_port: 35_101,
                        protocol: "vless".to_string(),
                        network_mode: "tcp".to_string(),
                        inbound_config: json!({}),
                        xhttp_mode: "auto".to_string(),
                        enabled: true,
                    },
                    AdminAccessNodeGroupEntryInput {
                        name: "deploy-listen-hy2".to_string(),
                        exit_endpoint_id,
                        listen_host: "deploy-listen.example.test".to_string(),
                        listen_port: 35_102,
                        protocol: "hy2".to_string(),
                        network_mode: "udp".to_string(),
                        inbound_config: json!({}),
                        xhttp_mode: "auto".to_string(),
                        enabled: true,
                    },
                ],
            )
            .await
            .unwrap();

        let ports = store.access_node_enabled_listen_ports(node_id).await.unwrap();

        assert_eq!(ports, vec![35_101]);
    }
