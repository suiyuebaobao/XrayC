/// 数据库测试分片 29。
// 本文件集中放置 VLESS Reality 线路绑定补充测试。
// 测试只使用示例域名、示例 key 文本和 RFC 文档地址。
// 这里验证绑定线路 API 会把 Reality 入站材料写入 access_lines。
// public key 和 short id 供订阅输出，private key 供 agent 渲染 Xray。
// 入口/出口配置以明文存储,直接断言列值内容。
// 本文件不访问远端服务器，不写真实代理地址或私有凭据。
// 新增入口安全模式时优先补充本分片，避免 HY2 分片继续膨胀。
// 所有 helper 和导入由父级 tests/mod.rs 统一提供。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_line_entries_persist_vless_reality_inbound_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL VLESS Reality entry test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "binding-reality-node".to_string(),
                public_host: "binding-reality.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-binding-reality-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "binding-reality-resource".to_string(),
                        endpoint_name: "binding-reality-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.41".to_string(),
                        port: 39_041,
                        outbound_config: json!({
                            "username": "reality-entry-user",
                            "password": "reality-entry-pass",
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
        store
            .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
            .await
            .unwrap();

        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "reality-entry-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "binding-reality.example.test".to_string(),
                    listen_port: 41_210,
                    protocol: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    inbound_config: json!({
                        "security": "reality",
                        "server_name": "binding-reality.example.test",
                        "public_key": "entry-reality-public-key",
                        "private_key": "entry-reality-private-key",
                        "short_id": "beef1234",
                        "dest": "binding-reality.example.test:443"
                    }),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .unwrap();

        let row = sqlx::query_as::<_, (String, String, bool, String, serde_json::Value)>(
            r#"
            SELECT public_key, short_id, udp_enabled, udp_packet_encoding, inbound_config
            FROM access_lines
            WHERE id = $1
            "#,
        )
        .bind(result.created_line_ids[0])
        .fetch_one(store.pool())
        .await
        .unwrap();
        let opened_inbound = row.4;

        assert_eq!(row.0, "entry-reality-public-key");
        assert_eq!(row.1, "beef1234");
        assert!(!row.2);
        assert_eq!(row.3, "");
        assert_eq!(opened_inbound["private_key"], "entry-reality-private-key");
    }

    #[tokio::test]
    async fn test_pg_line_entries_auto_generate_vless_reality_keys_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL VLESS Reality auto key entry test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "auto-reality-entry-node".to_string(),
                public_host: "auto-reality-entry.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-auto-reality-entry-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "auto-reality-entry-resource".to_string(),
                        endpoint_name: "auto-reality-entry-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.45".to_string(),
                        port: 39_045,
                        outbound_config: json!({
                            "username": "auto-reality-entry-user",
                            "password": "auto-reality-entry-pass",
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

        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "auto-reality-entry-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "auto-reality-entry.example.test".to_string(),
                    listen_port: 41_212,
                    protocol: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    inbound_config: json!({
                        "security": "reality",
                        "server_name": "auto-reality-entry.example.test",
                        "dest": "auto-reality-entry.example.test:443",
                        "fingerprint": "chrome"
                    }),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .expect("VLESS Reality entry binding should auto-fill key material");

        let row = sqlx::query_as::<_, (String, String, serde_json::Value)>(
            r#"
            SELECT public_key, short_id, inbound_config
            FROM access_lines
            WHERE id = $1
            "#,
        )
        .bind(result.created_line_ids[0])
        .fetch_one(store.pool())
        .await
        .unwrap();
        let opened_inbound = row.2;

        assert!(!row.0.trim().is_empty(), "public key should be generated");
        assert!(!row.1.trim().is_empty(), "short id should be generated");
        assert!(!opened_inbound["private_key"].as_str().unwrap_or("").trim().is_empty());
        assert_eq!(opened_inbound["public_key"], row.0);
        assert_eq!(opened_inbound["short_id"], row.1);
    }

    #[tokio::test]
    async fn test_pg_line_entries_default_reality_sni_uses_dest_host_for_ip_listen_host_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL VLESS Reality SNI default test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "ip-reality-entry-node".to_string(),
                public_host: "203.0.113.44".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-ip-reality-entry-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "ip-reality-entry-resource".to_string(),
                        endpoint_name: "ip-reality-entry-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.46".to_string(),
                        port: 39_046,
                        outbound_config: json!({
                            "username": "ip-reality-entry-user",
                            "password": "ip-reality-entry-pass",
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

        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "ip-reality-entry-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "203.0.113.44".to_string(),
                    listen_port: 41_213,
                    protocol: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    inbound_config: json!({
                        "security": "reality",
                        "dest": "www.cloudflare.com:443",
                        "fingerprint": "chrome"
                    }),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .expect("VLESS Reality entry should default SNI from dest host");

        let row = sqlx::query_as::<_, (String, serde_json::Value)>(
            r#"
            SELECT server_name, inbound_config
            FROM access_lines
            WHERE id = $1
            "#,
        )
        .bind(result.created_line_ids[0])
        .fetch_one(store.pool())
        .await
        .unwrap();
        let opened_inbound = row.1;

        assert_eq!(row.0, "www.cloudflare.com");
        assert_eq!(opened_inbound["server_name"], "www.cloudflare.com");
        assert_eq!(opened_inbound["dest"], "www.cloudflare.com:443");
    }

    #[tokio::test]
    async fn test_pg_line_entries_reject_vless_reality_xudp_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL VLESS Reality XUDP reject test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "reject-reality-xudp-node".to_string(),
                public_host: "reject-reality-xudp.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-reject-reality-xudp-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "reject-reality-xudp-resource".to_string(),
                        endpoint_name: "reject-reality-xudp-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.42".to_string(),
                        port: 39_042,
                        outbound_config: json!({
                            "username": "reject-reality-user",
                            "password": "reject-reality-pass",
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
        store
            .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
            .await
            .unwrap();

        let err = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "reject-reality-xudp-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "reject-reality-xudp.example.test".to_string(),
                    listen_port: 41_211,
                    protocol: "vless".to_string(),
                    network_mode: "xudp".to_string(),
                    inbound_config: json!({
                        "security": "reality",
                        "server_name": "reject-reality-xudp.example.test",
                        "public_key": "entry-reality-public-key",
                        "private_key": "entry-reality-private-key",
                        "short_id": "cafe1234",
                        "dest": "reject-reality-xudp.example.test:443"
                    }),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("Reality"));
    }

    #[tokio::test]
    async fn test_pg_local_exit_rejects_vless_reality_xudp_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL local VLESS Reality XUDP reject test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "local-reject-reality-node".to_string(),
                public_host: "local-reject-reality.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-local-reject-reality-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let err = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "local-reject-reality-resource".to_string(),
                        endpoint_name: "local-reject-reality-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "vless".to_string(),
                        network_mode: "xudp".to_string(),
                        host: "local-reject-reality.example.test".to_string(),
                        port: 39_043,
                        outbound_config: json!({
                            "uuid": "00000000-0000-4000-8000-000000000043",
                            "security": "reality",
                            "server_name": "local-reject-reality.example.test",
                            "public_key": "local-reality-public-key",
                            "short_id": "abcd1234"
                        }),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    }],
                },
            )
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("Reality"));
    }

    #[tokio::test]
    async fn test_pg_local_exit_vless_reality_replaces_ip_sni_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL local VLESS Reality IP SNI test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "local-ip-reality-node".to_string(),
                public_host: "203.0.113.50".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-local-ip-reality-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "local-ip-reality-resource".to_string(),
                        endpoint_name: "local-ip-reality-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "vless".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "203.0.113.50".to_string(),
                        port: 39_050,
                        outbound_config: json!({
                            "uuid": "00000000-0000-4000-8000-000000000050",
                            "security": "reality",
                            "server_name": "203.0.113.50",
                            "dest": "203.0.113.50:443"
                        }),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    }],
                },
            )
            .await
            .expect("local VLESS Reality should replace endpoint IP SNI");
        let exit_endpoint_id = uuid::Uuid::parse_str(
            created["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let row = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT outbound_config FROM exit_endpoints WHERE id = $1",
        )
        .bind(exit_endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let opened = row;

        assert_eq!(opened["server_name"], "www.cloudflare.com");
        assert_eq!(opened["dest"], "www.cloudflare.com:443");
        assert!(!opened["public_key"].as_str().unwrap_or("").is_empty());
        assert!(!opened["private_key"].as_str().unwrap_or("").is_empty());
    }
