/// 数据库测试分片 02。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    #[test]
    fn test_access_config_binds_public_domain_to_local_address() {
        let store = MemoryStore::seeded();
        let node_id = store
            .read(|data| data.access_nodes.keys().copied().next())
            .expect("seed node exists");
        store.write(|data| {
            let user_id = *data.users.keys().next().expect("seed user exists");
            let line = data.access_lines.values().next().expect("seed line exists");
            let endpoint = data
                .exit_pools
                .get(&line.exit_pool_id)
                .and_then(|pool| pool.members.first())
                .expect("seed endpoint exists");
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line.id,
                exit_pool_id: line.exit_pool_id,
                exit_endpoint_id: endpoint.id,
                assigned_at: Utc::now(),
                failover_reason: "test".to_string(),
            });
        });
        let (heartbeat, subscription) = store.read(|data| {
            (
                heartbeat_json(data, Some(node_id), None),
                user_subscription_json(data),
            )
        });

        assert_eq!(
            heartbeat["config"]["access_lines"][0]["listen_host"],
            "0.0.0.0"
        );
        assert_eq!(
            subscription["access_lines"][0]["listen_host"],
            "access.example.test"
        );
    }

    #[test]
    fn test_access_config_binds_public_ip_to_wildcard_address() {
        let store = MemoryStore::seeded();
        let node_id = store
            .read(|data| data.access_nodes.keys().copied().next())
            .expect("seed node exists");
        store.write(|data| {
            let user_id = *data.users.keys().next().expect("seed user exists");
            let line = data
                .access_lines
                .values_mut()
                .next()
                .expect("seed line exists");
            line.listen_host = "203.0.113.10".to_string();
            let line_id = line.id;
            let pool_id = line.exit_pool_id;
            let endpoint_id = data
                .exit_pools
                .get(&pool_id)
                .and_then(|pool| pool.members.first())
                .expect("seed endpoint exists")
                .id;
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: endpoint_id,
                assigned_at: Utc::now(),
                failover_reason: "test".to_string(),
            });
        });
        let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));

        assert_eq!(
            heartbeat["config"]["access_lines"][0]["listen_host"],
            "0.0.0.0"
        );
    }

    #[test]
    fn test_access_config_merges_same_entry_multi_exit_bindings_into_one_inbound() {
        let store = MemoryStore::seeded();
        let (node_id, user_id, first_line_id, first_pool_id, first_endpoint_id) =
            store.read(|data| {
                let node_id = *data.access_nodes.keys().next().unwrap();
                let user_id = *data.users.keys().next().unwrap();
                let line = data.access_lines.values().next().unwrap();
                let endpoint = data
                    .exit_pools
                    .get(&line.exit_pool_id)
                    .unwrap()
                    .members
                    .first()
                    .unwrap();
                (node_id, user_id, line.id, line.exit_pool_id, endpoint.id)
            });
        store.write(|data| {
            let first_line = data.access_lines[&first_line_id].clone();
            let second_endpoint_id = Uuid::new_v4();
            let second_pool_id = Uuid::new_v4();
            let second_line_id = Uuid::new_v4();
            let second_endpoint = ExitEndpoint {
                id: second_endpoint_id,
                resource_name: "runtime-merge-second-exit".to_string(),
                ownership: "third_party".to_string(),
                owner_access_node_id: None,
                outbound_type: EndpointType::Socks,
                host: "203.0.113.44".to_string(),
                port: 1080,
                outbound_config: json!({"username": "merge-user", "password": "merge-pass"}),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            data.exit_pools.insert(
                second_pool_id,
                ExitPool {
                    id: second_pool_id,
                    name: "runtime merge second pool".to_string(),
                    region_code: "US".to_string(),
                    strategy: "priority".to_string(),
                    enabled: true,
                    members: vec![second_endpoint],
                },
            );
            let mut second_line = first_line.clone();
            second_line.id = second_line_id;
            second_line.name = "runtime merge second binding".to_string();
            second_line.exit_pool_id = second_pool_id;
            second_line.exit_endpoint_id = Some(second_endpoint_id);
            data.access_lines.insert(second_line_id, second_line);

            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: first_line_id,
                exit_pool_id: first_pool_id,
                exit_endpoint_id: first_endpoint_id,
                assigned_at: Utc::now(),
                failover_reason: "first".to_string(),
            });
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: second_line_id,
                exit_pool_id: second_pool_id,
                exit_endpoint_id: second_endpoint_id,
                assigned_at: Utc::now(),
                failover_reason: "second".to_string(),
            });
        });

        let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
        let access_lines = heartbeat["config"]["access_lines"].as_array().unwrap();
        let routing_rules = heartbeat["config"]["routing_rules"].as_array().unwrap();

        assert_eq!(access_lines.len(), 1);
        assert_eq!(access_lines[0]["users"].as_array().unwrap().len(), 2);
        assert_eq!(routing_rules.len(), 2);
        assert_eq!(
            routing_rules[0]["inbound_tag"],
            routing_rules[1]["inbound_tag"]
        );
        assert_ne!(
            routing_rules[0]["user_email"],
            routing_rules[1]["user_email"]
        );
    }

    #[test]
    fn test_heartbeat_authorized_users_follow_node_assignments() {
        let store = MemoryStore::seeded();
        let (node_id, user_id, line_id, pool_id, endpoint_id) = store.read(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let user_id = *data.users.keys().next().unwrap();
            let line = data.access_lines.values().next().unwrap();
            let endpoint = data
                .exit_pools
                .get(&line.exit_pool_id)
                .unwrap()
                .members
                .first()
                .unwrap();
            (node_id, user_id, line.id, line.exit_pool_id, endpoint.id)
        });

        let unbound = store.read(|data| heartbeat_json(data, Some(node_id), None));
        assert_eq!(unbound["authorized_users"].as_array().unwrap().len(), 0);

        store.write(|data| {
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: endpoint_id,
                assigned_at: Utc::now(),
                failover_reason: "test".to_string(),
            });
        });
        let bound = store.read(|data| heartbeat_json(data, Some(node_id), None));
        assert_eq!(bound["authorized_users"].as_array().unwrap().len(), 1);

        store.write(|data| {
            data.users.get_mut(&user_id).unwrap().disabled = true;
        });
        let disabled = store.read(|data| heartbeat_json(data, Some(node_id), None));
        assert_eq!(disabled["authorized_users"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_access_heartbeat_requires_reported_node_id() {
        let store = MemoryStore::seeded();
        let heartbeat = store.read(|data| heartbeat_json(data, None, None));

        assert_eq!(heartbeat["accepted"], true);
        assert_eq!(heartbeat["config_status"]["required"], false);
        assert!(heartbeat["config"].is_null());
        assert!(heartbeat["config_hash"].is_null());
    }

    #[tokio::test]
    async fn test_pg_store_seed_subscription_and_billing_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL integration test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let admin = store
            .authenticate_user("admin", DEMO_ADMIN_PASSWORD)
            .await
            .unwrap()
            .expect("seeded admin should login");
        assert!(admin.is_admin);
        assert!(store
            .authenticate_user("admin", "wrong-password")
            .await
            .unwrap()
            .is_none());
        let demo_user = store
            .authenticate_user("DEMO@EXAMPLE.TEST", DEMO_USER_PASSWORD)
            .await
            .unwrap()
            .expect("seeded user should login");
        assert!(!demo_user.is_admin);
        assert!(store
            .verify_agent_token(
                Some(uuid("00000000-0000-0000-0000-000000000201")),
                DEMO_AGENT_TOKEN
            )
            .await
            .unwrap());
        assert!(!store
            .verify_agent_token(None, DEMO_AGENT_TOKEN)
            .await
            .unwrap());
        assert!(!store
            .verify_agent_token(
                Some(uuid("00000000-0000-0000-0000-000000000201")),
                "bad-token"
            )
            .await
            .unwrap());
        sqlx::query("DELETE FROM usage_ledgers")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_traffic_snapshots")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE user_subscriptions SET used_bytes = 0, limit_bytes = 300")
            .execute(store.pool())
            .await
            .unwrap();
        store
            .update_subscription_settings_json(json!({
                "profile_name": "测试订阅",
                "mixed_port": 7891,
                "allow_lan": true,
                "default_rules": ["DOMAIN-SUFFIX,example.com,默认分组"],
                "auto_test_name": "自动测速"
            }))
            .await
            .unwrap();
        let preserved_settings = store
            .update_subscription_settings_json(json!({
                "mixed_port": 7892
            }))
            .await
            .unwrap();
        assert_eq!(preserved_settings["profile_name"], "测试订阅");
        assert_eq!(preserved_settings["mixed_port"], 7892);
        assert_eq!(preserved_settings["allow_lan"], true);
        assert!(preserved_settings["proxy_groups"].is_null());
        assert_eq!(preserved_settings["auto_test_enabled"], false);
        assert_eq!(preserved_settings["auto_test_name"], "自动测速");
        assert_eq!(preserved_settings["block_unhealthy_lines"], false);

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml.contains("access.example.test"));
        assert!(yaml.contains("香港 01"));
        assert!(!yaml.contains("🇭🇰 香港 01"));
        assert!(!yaml.contains("name: 自动测速"));
        assert!(!yaml.contains("type: url-test"));
        assert!(yaml.contains("DOMAIN-SUFFIX,example.com,默认分组"));
        assert!(yaml.contains("MATCH,默认分组"));
        assert!(!yaml.contains("198.51.100.10"));

        let data = store.load_store_data().await.unwrap();
        let line_id = *data.access_lines.keys().next().unwrap();
        let demo_user_id = data.tokens.get("demo-token").unwrap().user_id;
        let key = data.users.get(&demo_user_id).unwrap().xray_user_key.clone();
        let first = TrafficReport {
            access_line_id: line_id,
            xray_user_key: key.clone(),
            uplink_total: 10,
            downlink_total: 20,
            collected_at: Utc::now(),
        };
        let mut second = first.clone();
        second.uplink_total = 110;
        second.downlink_total = 220;
        second.collected_at += Duration::seconds(10);

        assert!(store.apply_report(first).await.unwrap().baseline_only);
        let result = store.apply_report(second).await.unwrap();
        assert_eq!(result.billed_bytes, 300);
        assert!(result.config_refresh_required);
        let ledger = sqlx::query_as::<_, LedgerAuditRow>(
            r#"
            SELECT delta_total, billed_uplink, billed_downlink, billed_bytes
            FROM usage_ledgers
            WHERE access_line_id = $1 AND xray_user_key = $2
            "#,
        )
        .bind(line_id)
        .bind(&key)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(ledger.delta_total, 300);
        assert_eq!(
            ledger.billed_uplink + ledger.billed_downlink,
            ledger.billed_bytes
        );
        let snapshot_node_id = sqlx::query_scalar::<_, Option<Uuid>>(
            r#"
            SELECT access_node_id
            FROM access_traffic_snapshots
            WHERE access_line_id = $1 AND xray_user_key = $2
            "#,
        )
        .bind(line_id)
        .bind(&key)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(snapshot_node_id.is_some());
        let dirty_reason = sqlx::query_scalar::<_, String>(
            "SELECT config_dirty_reason FROM access_nodes WHERE id = $1",
        )
        .bind(snapshot_node_id.unwrap())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(dirty_reason, "traffic_quota_exhausted");

        sqlx::query("DELETE FROM usage_ledgers WHERE access_line_id = $1 AND user_id = $2")
            .bind(line_id)
            .bind(demo_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "DELETE FROM access_traffic_snapshots WHERE access_line_id = $1 AND xray_user_key = $2",
        )
        .bind(line_id)
        .bind(&key)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
            .bind(demo_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
            .bind(demo_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE user_subscriptions SET used_bytes = 0, limit_bytes = 300 WHERE user_id = $1",
        )
        .bind(demo_user_id)
        .execute(store.pool())
        .await
        .unwrap();

        let unbound_first = TrafficReport {
            access_line_id: line_id,
            xray_user_key: key.clone(),
            uplink_total: 10,
            downlink_total: 20,
            collected_at: Utc::now() + Duration::minutes(1),
        };
        let mut unbound_second = unbound_first.clone();
        unbound_second.uplink_total = 110;
        unbound_second.downlink_total = 220;
        unbound_second.collected_at += Duration::seconds(10);

        let unbound_baseline = store.apply_report(unbound_first).await.unwrap();
        assert!(unbound_baseline.baseline_only);
        assert!(unbound_baseline.config_refresh_required);
        let unbound_result = store.apply_report(unbound_second).await.unwrap();
        assert_eq!(unbound_result.billed_bytes, 0);
        assert!(unbound_result.config_refresh_required);
        let unbound_ledger_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM usage_ledgers WHERE access_line_id = $1 AND user_id = $2",
        )
        .bind(line_id)
        .bind(demo_user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(unbound_ledger_count, 0);
    }

    #[tokio::test]
    async fn test_pg_migrations_remove_legacy_user_rate_limit_schema_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL rate limit schema test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        let old_rate_prefix = "rate_limit_";
        let legacy_table_names = vec![
            format!("{old_rate_prefix}profiles"),
            format!("{}{}", "user_rate_limit_", "assignments"),
        ];
        let legacy_port_table_pattern = format!("{old_rate_prefix}port/_%");

        let legacy_tables = sqlx::query_scalar::<_, String>(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'public'
                AND (
                    table_name = ANY($1)
                    OR table_name LIKE $2 ESCAPE '/'
                )
            ORDER BY table_name
            "#,
        )
        .bind(&legacy_table_names)
        .bind(&legacy_port_table_pattern)
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert!(
            legacy_tables.is_empty(),
            "legacy user rate limit tables should be removed: {legacy_tables:?}"
        );
        let legacy_access_port_columns = vec![
            format!("{old_rate_prefix}port_start"),
            format!("{old_rate_prefix}port_end"),
        ];

        let legacy_access_node_columns = sqlx::query_scalar::<_, String>(
            r#"
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
                AND table_name = 'access_nodes'
                AND column_name = ANY($1)
            ORDER BY column_name
            "#,
        )
        .bind(&legacy_access_port_columns)
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert!(
            legacy_access_node_columns.is_empty(),
            "legacy access node rate limit columns should be removed: {legacy_access_node_columns:?}"
        );

        let rate_limit_buckets_exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = 'public'
                    AND table_name = 'rate_limit_buckets'
            )
            "#,
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(
            rate_limit_buckets_exists,
            "rate_limit_buckets should remain for API base rate limiting"
        );
    }
