/// 数据库测试分片 03。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_migrations_keep_unified_quota_schema_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL quota schema test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();

        let plan_quota_columns = sqlx::query_scalar::<_, String>(
            r#"
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'plans'
              AND column_name LIKE '%traffic%'
            ORDER BY column_name
            "#,
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(plan_quota_columns, vec!["traffic_limit_bytes".to_string()]);

        let subscription_quota_columns = sqlx::query_scalar::<_, String>(
            r#"
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'user_subscriptions'
              AND column_name IN ('used_bytes', 'limit_bytes')
            ORDER BY column_name
            "#,
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(
            subscription_quota_columns,
            vec!["limit_bytes".to_string(), "used_bytes".to_string()]
        );

        let routing_quota_columns = sqlx::query_scalar::<_, String>(
            r#"
            SELECT table_name || '.' || column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name IN ('exit_resources', 'exit_pools', 'access_lines', 'usage_ledgers')
              AND column_name IN ('traffic_' || 'pool', 'traffic_limit_bytes', 'traffic_bytes')
            ORDER BY table_name, column_name
            "#,
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert!(
            routing_quota_columns.is_empty(),
            "routing tables should not carry quota columns: {routing_quota_columns:?}"
        );
    }

    #[tokio::test]
    async fn test_pg_migration_turns_legacy_subscription_auto_test_off_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL subscription setting migration test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();

        sqlx::query(
            r#"
            INSERT INTO site_settings (setting_key, setting_value, updated_at)
            VALUES (
                'subscription_config',
                jsonb_build_object('auto_test_enabled', true, 'auto_test_name', '自动选择'),
                now()
            )
            ON CONFLICT (setting_key) DO UPDATE SET
                setting_value = EXCLUDED.setting_value,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .execute(store.pool())
        .await
        .unwrap();

        sqlx::raw_sql(include_str!(
            "../../../../migrations/202605250002_default_subscription_auto_test_off.sql"
        ))
        .execute(store.pool())
        .await
        .unwrap();

        let enabled = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT (setting_value->>'auto_test_enabled')::boolean
            FROM site_settings
            WHERE setting_key = 'subscription_config'
            "#,
        )
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert!(!enabled);
    }

    #[tokio::test]
    async fn test_pg_auth_challenge_is_consumed_atomically_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL auth challenge race test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();

        let scene = "login";
        let target = format!("challenge-race-{}@example.test", Uuid::new_v4().simple());
        let challenge_id = store
            .create_auth_challenge(scene, &target, "246810", 60)
            .await
            .unwrap();
        let barrier = Arc::new(Barrier::new(16));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let store = store.clone();
            let target = target.clone();
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                store
                    .verify_auth_challenge(scene, &target, challenge_id, "246810")
                    .await
                    .unwrap()
            }));
        }

        let mut success_count = 0;
        for task in tasks {
            if task.await.unwrap() {
                success_count += 1;
            }
        }
        assert_eq!(success_count, 1);

        let replay = store
            .verify_auth_challenge(scene, &target, challenge_id, "246810")
            .await
            .unwrap();
        assert!(!replay);
    }

    #[tokio::test]
    async fn test_pg_redeem_code_allows_only_one_concurrent_consumer_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL redeem race test");
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
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let generated = store
            .create_admin_redeem_codes_json(admin.id, plan_id, 1, Some(1), None)
            .await
            .unwrap();
        let code = Arc::new(
            generated["items"][0]["code"]
                .as_str()
                .expect("generated redeem code")
                .to_string(),
        );

        let mut users = Vec::new();
        let mut initial_limits = HashMap::new();
        for index in 0..12 {
            let email = format!(
                "redeem-race-{}-{index}@example.test",
                Uuid::new_v4().simple()
            );
            let registered = store.register_user(&email, "register123").await.unwrap();
            let initial_limit = sqlx::query_scalar::<_, i64>(
                "SELECT limit_bytes FROM user_subscriptions WHERE user_id = $1",
            )
            .bind(registered.user.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
            initial_limits.insert(registered.user.id, initial_limit);
            users.push(registered.user.id);
        }

        let barrier = Arc::new(Barrier::new(users.len()));
        let mut tasks = Vec::new();
        for user_id in users.iter().copied() {
            let store = store.clone();
            let code = Arc::clone(&code);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                (user_id, store.redeem_code_for_user(user_id, &code).await)
            }));
        }

        let mut success_users = Vec::new();
        let mut business_error_count = 0;
        for task in tasks {
            let (user_id, result) = task.await.unwrap();
            match result {
                Ok(_) => success_users.push(user_id),
                Err(error @ DbError::RedeemCodeUsed) => {
                    // 透出给用户的消息必须干净,不带 "Agent 上报" 内部前缀。
                    assert_eq!(error.to_string(), "兑换码已使用");
                    business_error_count += 1;
                }
                Err(error) => panic!("unexpected redeem error: {error:?}"),
            }
        }
        assert_eq!(success_users.len(), 1);
        assert_eq!(business_error_count, users.len() - 1);
        let success_user = success_users[0];

        let used_by_user_id = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT used_by_user_id FROM redeem_codes WHERE code = $1 AND is_used = TRUE AND used_at IS NOT NULL",
        )
        .bind(&*code)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(used_by_user_id, Some(success_user));

        let redeem_traffic = 10_i64 * 1024 * 1024 * 1024;
        for user_id in &users {
            let traffic_limit = sqlx::query_scalar::<_, i64>(
                "SELECT limit_bytes FROM user_subscriptions WHERE user_id = $1",
            )
            .bind(user_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
            let expected = initial_limits[user_id]
                + if *user_id == success_user {
                    redeem_traffic
                } else {
                    0
                };
            assert_eq!(traffic_limit, expected);
        }

        let ledger_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM usage_ledgers WHERE user_id = ANY($1)",
        )
        .bind(&users)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(ledger_count, 0);
    }

    #[tokio::test]
    async fn test_pg_exit_endpoint_stores_sensitive_config_as_plaintext() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL sensitive config plaintext test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let suffix = Uuid::new_v4().simple().to_string();
        let exit_resource_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_resources (name, region_code, enabled)
            VALUES ($1, 'US', TRUE)
            RETURNING id
            "#,
        )
        .bind(format!("加密出口资源-{suffix}"))
        .fetch_one(store.pool())
        .await
        .unwrap();
        let endpoint_id = store
            .create_admin_exit_endpoint(AdminExitEndpointInput {
                exit_resource_id,
                name: format!("加密 SOCKS-{suffix}"),
                outbound_type: "socks".to_string(),
                host: "127.0.0.1".to_string(),
                port: 1080,
                outbound_config: json!({
                    "username": "agent-user",
                    "password": "agent-secret"
                }),
                stream_config: json!({
                    "tls": {
                        "password": "stream-secret"
                    }
                }),
                probe_config: json!({
                    "headers": {
                        "token": "probe-secret"
                    }
                }),
                enabled: true,
            })
            .await
            .unwrap();
        let raw_config = sqlx::query_scalar::<_, Value>(
            "SELECT outbound_config FROM exit_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        // 字段加密已移除，敏感字段以明文落库，库里应直接含明文且不带密文前缀。
        let raw_text = raw_config.to_string();
        assert!(raw_text.contains("agent-user"));
        assert!(raw_text.contains("agent-secret"));

        let opened = raw_config;
        assert_eq!(opened["username"], "agent-user");
        assert_eq!(opened["password"], "agent-secret");

        let raw_stream = sqlx::query_scalar::<_, Value>(
            "SELECT stream_config FROM exit_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let raw_probe =
            sqlx::query_scalar::<_, Value>("SELECT probe_config FROM exit_endpoints WHERE id = $1")
                .bind(endpoint_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        let raw_stream_text = raw_stream.to_string();
        let raw_probe_text = raw_probe.to_string();
        assert!(raw_stream_text.contains("stream-secret"));
        assert!(raw_probe_text.contains("probe-secret"));
        assert_eq!(raw_stream["tls"]["password"], "stream-secret");
        assert_eq!(raw_probe["headers"]["token"], "probe-secret");
        let access_line_id = store
            .create_admin_access_line(AdminAccessLineInput {
                name: format!("加密中转入口-{suffix}"),
                access_node_id: uuid("00000000-0000-0000-0000-000000000201"),
                exit_pool_id: uuid("00000000-0000-0000-0000-000000000401"),
                listen_host: format!("sealed-{suffix}.example.test"),
                listen_port: 8443,
                protocol: "vless".to_string(),
                transport: "tcp".to_string(),
                user_uuid: String::new(),
                server_name: "www.example.test".to_string(),
                public_key: "public-key".to_string(),
                short_id: "abcd1234".to_string(),
                enabled: true,
                region_code: "US".to_string(),
                region_name: "美国".to_string(),
                region_flag: "🇺🇸".to_string(),
                flow: "xtls-rprx-vision".to_string(),
                udp_enabled: true,
                udp_packet_encoding: String::new(),
                xhttp_path: String::new(),
                xhttp_host: String::new(),
                xhttp_mode: "auto".to_string(),
                identity_mode: "credential".to_string(),
                user_key_source: "xray_email".to_string(),
                inbound_config: json!({
                    "security": "reality",
                    "private_key": "inbound-private-secret",
                    "token": "inbound-token-secret"
                }),
                visibility_weight: 100,
            })
            .await
            .unwrap();
        let raw_inbound =
            sqlx::query_scalar::<_, Value>("SELECT inbound_config FROM access_lines WHERE id = $1")
                .bind(access_line_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        let raw_inbound_text = raw_inbound.to_string();
        assert!(raw_inbound_text.contains("inbound-private-secret"));
        assert!(raw_inbound_text.contains("inbound-token-secret"));
        let opened_inbound = raw_inbound;
        assert_eq!(opened_inbound["private_key"], "inbound-private-secret");
        assert_eq!(opened_inbound["token"], "inbound-token-secret");

        store
            .update_admin_exit_endpoint(
                endpoint_id,
                AdminExitEndpointUpdate {
                    stream_config: Some(json!({"password": "updated-stream-secret"})),
                    probe_config: Some(json!({"token": "updated-probe-secret"})),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let updated_stream = sqlx::query_scalar::<_, Value>(
            "SELECT stream_config FROM exit_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let updated_probe =
            sqlx::query_scalar::<_, Value>("SELECT probe_config FROM exit_endpoints WHERE id = $1")
                .bind(endpoint_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(updated_stream.to_string().contains("updated-stream-secret"));
        assert!(updated_probe.to_string().contains("updated-probe-secret"));
        assert_eq!(updated_stream["password"], "updated-stream-secret");
        assert_eq!(updated_probe["token"], "updated-probe-secret");
    }
