// 数据库测试分片 04。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    struct RuntimeReportTestContext {
        store: PgStore,
        node_id: Uuid,
        line_id: Uuid,
        user_id: Uuid,
        exit_pool_id: Uuid,
        endpoint_id: Uuid,
        failover_endpoint_id: Uuid,
        user_key: &'static str,
    }

    async fn setup_runtime_report_test_context(database_url: &str) -> RuntimeReportTestContext {
        let store = PgStore::connect(database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let exit_resource_id = uuid("00000000-0000-0000-0000-000000000301");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let failover_endpoint_id = Uuid::new_v4();
        let user_key = "u-00000000000000000000000000000001@xrayc.local";

        store
            .update_subscription_settings_json(json!({
                "block_unhealthy_lines": true
            }))
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_line_metric_snapshots")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_user_sessions")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM usage_ledgers WHERE access_line_id = $1 AND user_id = $2")
            .bind(line_id)
            .bind(user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "DELETE FROM access_traffic_snapshots WHERE access_line_id = $1 AND xray_user_key = $2",
        )
        .bind(line_id)
        .bind(user_key)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE user_subscriptions
            SET used_bytes = 0,
                updated_at = now()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM access_line_probes WHERE access_line_id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE access_lines SET exit_endpoint_id = NULL WHERE id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "DELETE FROM access_exit_probes WHERE access_node_id = $1 AND exit_endpoint_id = $2",
        )
        .bind(node_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM access_exit_probe_states WHERE access_node_id = $1 AND exit_endpoint_id = $2",
        )
        .bind(node_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM exit_endpoints WHERE name = 'runtime-failover-socks'")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, name, outbound_type, host, port,
                outbound_config, stream_config, probe_config, enabled
            )
            VALUES ($1, $2, 'runtime-failover-socks', 'socks', '198.51.100.99', 1080,
                    '{}'::jsonb, '{}'::jsonb, '{}'::jsonb, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(failover_endpoint_id)
        .bind(exit_resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status, priority,
                allow_new_assignments
            )
            VALUES ($1, $2, 100, 'healthy', 100, TRUE)
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                status = 'healthy',
                allow_new_assignments = TRUE
            "#,
        )
        .bind(exit_pool_id)
        .bind(failover_endpoint_id)
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
        .bind(endpoint_id)
        .bind(failover_endpoint_id)
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
        .bind(line_id)
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
                user_id, access_line_id, exit_pool_id, exit_endpoint_id,
                failover_reason
            )
            VALUES ($1, $2, $3, $4, 'initial')
            ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                failover_reason = EXCLUDED.failover_reason
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(exit_pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        RuntimeReportTestContext {
            store,
            node_id,
            line_id,
            user_id,
            exit_pool_id,
            endpoint_id,
            failover_endpoint_id,
            user_key,
        }
    }

    #[tokio::test]
    async fn test_pg_agent_runtime_reports_update_operations_read_models_when_database_url_is_set()
    {

        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL runtime report test");
            return;
        };
        let _guard = pg_test_guard().await;
        let ctx = setup_runtime_report_test_context(&database_url).await;
        assert_runtime_metrics_and_ledger(&ctx).await;
        assert_runtime_probe_failover_and_recovery(&ctx).await;
        assert_runtime_read_models_and_cleanup(&ctx).await;
    }
