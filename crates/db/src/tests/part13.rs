/// 数据库测试分片 13。
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
    async fn test_pg_worker_maintenance_expires_and_prunes_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL worker maintenance test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let old_time = Utc::now() - Duration::days(3);
        let order_no = format!("worker-test-{}", Uuid::new_v4().simple());
        let refresh_hash = format!("worker-test-{}", Uuid::new_v4().simple());
        let guard_key = login_guard_key("worker-maintenance@example.test");

        sqlx::query("DELETE FROM orders WHERE order_no LIKE 'worker-test-%'")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM auth_challenges WHERE target_hash = 'worker-test-target'")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM login_guard_states WHERE guard_key = $1")
            .bind(&guard_key)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_line_metric_snapshots WHERE access_line_id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_traffic_snapshots WHERE access_line_id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_user_sessions WHERE access_node_id = $1")
            .bind(node_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_user_session_events WHERE access_node_id = $1")
            .bind(node_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_line_probes WHERE access_line_id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_exit_probes WHERE access_node_id = $1")
            .bind(node_id)
            .execute(store.pool())
            .await
            .unwrap();

        sqlx::query(
            r#"
            UPDATE user_subscriptions
            SET active = TRUE,
                expires_at = now() - interval '1 minute',
                updated_at = now()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
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
        .bind(group_id)
        .bind(line_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO user_exit_assignments (
                user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
            )
            VALUES ($1, $2, $3, $4, 'worker-test')
            ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                failover_reason = EXCLUDED.failover_reason
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE access_nodes SET config_dirty = FALSE, config_dirty_reason = '' WHERE id = $1",
        )
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO orders (
                order_no, user_id, plan_id, amount_cents, currency, status,
                payment_address, expires_at
            )
            VALUES ($1, $2, $3, 100, 'USDT', 'pending', 'worker-test', now() - interval '1 minute')
            "#,
        )
        .bind(&order_no)
        .bind(user_id)
        .bind(plan_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
            VALUES ($1, $2, now() - interval '1 minute')
            "#,
        )
        .bind(user_id)
        .bind(&refresh_hash)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO auth_challenges (scene, target_hash, code_hash, expires_at)
            VALUES ('register', 'worker-test-target', 'worker-test-code', now() - interval '1 minute')
            "#,
        )
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO login_guard_states (
                guard_key, failure_count, locked_until, expires_at, updated_at
            )
            VALUES ($1, 1, NULL, now() - interval '1 minute', now())
            "#,
        )
        .bind(&guard_key)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_line_metric_snapshots (
                access_line_id, online_users, active_connections, unique_client_ips,
                uplink_rate_bps, downlink_rate_bps, collected_at
            )
            VALUES ($1, 1, 1, 1, 1, 1, $2)
            "#,
        )
        .bind(line_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_traffic_snapshots (
                access_node_id, access_line_id, xray_user_key,
                uplink_total, downlink_total, collected_at
            )
            VALUES ($1, $2, 'worker-test-user', 1, 1, $3)
            "#,
        )
        .bind(node_id)
        .bind(line_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_user_sessions (
                access_node_id, access_line_id, xray_user_key, client_ip_hash,
                started_at, last_seen_at
            )
            VALUES ($1, $2, 'worker-test-user', 'worker-test-client', $3, $3)
            "#,
        )
        .bind(node_id)
        .bind(line_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_user_session_events (
                access_node_id, access_line_id, user_id, xray_user_key,
                client_ip, client_ip_hash, active_connection_count,
                status, observed_at, created_at
            )
            VALUES ($1, $2, $3, 'worker-test-user', '127.0.0.1',
                    'worker-test-client', 1, 'online', $4, $4)
            "#,
        )
        .bind(node_id)
        .bind(line_id)
        .bind(user_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_line_probes (access_line_id, status, latency_ms, probed_at)
            VALUES ($1, 'healthy', 1, $2)
            "#,
        )
        .bind(line_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_exit_probes (
                access_node_id, exit_endpoint_id, status, latency_ms, probed_at
            )
            VALUES ($1, $2, 'healthy', 1, $3)
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .bind(old_time)
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.run_worker_maintenance(1).await.unwrap();
        assert!(result.expired_subscriptions >= 1);
        assert!(result.expired_orders >= 1);
        assert!(result.expired_refresh_tokens >= 1);
        assert!(result.expired_auth_challenges >= 1);
        assert!(result.expired_login_guards >= 1);
        assert!(result.queued_exit_probe_tasks >= 1);
        assert!(result.pruned_metric_snapshots >= 1);
        assert!(result.pruned_traffic_snapshots >= 1);
        assert!(result.pruned_user_sessions >= 1);
        assert!(result.pruned_user_session_events >= 1);
        assert!(result.pruned_line_probes >= 1);
        assert!(result.pruned_exit_probes >= 1);

        let (active, used_bytes, limit_bytes, expires_at) = sqlx::query_as::<
            _,
            (bool, i64, i64, chrono::DateTime<chrono::Utc>),
        >(
            "SELECT active, used_bytes, limit_bytes, expires_at FROM user_subscriptions WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(active);
        assert_eq!(used_bytes, 0);
        assert_eq!(limit_bytes, 10_i64 * 1024 * 1024 * 1024);
        assert!(expires_at > Utc::now());
        let access_assignments = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM user_access_line_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(access_assignments, 0);
        let exit_assignments = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM user_exit_assignments WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(exit_assignments, 0);
        let dirty_reason = sqlx::query_scalar::<_, String>(
            "SELECT config_dirty_reason FROM access_nodes WHERE id = $1",
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(dirty_reason, "worker_expired_subscription");
        let order_status =
            sqlx::query_scalar::<_, String>("SELECT status FROM orders WHERE order_no = $1")
                .bind(&order_no)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(order_status, "expired");
        let refresh_left = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM refresh_tokens WHERE token_hash = $1",
        )
        .bind(&refresh_hash)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(refresh_left, 0);
        let challenge_left = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM auth_challenges WHERE target_hash = 'worker-test-target'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(challenge_left, 0);
        let guard_left = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM login_guard_states WHERE guard_key = $1",
        )
        .bind(&guard_key)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(guard_left, 0);
        let queued_probe_left = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::BIGINT
            FROM access_exit_probes
            WHERE access_node_id = $1
              AND exit_endpoint_id = $2
              AND status = 'queued'
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(queued_probe_left >= 1);
    }
