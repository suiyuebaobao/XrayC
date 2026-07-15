/// 数据库测试分片 12。
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
    async fn test_pg_invalid_vless_reality_endpoint_is_not_assignable_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL invalid VLESS endpoint test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let resource_id = uuid("00000000-0000-0000-0000-000000009321");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000009322");

        sqlx::query(
            "UPDATE exit_pool_members SET status = 'offline', allow_new_assignments = FALSE WHERE exit_pool_id = $1",
        )
        .bind(pool_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_resources (
                id, name, region_code, ownership, enabled
            )
            VALUES ($1, 'invalid-vless-reality', 'US', 'third_party', TRUE)
            ON CONFLICT (id) DO UPDATE SET enabled = TRUE
            "#,
        )
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, outbound_type, host, port,
                outbound_config, enabled
            )
            VALUES (
                $1, $2, 'vless'::endpoint_type, 'invalid-vless.example.test', 443,
                '{"uuid":"5dd7c58d-2ae0-478e-bba9-80354e17e107","security":"reality","server_name":"invalid-vless-sni.example.test"}'::jsonb,
                TRUE
            )
            ON CONFLICT (id) DO UPDATE SET
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(endpoint_id)
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status,
                allow_new_assignments
            )
            VALUES ($1, $2, 100, 'healthy', TRUE)
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                status = EXCLUDED.status,
                allow_new_assignments = EXCLUDED.allow_new_assignments
            "#,
        )
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM user_exit_assignments WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3",
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .execute(store.pool())
        .await
        .unwrap();

        let subscription = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        assert_eq!(
            subscription["access_lines"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            0
        );
        let assigned_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM user_exit_assignments
            WHERE user_id = $1
              AND access_line_id = $2
              AND exit_pool_id = $3
              AND exit_endpoint_id = $4
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(assigned_count, 0);

        let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        let invalid_tag = exit_endpoint_tag(endpoint_id);
        assert!(!heartbeat["config"]["exit_endpoints"]
            .as_array()
            .is_some_and(|endpoints| endpoints
                .iter()
                .any(|endpoint| endpoint["tag"] == invalid_tag)));
    }

    #[tokio::test]
    async fn test_pg_revoked_subscription_token_clears_clean_agent_config_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL revoked token config test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let direct_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        sqlx::query("UPDATE exit_endpoints SET enabled = TRUE WHERE id = $1")
            .bind(direct_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE WHEN exit_endpoint_id = $2 THEN 'healthy' ELSE 'offline' END,
                allow_new_assignments = CASE WHEN exit_endpoint_id = $2 THEN TRUE ELSE FALSE END
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(exit_pool_id)
        .bind(direct_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let first = store.heartbeat_json(Some(node_id), None).await.unwrap();
        assert_eq!(first["config_status"]["required"], true);
        assert!(first["config"]["access_lines"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()));
        let old_config_hash = first["config_hash"].as_str().unwrap().to_string();
        store
            .record_config_result(Some(node_id), Some(&old_config_hash), true, None)
            .await
            .unwrap();
        let clean =
            sqlx::query_scalar::<_, bool>("SELECT config_dirty FROM access_nodes WHERE id = $1")
                .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(!clean);
        sqlx::query(
            r#"
            INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id)
            VALUES ($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(line_group_id)
        .bind(line_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO user_exit_assignments (user_id, access_line_id, exit_pool_id, exit_endpoint_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
                exit_endpoint_id = EXCLUDED.exit_endpoint_id
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(exit_pool_id)
        .bind(direct_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        sqlx::query(
            "UPDATE subscription_tokens SET revoked_at = now() WHERE token = 'demo-token'",
        )
        .execute(store.pool())
        .await
        .unwrap();
        assert!(matches!(
            store.generate_subscription_yaml("demo-token").await,
            Err(DbError::Subscription(SubscriptionError::TokenNotFound))
        ));
        let access_assignment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_access_line_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let exit_assignment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_exit_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(access_assignment_count, 0);
        assert_eq!(exit_assignment_count, 0);

        let revoked = store
            .heartbeat_json(Some(node_id), Some(&old_config_hash))
            .await
            .unwrap();
        assert_eq!(revoked["config_status"]["required"], true);
        assert_ne!(
            revoked["config_hash"].as_str().unwrap_or_default(),
            old_config_hash
        );
        assert!(
            revoked["config"].is_null(),
            "无有效 token 时应要求 agent 写入空配置，而不是保留旧 Xray 用户"
        );

        let empty_config_hash = revoked["config_hash"].as_str().unwrap().to_string();
        store
            .record_config_result(Some(node_id), Some(&empty_config_hash), true, None)
            .await
            .unwrap();
        let settled = store
            .heartbeat_json(Some(node_id), Some(&empty_config_hash))
            .await
            .unwrap();
        assert_eq!(settled["config_status"]["required"], false);
        assert!(settled["config"].is_null());
    }

    #[tokio::test]
    async fn test_pg_register_refresh_and_logout_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL auth lifecycle test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let email = format!("user-{}@example.test", Uuid::new_v4().simple());
        let registered = store.register_user(&email, "register123").await.unwrap();

        assert_eq!(registered.user.email, email);
        assert!(registered.subscription_token.starts_with("sub-"));
        assert!(store
            .authenticate_user(&email, "register123")
            .await
            .unwrap()
            .is_some());
        assert!(matches!(
            store.register_user(&email, "register123").await,
            Err(DbError::EmailExists)
        ));
        let subscription = store
            .user_subscription_json_for_user(registered.user.id)
            .await
            .unwrap();
        assert_eq!(subscription["token"], registered.subscription_token);
        let old_access_credential: String =
            sqlx::query_scalar("SELECT access_credential FROM users WHERE id = $1")
                .bind(registered.user.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();

        sqlx::query("UPDATE users SET disabled = TRUE WHERE id = $1")
            .bind(registered.user.id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(matches!(
            store
                .generate_subscription_yaml(&registered.subscription_token)
                .await,
            Err(DbError::Subscription(SubscriptionError::TokenNotFound))
        ));
        sqlx::query("UPDATE users SET disabled = FALSE WHERE id = $1")
            .bind(registered.user.id)
            .execute(&store.pool)
            .await
            .unwrap();

        let reset_subscription = store
            .reset_subscription_token_for_user_json(registered.user.id)
            .await
            .unwrap();
        let reset_token = reset_subscription["token"].as_str().unwrap_or_default();
        assert!(reset_token.starts_with("sub-"));
        assert_ne!(reset_token, registered.subscription_token);
        assert!(matches!(
            store
                .generate_subscription_yaml(&registered.subscription_token)
                .await,
            Err(DbError::Subscription(SubscriptionError::TokenNotFound))
        ));
        let reset_yaml = store.generate_subscription_yaml(reset_token).await.unwrap();
        let reset_access_credential: String =
            sqlx::query_scalar("SELECT access_credential FROM users WHERE id = $1")
                .bind(registered.user.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let reset_binding_credential =
            xrayc_core::binding_credential("vless", &reset_access_credential, access_line_id);
        assert_ne!(reset_access_credential, old_access_credential);
        assert!(reset_yaml.contains(&reset_binding_credential));
        assert!(!reset_yaml.contains(&reset_access_credential));
        assert!(
            !reset_yaml.contains(&registered.user.id.to_string()),
            "重置订阅后不能继续使用稳定用户 UUID 作为节点入站凭据"
        );
        let heartbeat = store
            .heartbeat_json(Some(uuid("00000000-0000-0000-0000-000000000201")), None)
            .await
            .unwrap();
        let reset_config_hash = heartbeat["config_hash"].as_str().unwrap().to_string();
        let access_users = heartbeat["config"]["access_lines"][0]["users"]
            .as_array()
            .unwrap();
        assert!(access_users
            .iter()
            .any(|user| user["credential"].as_str() == Some(reset_binding_credential.as_str())));
        let rendered_config = serde_json::to_string(&heartbeat["config"]).unwrap();
        assert!(!rendered_config.contains(&registered.user.id.to_string()));
        let dirty_reason: String =
            sqlx::query_scalar("SELECT config_dirty_reason FROM access_nodes LIMIT 1")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(dirty_reason, "subscription_token_reset_rotated_credential");
        sqlx::query("UPDATE subscription_tokens SET revoked_at = now() WHERE token = $1")
            .bind(reset_token)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(matches!(
            store.generate_subscription_yaml(reset_token).await,
            Err(DbError::Subscription(SubscriptionError::TokenNotFound))
        ));
        let revoked_heartbeat = store
            .heartbeat_json(
                Some(uuid("00000000-0000-0000-0000-000000000201")),
                Some(&reset_config_hash),
            )
            .await
            .unwrap();
        assert_eq!(revoked_heartbeat["config_status"]["required"], true);
        let revoked_config = serde_json::to_string(&revoked_heartbeat["config"]).unwrap();
        assert!(
            !revoked_config.contains(&reset_binding_credential),
            "订阅 token 已失效时，agent 新配置不能继续保留旧客户端入站凭据"
        );
        let restored_subscription = store
            .reset_subscription_token_for_user_json(registered.user.id)
            .await
            .unwrap();
        assert!(restored_subscription["token"]
            .as_str()
            .is_some_and(|token| token.starts_with("sub-")));
        let token_row_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM subscription_tokens WHERE user_id = $1")
                .bind(registered.user.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(token_row_count, 1);

        let refresh = store
            .create_refresh_token(registered.user.id, Duration::days(7))
            .await
            .unwrap();
        assert!(store
            .authenticate_refresh_token(&refresh)
            .await
            .unwrap()
            .is_some());
        let (rotated_user, rotated) = store
            .rotate_refresh_token(&refresh, Duration::days(7))
            .await
            .unwrap()
            .expect("refresh token should rotate");
        assert_eq!(rotated_user.id, registered.user.id);
        assert_ne!(rotated, refresh);
        assert!(store
            .authenticate_refresh_token(&refresh)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .authenticate_refresh_token(&rotated)
            .await
            .unwrap()
            .is_some());
        store.revoke_refresh_token(&rotated).await.unwrap();
        assert!(store
            .authenticate_refresh_token(&rotated)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_pg_authorize_access_claims_rechecks_user_state_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL access claims test");
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
        assert!(
            store
                .authorize_access_claims(admin.id, true)
                .await
                .unwrap()
                .is_admin
        );

        sqlx::query("UPDATE users SET is_admin = FALSE WHERE id = $1")
            .bind(admin.id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(matches!(
            store.authorize_access_claims(admin.id, true).await,
            Err(DbError::AdminRequired)
        ));

        let email = format!("claims-{}@example.test", Uuid::new_v4().simple());
        let registered = store.register_user(&email, "register123").await.unwrap();
        assert!(store
            .authorize_access_claims(registered.user.id, false)
            .await
            .is_ok());

        sqlx::query("UPDATE users SET disabled = TRUE WHERE id = $1")
            .bind(registered.user.id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(matches!(
            store
                .authorize_access_claims(registered.user.id, false)
                .await,
            Err(DbError::UserDisabled)
        ));
    }
