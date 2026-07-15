// 数据库测试拆分片段四十。
// 本文件从 part29 拆出本机出口和分组授权测试。
// 使用 include 聚合，测试函数名保持不变。
// 拆分只为满足源码长度门禁。
// 不改变 PostgreSQL 初始化和断言逻辑。
// 新增相关测试时优先保持单文件低于上限。
// 测试仍按 DATABASE_URL 存在与否决定是否执行。
// 输出只包含脱敏跳过原因。
// 本文件不定义独立模块层级。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_local_exit_rejects_hy2_without_tls_files_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL local HY2 TLS file reject test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "local-hy2-no-cert-node".to_string(),
                public_host: "local-hy2-no-cert.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-local-hy2-no-cert-token".to_string(),
                // 节点本身配了域名直连地址,先过节点证书护栏;本测试聚焦「出口配置缺证书文件」这一层,
                // 此处的 no-cert 指 outbound_config 没带证书文件,而非节点没有 cert_domain。
                cert_domain: Some("local-hy2-no-cert.example.test".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        let err = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "local-hy2-no-cert-resource".to_string(),
                        endpoint_name: "local-hy2-no-cert-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "hysteria".to_string(),
                        network_mode: "udp".to_string(),
                        host: "local-hy2-no-cert.example.test".to_string(),
                        port: 39_051,
                        outbound_config: json!({
                            "password": "local-hy2-password"
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

        assert!(format!("{err:?}").contains("HY2 本机出口需要配置 TLS 证书文件和私钥文件"));
    }

    #[tokio::test]
    async fn test_pg_local_exit_hy2_includes_tls_files_in_heartbeat_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL local HY2 TLS heartbeat test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "local-hy2-cert-node".to_string(),
                public_host: "local-hy2-cert.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-local-hy2-cert-token".to_string(),
                // HY2 是要证书协议,节点必须配域名直连地址才能过出口侧护栏。
                cert_domain: Some("local-hy2-cert.example.test".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "local-hy2-cert-resource".to_string(),
                        endpoint_name: "local-hy2-cert-endpoint".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "hysteria".to_string(),
                        network_mode: "udp".to_string(),
                        host: "local-hy2-cert.example.test".to_string(),
                        port: 39_052,
                        outbound_config: json!({
                            "password": "local-hy2-password",
                            "server_name": "local-hy2-cert.example.test",
                            "certificate_file": "/etc/letsencrypt/live/local-hy2-cert.example.test/fullchain.pem",
                            "key_file": "/etc/letsencrypt/live/local-hy2-cert.example.test/privkey.pem"
                        }),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                                            node_domain_id: None,
                    }],
                },
            )
            .await
            .expect("local HY2 line with certificate files should be created");

        let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        let service = &heartbeat["config"]["local_exit_services"][0];

        assert_eq!(service["protocol"], "hysteria2");
        assert_eq!(
            service["tls_certificate_file"],
            "/etc/letsencrypt/live/local-hy2-cert.example.test/fullchain.pem"
        );
        assert_eq!(
            service["tls_key_file"],
            "/etc/letsencrypt/live/local-hy2-cert.example.test/privkey.pem"
        );
    }

    #[tokio::test]
    async fn test_pg_line_group_members_are_exit_endpoints_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL line group endpoint member test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let created = store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "group-endpoint-resource".to_string(),
                        endpoint_name: "group-endpoint-line".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.44".to_string(),
                        port: 39_044,
                        outbound_config: json!({
                            "username": "group-endpoint-user",
                            "password": "group-endpoint-pass",
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

        let replaced = store
            .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
            .await
            .unwrap();
        assert_eq!(replaced, 1);

        let endpoint_member_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM line_group_exit_endpoints
            WHERE line_group_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(group_id)
        .bind(exit_endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(endpoint_member_count, 1);

        let old_access_line_member_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM line_group_lines WHERE line_group_id = $1",
        )
        .bind(group_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(old_access_line_member_count, 0);
    }

    #[tokio::test]
    async fn test_pg_subscription_exports_all_entries_bound_to_authorized_group_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL multi group entry subscription test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let data = store.load_store_data().await.unwrap();
        let exit_endpoint_id = data
            .exit_pools
            .values()
            .find_map(|pool| pool.members.first().map(|member| member.id))
            .unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "multi-entry-node".to_string(),
                public_host: "multi-entry.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-multi-entry-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("多入口线路-{}", uuid::Uuid::new_v4().simple()),
                country_code: "HK".to_string(),
                icon: "🇭🇰".to_string(),
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
            .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
            .await
            .unwrap();
        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![
                    AdminAccessNodeGroupEntryInput {
                        name: "multi-entry-a".to_string(),
                        exit_endpoint_id,
                        listen_host: "multi-entry.example.test".to_string(),
                        listen_port: 42_120,
                        protocol: "vless".to_string(),
                        network_mode: "tcp".to_string(),
                        inbound_config: json!({}),
                        xhttp_mode: "auto".to_string(),
                        enabled: true,
                    },
                    AdminAccessNodeGroupEntryInput {
                        name: "multi-entry-b".to_string(),
                        exit_endpoint_id,
                        listen_host: "multi-entry.example.test".to_string(),
                        listen_port: 42_121,
                        protocol: "vless".to_string(),
                        network_mode: "tcp".to_string(),
                        inbound_config: json!({}),
                        xhttp_mode: "auto".to_string(),
                        enabled: true,
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(result.created_line_ids.len(), 2);

        let demo_user_id = uuid("00000000-0000-0000-0000-000000000001");
        let demo_plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let seed_line_id = uuid("00000000-0000-0000-0000-000000000501");
        store
            .replace_admin_plan_line_groups(
                demo_plan_id,
                vec![AdminPlanLineGroupInput {
                    line_group_id: group_id,
                    billing_multiplier: None,
                }],
            )
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
            .bind(demo_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE access_lines SET enabled = FALSE WHERE id = $1")
            .bind(seed_line_id)
            .execute(store.pool())
            .await
            .unwrap();

        let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
        let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let ports = profile["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy["port"].as_i64())
            .collect::<Vec<_>>();
        assert_eq!(ports, vec![42_120, 42_121]);

        let subscription_json = store
            .user_subscription_json_for_user(demo_user_id)
            .await
            .unwrap();
        let visible_ports = subscription_json["access_lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|line| line["listen_port"].as_i64())
            .collect::<Vec<_>>();
        assert_eq!(visible_ports, vec![42_120, 42_121]);
    }

    #[tokio::test]
    async fn test_pg_clearing_group_lines_keeps_binding_node_authorization_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL clearing group runtime auth test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");

        store
            .replace_admin_plan_line_groups(
                plan_id,
                vec![AdminPlanLineGroupInput {
                    line_group_id: group_id,
                    billing_multiplier: None,
                }],
            )
            .await
            .unwrap();

        let active_heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        assert_eq!(active_heartbeat["authorized_users"].as_array().unwrap().len(), 1);
        assert!(
            active_heartbeat["config"]["access_lines"]
                .as_array()
                .is_some_and(|lines| !lines.is_empty())
        );

        store
            .replace_admin_line_group_lines(group_id, Vec::new())
            .await
            .unwrap();
        let cleared_heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();

        // 口径修正（随 per-binding 出口池 ordering BUG 修复同步）：
        // 「分组绑定线路」(replace_admin_line_group_lines) 按出口端点集合维护成员，清空端点集合只删
        // line_group_exit_endpoints，**保留**通过主路径「分组绑定节点」(line_group_binding_nodes) 持有的
        // binding（见 part42 对 binding_nodes 保留的契约）。该 binding 自己的 per-binding 出口成员不再被
        // 共享池剪枝误删，故其运行态 access_line 仍可用、用户仍经主路径分组成员被授权。
        // 旧断言期望「清空即撤权」其实是建立在「剪枝误删 binding 出口让线路失效」这一被修复掉的 BUG 上，
        // 与 part42 的 binding_nodes 保留契约互相矛盾，这里更正为新口径下的真实行为。
        assert_eq!(
            cleared_heartbeat["authorized_users"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "清空分组绑定线路后，主路径 binding_nodes 持有的 binding 仍应授权"
        );
        let retained_binding_nodes = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM line_group_binding_nodes WHERE line_group_id = $1",
        )
        .bind(group_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            retained_binding_nodes, 1,
            "清空线路端点集合不得删主路径 binding_nodes（与 part42 契约一致）"
        );
        let line_healthy_member = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM exit_pool_members m
            JOIN access_lines l
              ON l.exit_pool_id = m.exit_pool_id
             AND l.exit_endpoint_id = m.exit_endpoint_id
            WHERE l.id = $1
              AND m.status = 'healthy'
            "#,
        )
        .bind(uuid("00000000-0000-0000-0000-000000000501"))
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            line_healthy_member, 1,
            "共享池剪枝不得删掉该 binding 自己的出口成员（ordering BUG 已修）"
        );
    }
