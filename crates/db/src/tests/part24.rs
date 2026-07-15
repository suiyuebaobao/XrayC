/// 数据库测试分片 24。
// 本文件集中放置 HY2 用户入站相关数据库映射测试。
// 测试只使用内存模型，不连接真实数据库或远端服务器。
// 这里验证访问线路协议能映射到 Xray HY2 入站类型。
// TLS 证书路径使用示例域名，不包含真实部署信息。
// 新增 HY2 校验时优先补到本分片，避免 part01 继续膨胀。
// 所有 helper 和导入由父级 tests/mod.rs 统一提供。
// 本文件不得写入测试账号、服务器密码或公网 IP。
// 保持单文件低于五百行，便于后续拆分维护。
// 本头部满足前十行中文注释约束。

    #[test]
    fn test_access_protocol_mapping_supports_hysteria2_tls() {
        let store = MemoryStore::seeded();
        let mut line = store.read(|data| data.access_lines.values().next().unwrap().clone());
        line.protocol = "hy2".to_string();
        line.transport = "hysteria".to_string();
        line.server_name = "hy2.example.test".to_string();
        line.inbound_config = json!({
            "security": "tls",
            "certificate_file": "/etc/letsencrypt/live/hy2.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/hy2.example.test/privkey.pem"
        });

        let protocol = access_protocol_for_line(&line).expect("hy2 should map");

        assert!(matches!(protocol, XrayAccessProtocol::Hysteria2));
    }

    #[test]
    fn test_access_protocol_rejects_hysteria_transport_for_other_protocols() {
        assert!(validate_access_protocol_transport("hysteria", "hysteria").is_ok());
        let err = validate_access_protocol_transport("trojan", "hysteria")
            .expect_err("trojan must not keep stale hy2 transport");
        assert!(err.to_string().contains("HY2 传输只能用于 HY2"));
    }

    #[test]
    fn test_hong_kong_region_rejects_hy2_protocol() {
        let err = validate_hong_kong_hy2("hy2", "HK")
            .expect_err("hong kong lines must not use hy2");
        assert!(err.to_string().contains("香港线路不支持 HY2"));
        assert!(validate_hong_kong_hy2("vless", "HK").is_ok());
        assert!(validate_hong_kong_hy2("hysteria", "US").is_ok());
    }

    #[tokio::test]
    async fn test_pg_local_exit_hy2_access_line_gets_tls_config_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL local HY2 line test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "local-hy2-owner".to_string(),
                public_host: "hy2.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-hy2-owner-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "local-hy2-resource".to_string(),
                        endpoint_name: "local-hy2-endpoint".to_string(),
                        region_code: "LOCAL".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "hy2.example.test".to_string(),
                        port: 34_581,
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
            .expect("local HY2 upstream line should be created");
        let exit_endpoint_id = uuid::Uuid::parse_str(
            created["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let line_group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("local-hy2-group-{}", uuid::Uuid::new_v4().simple()),
                country_code: "LOCAL".to_string(),
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
        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "local-hy2-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "hy2.example.test".to_string(),
                    listen_port: 34_580,
                    protocol: "hy2".to_string(),
                    network_mode: "udp".to_string(),
                    inbound_config: json!({}),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .expect("HY2 entry should be explicitly bound to the line");
        let access_line_id = result
            .created_line_ids
            .first()
            .copied()
            .expect("created HY2 access line id");
        let row = sqlx::query_as::<_, (String, String, serde_json::Value)>(
            "SELECT protocol, server_name, inbound_config FROM access_lines WHERE id = $1",
        )
        .bind(access_line_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert_eq!(row.0, "hysteria");
        assert_eq!(row.1, "hy2.example.test");
        assert_eq!(row.2["security"], "tls");
        assert!(row.2["certificate_file"]
            .as_str()
            .is_some_and(|value| value.contains("hy2.example.test")));
    }

    #[tokio::test]
    async fn test_pg_line_entries_do_not_auto_join_line_group_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL line entry group test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "default-group-single-line-node".to_string(),
                public_host: "default-group-single-line.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-default-group-single-line-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut endpoint_ids = Vec::new();
        for idx in 0..2 {
            let created = store
                .create_admin_local_exit_lines(
                    node_id,
                    AdminLocalExitLinesInput {
                        lines: vec![AdminLocalExitLineInput {
                            resource_name: format!("single-default-resource-{idx}"),
                            endpoint_name: format!("single-default-endpoint-{idx}"),
                            region_code: "US".to_string(),
                            outbound_type: "socks".to_string(),
                            network_mode: "tcp".to_string(),
                            host: format!("198.51.100.{}", idx + 1),
                            port: 39_000 + idx,
                            outbound_config: json!({
                                "username": format!("single-user-{idx}"),
                                "password": format!("single-pass-{idx}"),
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
            endpoint_ids.push(
                uuid::Uuid::parse_str(created["created_lines"][0]["exit_endpoint_id"].as_str().unwrap())
                    .unwrap(),
            );
        }

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
            .replace_admin_line_group_lines(line_group_id, endpoint_ids.clone())
            .await
            .unwrap();

        let entries = endpoint_ids
            .iter()
            .enumerate()
            .map(|(idx, exit_endpoint_id)| AdminAccessNodeGroupEntryInput {
                name: format!("single-default-line-{idx}"),
                exit_endpoint_id: *exit_endpoint_id,
                listen_host: "203.0.113.10".to_string(),
                listen_port: 41_100 + idx as u16,
                protocol: "vless".to_string(),
                network_mode: "tcp".to_string(),
                inbound_config: json!({}),
                xhttp_mode: "auto".to_string(),
                enabled: true,
            })
            .collect::<Vec<_>>();
        let result = store
            .create_admin_access_node_group_entries(node_id, entries)
            .await
            .unwrap();

        let (auto_group_count, legacy_group_snapshot_count, enabled_line_count) =
            sqlx::query_as::<_, (i64, i64, i64)>(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM line_group_lines WHERE access_line_id = ANY($1)),
                    (SELECT COUNT(*) FROM access_lines WHERE id = ANY($1) AND line_group_id IS NOT NULL),
                    (SELECT COUNT(*) FROM access_lines WHERE access_node_id = $2 AND enabled = TRUE)
                "#,
            )
        .bind(&result.created_line_ids)
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert_eq!(enabled_line_count, 2);
        assert_eq!(auto_group_count, 0);
        assert_eq!(legacy_group_snapshot_count, 0);
    }

    #[tokio::test]
    async fn test_pg_line_entries_create_selected_network_modes_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL binding network mode test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "binding-network-mode-node".to_string(),
                public_host: "binding-network-mode.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-binding-network-mode-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "binding-network-mode-resource".to_string(),
                        endpoint_name: "binding-network-mode-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.40".to_string(),
                        port: 39_040,
                        outbound_config: json!({
                            "username": "network-mode-user",
                            "password": "network-mode-pass",
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

        let entries = [
            ("tcp-line", "tcp", 41_200),
            ("xhttp-line", "xhttp", 41_201),
            ("xudp-line", "xudp", 41_202),
        ]
        .into_iter()
        .map(|(name, network_mode, listen_port)| AdminAccessNodeGroupEntryInput {
            name: name.to_string(),
            exit_endpoint_id,
            listen_host: "binding-network-mode.example.test".to_string(),
            listen_port,
            protocol: "vless".to_string(),
            network_mode: network_mode.to_string(),
            inbound_config: json!({}),
            xhttp_mode: "stream-one".to_string(),
            enabled: true,
        })
        .collect::<Vec<_>>();
        let result = store
            .create_admin_access_node_group_entries(node_id, entries)
            .await
            .unwrap();

        let modes = sqlx::query_as::<_, (String, bool, String, i32)>(
            r#"
            SELECT transport, udp_enabled, udp_packet_encoding, listen_port
            FROM access_lines
            WHERE access_node_id = $1
              AND enabled = TRUE
            ORDER BY listen_port
            "#,
        )
        .bind(node_id)
        .fetch_all(store.pool())
        .await
        .unwrap();

        assert_eq!(result.created_line_ids.len(), 3);
        assert_eq!(
            modes,
            vec![
                ("tcp".to_string(), false, String::new(), 41_200),
                ("xhttp".to_string(), false, String::new(), 41_201),
                ("tcp".to_string(), true, "xudp".to_string(), 41_202),
            ]
        );
    }

    #[tokio::test]
    async fn test_pg_flat_line_group_without_parent_can_be_bound_to_plan_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL flat line group test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let line_group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("GPT 分组-{}", uuid::Uuid::new_v4().simple()),
                country_code: "GLOBAL".to_string(),
                icon: "🌐".to_string(),
                group_level: None,
                parent_group_id: None,
                sort_weight: Some(120),
                billing_multiplier: None,
                enabled: Some(true),
                dedicated_rules: None,
                rule_set_bindings: None,
            })
            .await
            .expect("flat line group should be created without parent");
        let plan_id = sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM plans WHERE is_default = TRUE AND is_deleted = FALSE LIMIT 1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();

        store
            .replace_admin_plan_line_groups(
                plan_id,
                vec![AdminPlanLineGroupInput {
                    line_group_id,
                    billing_multiplier: None,
                }],
            )
            .await
            .expect("plan should authorize flat line group directly");

        let stored_parent = sqlx::query_scalar::<_, Option<uuid::Uuid>>(
            "SELECT parent_group_id FROM line_groups WHERE id = $1",
        )
        .bind(line_group_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let binding_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM plan_line_groups WHERE plan_id = $1 AND line_group_id = $2",
        )
        .bind(plan_id)
        .bind(line_group_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert!(stored_parent.is_none());
        assert_eq!(binding_count, 1);
    }
