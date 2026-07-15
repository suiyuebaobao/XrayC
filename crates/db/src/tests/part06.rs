// 数据库测试分片 06。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    async fn register_test_binding_node_for_access_line(
        store: &PgStore,
        line_id: Uuid,
        node_id: Uuid,
        exit_endpoint_id: Uuid,
        exit_pool_id: Uuid,
        group_id: Uuid,
    ) {
        sqlx::query(
            r#"
            INSERT INTO access_entries (
                id, access_node_id, name, listen_host, listen_port, protocol, transport,
                security, enabled, sort_weight
            )
            SELECT id, access_node_id, name, listen_host, listen_port, protocol, transport,
                   COALESCE(NULLIF(inbound_config->>'security', ''), ''), enabled, visibility_weight
            FROM access_lines
            WHERE id = $1
            ON CONFLICT (id) DO UPDATE SET
                access_node_id = EXCLUDED.access_node_id,
                name = EXCLUDED.name,
                listen_host = EXCLUDED.listen_host,
                listen_port = EXCLUDED.listen_port,
                protocol = EXCLUDED.protocol,
                transport = EXCLUDED.transport,
                enabled = EXCLUDED.enabled,
                sort_weight = EXCLUDED.sort_weight,
                updated_at = now()
            "#,
        )
        .bind(line_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_entry_exit_bindings (
                id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight
            )
            SELECT id, id, $2, $3, name, enabled, visibility_weight
            FROM access_lines
            WHERE id = $1
            ON CONFLICT (id) DO UPDATE SET
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                exit_pool_id = EXCLUDED.exit_pool_id,
                name = EXCLUDED.name,
                enabled = EXCLUDED.enabled,
                sort_weight = EXCLUDED.sort_weight,
                updated_at = now()
            "#,
        )
        .bind(line_id)
        .bind(exit_endpoint_id)
        .bind(exit_pool_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO line_group_binding_nodes (line_group_id, entry_exit_binding_id, position)
            VALUES ($1, $2, 100)
            ON CONFLICT (line_group_id, entry_exit_binding_id) DO NOTHING
            "#,
        )
        .bind(group_id)
        .bind(line_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE access_lines
            SET access_node_id = $2,
                exit_endpoint_id = COALESCE(exit_endpoint_id, $3),
                exit_pool_id = $4
            WHERE id = $1
            "#,
        )
        .bind(line_id)
        .bind(node_id)
        .bind(exit_endpoint_id)
        .bind(exit_pool_id)
        .execute(store.pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_pg_subscription_syncs_all_flat_group_entries_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL visible line assignment test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let first_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let second_line_id = uuid("00000000-0000-0000-0000-000000000502");

        sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
            .bind(user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM plan_line_groups WHERE plan_id = $1")
            .bind(plan_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO plan_line_groups (plan_id, line_group_id)
            VALUES ($1, $2)
            ON CONFLICT (plan_id, line_group_id) DO NOTHING
            "#,
        )
        .bind(plan_id)
        .bind(group_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_lines (
                id, name, access_node_id, line_group_id, exit_pool_id, listen_host, listen_port,
                protocol, transport, user_uuid, server_name, public_key, short_id,
                enabled, visibility_weight
            )
            VALUES (
                $1, '日本 02', $2, $3, $4, 'access.example.test', 8443,
                'vless', 'tcp', $5, 'www.cloudflare.com', 'seeded-public-key-2',
                'b1c2d3e4', TRUE, 200
            )
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                line_group_id = EXCLUDED.line_group_id,
                exit_pool_id = EXCLUDED.exit_pool_id,
                listen_port = EXCLUDED.listen_port,
                enabled = TRUE,
                visibility_weight = EXCLUDED.visibility_weight
            "#,
        )
        .bind(second_line_id)
        .bind(node_id)
        .bind(group_id)
        .bind(exit_pool_id)
        .bind(user_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        register_test_binding_node_for_access_line(
            &store,
            second_line_id,
            node_id,
            uuid("00000000-0000-0000-0000-000000000302"),
            exit_pool_id,
            group_id,
        )
        .await;
        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml.contains("port: 443"));
        assert!(yaml.contains("port: 8443"));
        let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let mut proxy_ports = profile["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy["port"].as_i64())
            .collect::<Vec<_>>();
        proxy_ports.sort();
        assert_eq!(proxy_ports, vec![443, 8443]);
        let groups = profile["proxy-groups"].as_sequence().unwrap();
        assert_eq!(groups.len(), 1);
        let subscription_json = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        let mut visible_ports = subscription_json["access_lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|line| line["listen_port"].as_i64())
            .collect::<Vec<_>>();
        visible_ports.sort();
        assert_eq!(visible_ports, vec![443, 8443]);
        let mut assigned = assigned_line_ids(&store, user_id, group_id).await;
        assigned.sort();
        assert_eq!(assigned, vec![first_line_id, second_line_id]);

        let yaml_again = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml_again.contains("port: 443"));
        assert!(yaml_again.contains("port: 8443"));
        let mut assigned_again = assigned_line_ids(&store, user_id, group_id).await;
        assigned_again.sort();
        assert_eq!(assigned_again, assigned);

        sqlx::query("UPDATE access_lines SET enabled = FALSE WHERE id = $1")
            .bind(second_line_id)
            .execute(store.pool())
            .await
            .unwrap();
        let failover_yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(failover_yaml.contains("香港 01"));
        assert!(!failover_yaml.contains("🇭🇰 香港 01"));
        assert!(failover_yaml.contains("port: 443"));
        assert_eq!(
            assigned_line_ids(&store, user_id, group_id).await,
            vec![first_line_id]
        );
    }

    #[tokio::test]
    async fn test_pg_trojan_access_line_subscription_uses_password_shape_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL trojan access subscription test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        sqlx::query(
            r#"
            UPDATE access_lines
            SET protocol = 'trojan',
                udp_enabled = FALSE,
                inbound_config = jsonb_build_object(
                    'security', 'tls',
                    'server_name', 'www.cloudflare.com',
                    'tls_certificate_file', '/etc/letsencrypt/live/www.cloudflare.com/fullchain.pem',
                    'tls_key_file', '/etc/letsencrypt/live/www.cloudflare.com/privkey.pem'
                )
            WHERE id = $1
            "#,
        )
        .bind(access_line_id)
        .execute(store.pool())
        .await
        .unwrap();

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let access_credential: String =
            sqlx::query_scalar("SELECT access_credential FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        let expected_password =
            xrayc_core::binding_credential("trojan", &access_credential, access_line_id);

        assert!(yaml.contains("type: trojan"));
        assert!(yaml.contains(&format!("password: {expected_password}")));
        assert!(!yaml.contains(&format!("password: {user_id}")));
        assert!(yaml.contains("sni: www.cloudflare.com"));
        assert!(yaml.contains("udp: false"));
        assert!(!yaml.contains("uuid:"));
        assert!(!yaml.contains("flow:"));
        assert!(!yaml.contains("reality-opts:"));
        assert!(!yaml.contains("seeded-public-key"));
    }

    #[tokio::test]
    async fn test_pg_line_group_shrink_recomputes_visible_line_assignments_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL line group shrink test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let seed_exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let seed_access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("临时收缩分组-{}", Uuid::new_v4().simple()),
                country_code: "TMP".to_string(),
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
        let listen_host = format!("shrink-{}.access.example.test", Uuid::new_v4().simple());
        let first_line_id = Uuid::new_v4();
        let second_line_id = Uuid::new_v4();
        for (line_id, name, exit_pool_id, port, weight) in [
            (
                first_line_id,
                "临时收缩线路 A",
                exit_pool_id,
                34_601_i32,
                200_i32,
            ),
            (
                second_line_id,
                "临时收缩线路 B",
                exit_pool_id,
                34_602_i32,
                100_i32,
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO access_lines (
                    id, name, access_node_id, line_group_id, exit_pool_id, listen_host, listen_port,
                    protocol, transport, user_uuid, enabled,
                    identity_mode, user_key_source, visibility_weight
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    'vless', 'tcp', $8, TRUE,
                    'credential', 'xray_email', $9
                )
                "#,
            )
            .bind(line_id)
            .bind(name)
            .bind(node_id)
            .bind(group_id)
            .bind(exit_pool_id)
            .bind(&listen_host)
            .bind(port)
            .bind(Uuid::new_v4().to_string())
            .bind(weight)
            .execute(store.pool())
            .await
            .unwrap();
            register_test_binding_node_for_access_line(
                &store,
                line_id,
                node_id,
                seed_exit_endpoint_id,
                exit_pool_id,
                group_id,
            )
            .await;
        }

        store
            .replace_admin_line_group_lines(group_id, vec![seed_exit_endpoint_id])
            .await
            .unwrap();
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
        sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
            .bind(user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE access_lines SET enabled = FALSE WHERE id = $1")
            .bind(seed_access_line_id)
            .execute(store.pool())
            .await
            .unwrap();

        let initial = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        assert_eq!(initial["access_lines"].as_array().unwrap().len(), 2);
        let mut initial_assigned = assigned_line_ids(&store, user_id, group_id).await;
        initial_assigned.sort();
        let mut expected_initial_assigned = vec![first_line_id, second_line_id];
        expected_initial_assigned.sort();
        assert_eq!(initial_assigned, expected_initial_assigned);

        sqlx::query("UPDATE access_lines SET enabled = FALSE WHERE id = $1")
            .bind(first_line_id)
            .execute(store.pool())
            .await
            .unwrap();
        let shrunk = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        let visible_names = shrunk["access_lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|line| line["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(visible_names.len(), 1);
        assert!(
            visible_names[0].starts_with("临时收缩线路 B"),
            "unexpected visible node name: {}",
            visible_names[0]
        );
        assert_eq!(
            assigned_line_ids(&store, user_id, group_id).await,
            vec![second_line_id]
        );
        let stale_assignment_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM user_access_line_assignments
            WHERE user_id = $1 AND access_line_id = $2
            "#,
        )
        .bind(user_id)
        .bind(first_line_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(stale_assignment_count, 0);
    }
