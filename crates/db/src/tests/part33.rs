// 数据库测试分片 33。
// 本文件覆盖基础套餐自动重置和付费套餐到期回落逻辑。
// Worker 维护任务到期处理不再直接停用所有订阅。
// 基础套餐到期后必须清零已用流量并开启下一个周期。
// 付费套餐到期后必须切回基础套餐并清零已用流量。
// 付费套餐购买叠加逻辑不在本文件重复验证。
// 测试只使用 demo seed 和临时套餐，不包含真实服务器信息。
// PostgreSQL 用例仅在 DATABASE_URL 存在时运行。
// 文件保持短小，避免影响源码长度门禁。
// 本头部满足前十行中文注释约束。

struct ExpiredRuntimeAssignmentSeed {
    user_id: Uuid,
    plan_id: Uuid,
    line_group_id: Uuid,
    line_id: Uuid,
    exit_pool_id: Uuid,
    exit_endpoint_id: Uuid,
    used_bytes: i64,
    limit_bytes: i64,
}

struct RetentionLedgerSeed<'a> {
    user_id: Uuid,
    access_line_id: Uuid,
    exit_endpoint_id: Uuid,
    xray_user_key: &'a str,
    traffic_source: &'a str,
    collected_at: chrono::DateTime<Utc>,
    delta_uplink: i64,
    delta_downlink: i64,
    billed_bytes: i64,
}

#[tokio::test]
async fn test_pg_expired_default_subscription_auto_resets_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL default subscription reset test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let default_plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
    let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let default_limit = 10_i64 * 1024 * 1024 * 1024;

    seed_expired_subscription_with_runtime_assignments(&store, ExpiredRuntimeAssignmentSeed {
        user_id,
        plan_id: default_plan_id,
        line_group_id,
        line_id,
        exit_pool_id,
        exit_endpoint_id,
        used_bytes: 9_i64 * 1024 * 1024 * 1024,
        limit_bytes: default_limit,
    })
    .await;

    let result = store.run_worker_maintenance(1).await.unwrap();
    assert_eq!(result.expired_subscriptions, 1);

    assert_subscription_reset_to_default(&store, user_id, default_plan_id, default_limit).await;
    assert_runtime_assignments_cleared(&store, user_id).await;
    assert_node_dirty_reason(&store, node_id, "worker_expired_subscription").await;
}

#[tokio::test]
async fn test_pg_expired_paid_subscription_falls_back_to_default_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL paid subscription fallback test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let default_plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
    let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let default_limit = 10_i64 * 1024 * 1024 * 1024;
    let paid_plan_id = Uuid::parse_str(
        store
            .create_admin_plan_json(AdminPlanInput {
                name: "paid-expired-fallback-plan".to_string(),
                traffic_limit_bytes: 100_i64 * 1024 * 1024 * 1024,
                rate_limit_bps: 0,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
                billing_multiplier: 1.0,
                enabled: true,
                price_cents: 999,
                currency: "USDT".to_string(),
                duration_days: 30,
                sort_weight: 100,
            })
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    seed_expired_subscription_with_runtime_assignments(&store, ExpiredRuntimeAssignmentSeed {
        user_id,
        plan_id: paid_plan_id,
        line_group_id,
        line_id,
        exit_pool_id,
        exit_endpoint_id,
        used_bytes: 100_i64 * 1024 * 1024 * 1024,
        limit_bytes: 100_i64 * 1024 * 1024 * 1024,
    })
    .await;

    let result = store.run_worker_maintenance(1).await.unwrap();
    assert_eq!(result.expired_subscriptions, 1);

    assert_subscription_reset_to_default(&store, user_id, default_plan_id, default_limit).await;
    assert_runtime_assignments_cleared(&store, user_id).await;
    assert_node_dirty_reason(&store, node_id, "worker_expired_subscription").await;
}

