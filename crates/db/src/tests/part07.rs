/// 数据库测试分片 07。
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
    async fn test_pg_exit_member_disable_failover_and_auto_recovery_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL exit auto recovery test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let resource_id = Uuid::new_v4();
        let primary_endpoint_id = Uuid::new_v4();
        let backup_endpoint_id = Uuid::new_v4();
        let pool_id = store
            .create_admin_exit_pool(AdminExitPoolInput {
                name: format!("临时恢复出口池-{}", Uuid::new_v4().simple()),
                region_code: "TMP".to_string(),
                strategy: "priority".to_string(),
                enabled: true,
            })
            .await
            .unwrap();
        let access_line_id = Uuid::new_v4();
        let group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("临时恢复分组-{}", Uuid::new_v4().simple()),
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
        let listen_host = format!("recovery-{}.access.example.test", Uuid::new_v4().simple());

        sqlx::query(
            r#"
            INSERT INTO exit_resources (
                id, name, region_code, ownership, access_node_id, enabled
            )
            VALUES ($1, '临时恢复出口资源', 'TMP', 'self_hosted', NULL, TRUE)
            "#,
        )
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        for (endpoint_id, host) in [
            (primary_endpoint_id, "198.51.100.21"),
            (backup_endpoint_id, "198.51.100.22"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO exit_endpoints (
                    id, exit_resource_id, outbound_type, host, port,
                    outbound_config, enabled
                )
                VALUES ($1, $2, 'socks'::endpoint_type, $3, 1080, '{}'::jsonb, TRUE)
                "#,
            )
            .bind(endpoint_id)
            .bind(resource_id)
            .bind(host)
            .execute(store.pool())
            .await
            .unwrap();
        }
        store
            .replace_admin_exit_pool_members(
                pool_id,
                vec![
                    AdminExitPoolMemberInput {
                        exit_endpoint_id: primary_endpoint_id,
                        weight: 100,
                        priority: 200,
                        status: "healthy".to_string(),
                        allow_new_assignments: true,
                    },
                    AdminExitPoolMemberInput {
                        exit_endpoint_id: backup_endpoint_id,
                        weight: 100,
                        priority: 100,
                        status: "offline".to_string(),
                        allow_new_assignments: true,
                    },
                ],
            )
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_lines (
                id, name, access_node_id, line_group_id, exit_pool_id, listen_host, listen_port,
                protocol, transport, user_uuid, enabled,
                identity_mode, user_key_source, visibility_weight
            )
            VALUES (
                $1, '临时恢复中转入口', $2, $3, $4, $5, 34611,
                'vless', 'tcp', $6, TRUE,
                'credential', 'xray_email', 100
            )
            "#,
        )
        .bind(access_line_id)
        .bind(node_id)
        .bind(group_id)
        .bind(pool_id)
        .bind(&listen_host)
        .bind(Uuid::new_v4().to_string())
        .execute(store.pool())
        .await
        .unwrap();
        store
            .replace_admin_line_group_lines(group_id, vec![primary_endpoint_id, backup_endpoint_id])
            .await
            .unwrap();
        let runtime_pool_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_pool_id FROM access_lines WHERE id = $1",
        )
        .bind(access_line_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        store
            .replace_admin_exit_pool_members(
                runtime_pool_id,
                vec![
                    AdminExitPoolMemberInput {
                        exit_endpoint_id: primary_endpoint_id,
                        weight: 100,
                        priority: 200,
                        status: "healthy".to_string(),
                        allow_new_assignments: true,
                    },
                    AdminExitPoolMemberInput {
                        exit_endpoint_id: backup_endpoint_id,
                        weight: 100,
                        priority: 100,
                        status: "offline".to_string(),
                        allow_new_assignments: true,
                    },
                ],
            )
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

        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let initial = assigned_exit_assignment(&store, user_id, access_line_id, runtime_pool_id).await;
        assert_eq!(initial.exit_endpoint_id, primary_endpoint_id);
        assert_eq!(initial.failover_reason, "initial");

        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE
                    WHEN exit_endpoint_id = $2 THEN 'offline'
                    ELSE 'healthy'
                END,
                allow_new_assignments = TRUE
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(runtime_pool_id)
        .bind(primary_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE exit_endpoints SET enabled = FALSE WHERE id = $1")
            .bind(primary_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();

        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let failed_over = assigned_exit_assignment(&store, user_id, access_line_id, runtime_pool_id).await;
        assert_eq!(failed_over.exit_endpoint_id, backup_endpoint_id);
        assert_eq!(failed_over.failover_reason, "failover:unavailable");

        sqlx::query("UPDATE exit_endpoints SET enabled = TRUE WHERE id = $1")
            .bind(primary_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE
                    WHEN exit_endpoint_id = $2 THEN 'healthy'
                    ELSE 'offline'
                END,
                allow_new_assignments = TRUE
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(runtime_pool_id)
        .bind(primary_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let recovered = assigned_exit_assignment(&store, user_id, access_line_id, runtime_pool_id).await;
        assert_eq!(recovered.exit_endpoint_id, primary_endpoint_id);
        assert_eq!(recovered.failover_reason, "failover:unavailable");
    }

    #[tokio::test]
    async fn test_pg_subscription_filters_unavailable_access_node_and_exit_pool_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL subscription availability test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let group_id = store
            .create_admin_line_group(AdminLineGroupInput {
                name: format!("availability-group-{}", Uuid::new_v4().simple()),
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
        let access_node_id = uuid("00000000-0000-0000-0000-000000000201");
        let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");

        sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
            .bind(user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
            .bind(user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM plan_line_groups WHERE plan_id = $1 AND line_group_id <> $2")
            .bind(plan_id)
            .bind(group_id)
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
            UPDATE access_nodes
            SET status = 'unknown'
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE access_lines
            SET enabled = TRUE,
                line_group_id = $2
            WHERE id = $1
            "#,
        )
        .bind(access_line_id)
        .bind(group_id)
        .execute(store.pool())
        .await
        .unwrap();
        store
            .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
            .await
            .unwrap();
        let runtime_pool_id =
            sqlx::query_scalar::<_, Uuid>("SELECT exit_pool_id FROM access_lines WHERE id = $1")
                .bind(access_line_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'healthy', allow_new_assignments = TRUE
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(runtime_pool_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM exit_pool_members
            WHERE exit_pool_id = $1 AND exit_endpoint_id <> $2
            "#,
        )
        .bind(runtime_pool_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let baseline_yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(baseline_yaml.contains("香港 01"));
        assert!(!baseline_yaml.contains("🇭🇰 香港 01"));
        assert_eq!(
            assigned_line_ids(&store, user_id, group_id).await,
            vec![access_line_id]
        );

        sqlx::query("UPDATE access_nodes SET status = 'offline' WHERE id = $1")
            .bind(access_node_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            store.generate_subscription_yaml("demo-token").await,
            Err(DbError::Subscription(SubscriptionError::NoAvailableLines))
        ));
        assert!(assigned_line_ids(&store, user_id, group_id)
            .await
            .is_empty());
        let offline_node_subscription = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        assert!(offline_node_subscription["access_lines"]
            .as_array()
            .is_some_and(Vec::is_empty));

        sqlx::query("UPDATE access_nodes SET status = 'unknown' WHERE id = $1")
            .bind(access_node_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_exit_probe_states (
                access_node_id, exit_endpoint_id, effective_status,
                consecutive_failures, last_probe_status, updated_at
            )
            VALUES ($1, $2, 'offline', 3, 'timeout', now())
            ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
                effective_status = EXCLUDED.effective_status,
                consecutive_failures = EXCLUDED.consecutive_failures,
                last_probe_status = EXCLUDED.last_probe_status,
                updated_at = now()
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        let offline_probe_yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .expect("probe offline should not hide lines by default");
        assert!(offline_probe_yaml.contains("香港 01"));
        assert!(!offline_probe_yaml.contains("🇭🇰 香港 01"));
        let offline_probe_subscription = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        assert!(offline_probe_subscription["access_lines"]
            .as_array()
            .is_some_and(|lines| lines.len() == 1));

        store
            .update_subscription_settings_json(json!({
                "block_unhealthy_lines": true
            }))
            .await
            .unwrap();
        assert!(matches!(
            store.generate_subscription_yaml("demo-token").await,
            Err(DbError::Subscription(SubscriptionError::NoAvailableLines))
        ));
        let blocked_probe_subscription = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap();
        assert!(blocked_probe_subscription["access_lines"]
            .as_array()
            .is_some_and(Vec::is_empty));

        sqlx::query(
            r#"
            DELETE FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'offline'
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(runtime_pool_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        assert!(matches!(
            store.generate_subscription_yaml("demo-token").await,
            Err(DbError::Subscription(SubscriptionError::NoAvailableLines))
        ));

        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'healthy', allow_new_assignments = TRUE
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(runtime_pool_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
    }
