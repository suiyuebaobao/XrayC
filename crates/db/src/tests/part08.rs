/// 数据库测试分片 08。
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
    async fn test_pg_admin_plan_crud_soft_deletes_and_migrates_subscriptions_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL admin plan CRUD test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let default_plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");

        let created = store
            .create_admin_plan_json(AdminPlanInput {
                name: "CRUD 测试套餐".to_string(),
                traffic_limit_bytes: 20 * 1024 * 1024,
                rate_limit_bps: 0,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
                billing_multiplier: 1.25,
                enabled: true,
                price_cents: 990,
                currency: "usdt".to_string(),
                duration_days: 15,
                sort_weight: 5,
            })
            .await
            .unwrap();
        let plan_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
        assert_eq!(created["currency"], "USDT");

        store
            .replace_admin_plan_line_groups(
                plan_id,
                vec![AdminPlanLineGroupInput {
                    line_group_id,
                    billing_multiplier: None,
                }],
            )
            .await
            .unwrap();
        let admin_plans = store.admin_plans_json().await.unwrap();
        let created_plan = admin_plans["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plan| plan["id"] == plan_id.to_string())
            .unwrap();
        assert_eq!(created_plan["line_groups"][0]["line_group_id"], line_group_id.to_string());
        assert!(store
            .list_plans()
            .await
            .unwrap()
            .iter()
            .any(|plan| plan.id == plan_id));

        store
            .update_admin_plan_json(
                plan_id,
                AdminPlanUpdate {
                    name: Some("CRUD 测试套餐停用".to_string()),
                    enabled: Some(false),
                    ..AdminPlanUpdate::default()
                },
            )
            .await
            .unwrap();
        assert!(!store
            .list_plans()
            .await
            .unwrap()
            .iter()
            .any(|plan| plan.id == plan_id));

        let default_disable = store
            .update_admin_plan_json(
                default_plan_id,
                AdminPlanUpdate {
                    enabled: Some(false),
                    ..AdminPlanUpdate::default()
                },
            )
            .await;
        assert!(default_disable.is_err());

        store
            .update_admin_plan_json(
                plan_id,
                AdminPlanUpdate {
                    enabled: Some(true),
                    ..AdminPlanUpdate::default()
                },
            )
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE user_subscriptions
            SET plan_id = $2,
                used_bytes = 123,
                limit_bytes = 999,
                active = TRUE
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .bind(plan_id)
        .execute(store.pool())
        .await
        .unwrap();
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
            INSERT INTO user_exit_assignments (
                user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
            )
            VALUES ($1, $2, $3, $4, 'test')
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let delete_default = store.delete_admin_plan_json(default_plan_id).await;
        assert!(delete_default.is_err());
        let deleted = store.delete_admin_plan_json(plan_id).await.unwrap();
        assert_eq!(deleted["deleted"], true);
        assert_eq!(deleted["affected_subscriptions"], 1);

        let subscription = sqlx::query_as::<_, SubscriptionRow>(
            r#"
            SELECT user_id, plan_id, active, expires_at, used_bytes,
                   limit_bytes
            FROM user_subscriptions
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(subscription.plan_id, default_plan_id);
        assert_eq!(subscription.used_bytes, 0);
        assert!(!store.admin_plans_json().await.unwrap()["plans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plan| plan["id"] == plan_id.to_string()));
        let assignment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_access_line_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(assignment_count, 0);
        let exit_assignment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_exit_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(exit_assignment_count, 0);
        let dirty_reason = sqlx::query_scalar::<_, String>(
            "SELECT config_dirty_reason FROM access_nodes WHERE id = $1",
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(dirty_reason, "admin_deleted_plan");
    }

    #[tokio::test]
    async fn test_pg_user_exit_assignments_follow_priority_and_failover_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL exit assignment test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let access_node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let exit_resource_id = uuid("00000000-0000-0000-0000-000000000301");
        let first_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let second_endpoint_id = uuid("00000000-0000-0000-0000-000000000303");

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
            .bind(line_group_id)
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
        .bind(line_group_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM line_group_lines
            WHERE line_group_id = $1 AND access_line_id <> $2
            "#,
        )
        .bind(line_group_id)
        .bind(access_line_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id IN ($2, $3)
            "#,
        )
        .bind(access_node_id)
        .bind(first_endpoint_id)
        .bind(second_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE access_lines SET exit_endpoint_id = NULL WHERE id = $1")
            .bind(access_line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'healthy', allow_new_assignments = TRUE, weight = 100, priority = 100
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(exit_pool_id)
        .bind(first_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, outbound_type, host, port, outbound_config, enabled
            )
            VALUES ($1, $2, 'socks'::endpoint_type, '198.51.100.11', 1081, '{}'::jsonb, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(second_endpoint_id)
        .bind(exit_resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status, priority, allow_new_assignments
            )
            VALUES ($1, $2, 200, 'healthy', 100, TRUE)
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                weight = EXCLUDED.weight,
                status = EXCLUDED.status,
                priority = EXCLUDED.priority,
                allow_new_assignments = EXCLUDED.allow_new_assignments
            "#,
        )
        .bind(exit_pool_id)
        .bind(second_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM exit_pool_members
            WHERE exit_pool_id = $1 AND exit_endpoint_id NOT IN ($2, $3)
            "#,
        )
        .bind(exit_pool_id)
        .bind(first_endpoint_id)
        .bind(second_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml.contains("access.example.test"));
        assert!(!yaml.contains("198.51.100.10"));
        assert!(!yaml.contains("198.51.100.11"));

        let initial = assigned_exit_assignment(&store, user_id, access_line_id, exit_pool_id).await;
        assert_eq!(initial.failover_reason, "initial");
        assert_eq!(initial.exit_endpoint_id, second_endpoint_id);
        let data = store.load_store_data().await.unwrap();
        let pools = exit_pools_json(&data);
        let pool_summary = pools["exit_pools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pool| pool["uuid"] == exit_pool_id.to_string())
            .unwrap();
        let real_assignment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_exit_assignments WHERE exit_pool_id = $1",
        )
        .bind(exit_pool_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(pool_summary["active_assignments"], real_assignment_count);

        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let repeated =
            assigned_exit_assignment(&store, user_id, access_line_id, exit_pool_id).await;
        assert_eq!(repeated.exit_endpoint_id, initial.exit_endpoint_id);
        assert_eq!(repeated.failover_reason, "initial");
        assert_eq!(repeated.assigned_at, initial.assigned_at);

        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'degraded'
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(exit_pool_id)
        .bind(initial.exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let degraded =
            assigned_exit_assignment(&store, user_id, access_line_id, exit_pool_id).await;
        assert_eq!(degraded.exit_endpoint_id, first_endpoint_id);
        assert_eq!(degraded.failover_reason, "failover:unavailable");

        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'healthy'
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(exit_pool_id)
        .bind(initial.exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let restored =
            assigned_exit_assignment(&store, user_id, access_line_id, exit_pool_id).await;
        assert_eq!(restored.exit_endpoint_id, second_endpoint_id);
        assert_eq!(restored.failover_reason, "failover:unavailable");

        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'offline'
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(exit_pool_id)
        .bind(initial.exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        let failover =
            assigned_exit_assignment(&store, user_id, access_line_id, exit_pool_id).await;
        assert_ne!(failover.exit_endpoint_id, initial.exit_endpoint_id);
        assert_eq!(failover.failover_reason, "failover:unavailable");
    }