async fn seed_expired_subscription_with_runtime_assignments(
    store: &PgStore,
    seed: ExpiredRuntimeAssignmentSeed,
) {
    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET plan_id = $2,
            active = TRUE,
            expires_at = now() - interval '1 hour',
            used_bytes = $3,
            limit_bytes = $4,
            updated_at = now()
        WHERE user_id = $1
        "#,
    )
    .bind(seed.user_id)
    .bind(seed.plan_id)
    .bind(seed.used_bytes)
    .bind(seed.limit_bytes)
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
    .bind(seed.user_id)
    .bind(seed.line_group_id)
    .bind(seed.line_id)
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
    .bind(seed.user_id)
    .bind(seed.line_id)
    .bind(seed.exit_pool_id)
    .bind(seed.exit_endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn assert_subscription_reset_to_default(
    store: &PgStore,
    user_id: Uuid,
    default_plan_id: Uuid,
    default_limit: i64,
) {
    let (plan_id, active, expires_at, used_bytes, limit_bytes) = sqlx::query_as::<
        _,
        (Uuid, bool, chrono::DateTime<chrono::Utc>, i64, i64),
    >(
        r#"
        SELECT plan_id, active, expires_at, used_bytes, limit_bytes
        FROM user_subscriptions
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(plan_id, default_plan_id);
    assert!(active);
    assert!(expires_at > Utc::now());
    assert_eq!(used_bytes, 0);
    assert_eq!(limit_bytes, default_limit);
}

async fn assert_runtime_assignments_cleared(store: &PgStore, user_id: Uuid) {
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
}

async fn assert_node_dirty_reason(store: &PgStore, node_id: Uuid, expected_reason: &str) {
    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(dirty_reason, expected_reason);
}

#[tokio::test]
async fn test_pg_worker_rolls_up_and_prunes_old_traffic_logs_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL traffic log retention test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let access_node_id = uuid("00000000-0000-0000-0000-000000000201");
    let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
    let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let xray_user_key = "traffic-retention-user@example.test";
    let old_time = Utc::now() - Duration::days(2);
    let recent_time = Utc::now() - Duration::hours(1);
    let used_bytes_before = 123_456_i64;

    store
        .update_access_operations_settings_json(json!({
            "traffic_log_retention": {
                "detail_retention_days": 1,
                "prune_enabled": true,
                "delete_batch_size": 100
            }
        }))
        .await
        .unwrap();

    sqlx::query("DELETE FROM usage_ledgers WHERE xray_user_key = $1")
        .bind(xray_user_key)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM usage_daily_rollups WHERE xray_user_key = $1")
        .bind(xray_user_key)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM usage_hourly_rollups WHERE xray_user_key = $1")
        .bind(xray_user_key)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE users
        SET xray_user_key = $2
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .bind(xray_user_key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET active = TRUE,
            expires_at = now() + interval '30 days',
            used_bytes = $2,
            updated_at = now()
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .bind(used_bytes_before)
    .execute(store.pool())
    .await
    .unwrap();
    insert_usage_ledger_for_retention_test(&store, RetentionLedgerSeed {
        user_id,
        access_line_id,
        exit_endpoint_id,
        xray_user_key,
        traffic_source: "access_line",
        collected_at: old_time,
        delta_uplink: 100,
        delta_downlink: 400,
        billed_bytes: 750,
    })
    .await;
    insert_usage_ledger_for_retention_test(&store, RetentionLedgerSeed {
        user_id,
        access_line_id,
        exit_endpoint_id,
        xray_user_key,
        traffic_source: "internal",
        collected_at: old_time + Duration::minutes(10),
        delta_uplink: 1000,
        delta_downlink: 1000,
        billed_bytes: 3000,
    })
    .await;
    insert_usage_ledger_for_retention_test(&store, RetentionLedgerSeed {
        user_id,
        access_line_id,
        exit_endpoint_id,
        xray_user_key,
        traffic_source: "access_line",
        collected_at: recent_time,
        delta_uplink: 10,
        delta_downlink: 20,
        billed_bytes: 30,
    })
    .await;

    let result = store.run_worker_maintenance(14).await.unwrap();

    assert_eq!(result.pruned_usage_ledgers, 2);
    let remaining_ledgers = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM usage_ledgers WHERE xray_user_key = $1",
    )
    .bind(xray_user_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining_ledgers, 1);

    let (rollup_count, rollup_uplink, rollup_downlink, rollup_total, rollup_billed) = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r#"
        SELECT COALESCE(SUM(ledger_count), 0)::BIGINT,
               COALESCE(SUM(delta_uplink), 0)::BIGINT,
               COALESCE(SUM(delta_downlink), 0)::BIGINT,
               COALESCE(SUM(delta_total), 0)::BIGINT,
               COALESCE(SUM(billed_bytes), 0)::BIGINT
        FROM usage_daily_rollups
        WHERE xray_user_key = $1
          AND traffic_source = 'access_line'
          AND rollup_date = $2
          AND user_id = $3
          AND access_line_id = $4
          AND access_node_id = $5
          AND exit_endpoint_id = $6
        "#,
    )
    .bind(xray_user_key)
    .bind(old_time.date_naive())
    .bind(user_id)
    .bind(access_line_id)
    .bind(access_node_id)
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(rollup_count, 1);
    assert_eq!(rollup_uplink, 100);
    assert_eq!(rollup_downlink, 400);
    assert_eq!(rollup_total, 500);
    assert_eq!(rollup_billed, 750);

    let (hourly_count, hourly_total, hourly_billed) = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT COALESCE(SUM(ledger_count), 0)::BIGINT,
               COALESCE(SUM(delta_total), 0)::BIGINT,
               COALESCE(SUM(billed_bytes), 0)::BIGINT
        FROM usage_hourly_rollups
        WHERE xray_user_key = $1
          AND traffic_source = 'access_line'
          AND hour_start = date_trunc('hour', $2::TIMESTAMPTZ)
          AND access_line_id = $3
          AND access_node_id = $4
          AND exit_endpoint_id = $5
        "#,
    )
    .bind(xray_user_key)
    .bind(old_time)
    .bind(access_line_id)
    .bind(access_node_id)
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(hourly_count, 1);
    assert_eq!(hourly_total, 500);
    assert_eq!(hourly_billed, 750);

    let used_bytes_after = sqlx::query_scalar::<_, i64>(
        "SELECT used_bytes FROM user_subscriptions WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(used_bytes_after, used_bytes_before);

    let ranking = store.operations_ledger_ranking_json(10).await.unwrap();
    let totals = &ranking["totals"];
    assert_eq!(totals["ledger_count"], json!(2));
    assert_eq!(totals["billed_bytes"], json!(780));
    let retained_line = ranking["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["access_line_id"] == json!(access_line_id))
        .expect("retained traffic line ranking item");
    assert_eq!(retained_line["ledger_count"], json!(2));
    assert_eq!(retained_line["billed_bytes"], json!(780));

    let metrics = store.access_line_metrics_json(access_line_id).await.unwrap();
    assert_eq!(metrics["ledger_summary"]["ledger_count"], json!(2));
    assert_eq!(metrics["ledger_summary"]["billed_bytes"], json!(780));
    assert_eq!(metrics["traffic_summary"]["ledger_count"], json!(2));

    let summary = store.operations_summary_json().await.unwrap();
    let line_item = summary["traffic_health"]["line_items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["access_line_id"] == json!(access_line_id))
        .expect("traffic health line item");
    assert_eq!(line_item["total_real_bytes"], json!(530));
    assert_eq!(line_item["total_billed_bytes"], json!(780));
    if old_time.format("%Y-%m").to_string() == Utc::now().format("%Y-%m").to_string() {
        assert_eq!(line_item["peak_hour_real_bytes"], json!(500));
        assert_eq!(
            line_item["chart"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|point| point["window_start"].is_string())
                .count(),
            2
        );
    }

    let second_result = store.run_worker_maintenance(14).await.unwrap();
    assert_eq!(second_result.pruned_usage_ledgers, 0);
    let access_rollup_billed_after_second = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(billed_bytes), 0)::BIGINT
        FROM usage_daily_rollups
        WHERE xray_user_key = $1
          AND traffic_source = 'access_line'
        "#,
    )
    .bind(xray_user_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(access_rollup_billed_after_second, 750);
}

async fn insert_usage_ledger_for_retention_test(
    store: &PgStore,
    seed: RetentionLedgerSeed<'_>,
) {
    let billed_uplink =
        seed.billed_bytes * seed.delta_uplink / (seed.delta_uplink + seed.delta_downlink);
    sqlx::query(
        r#"
        INSERT INTO usage_ledgers (
            access_line_id, user_id, xray_user_key, traffic_source,
            delta_uplink, delta_downlink, delta_total, billing_multiplier,
            billed_uplink, billed_downlink, billed_bytes,
            collected_at, recorded_at, exit_endpoint_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 1.500, $8, $9, $10, $11, $11, $12)
        "#,
    )
    .bind(seed.access_line_id)
    .bind(seed.user_id)
    .bind(seed.xray_user_key)
    .bind(seed.traffic_source)
    .bind(seed.delta_uplink)
    .bind(seed.delta_downlink)
    .bind(seed.delta_uplink + seed.delta_downlink)
    .bind(billed_uplink)
    .bind(seed.billed_bytes - billed_uplink)
    .bind(seed.billed_bytes)
    .bind(seed.collected_at)
    .bind(seed.exit_endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();
}
