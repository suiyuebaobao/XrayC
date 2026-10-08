// 维护任务故障隔离回归：汇总失败不能回滚套餐续期，旧累计基准不能作为日志删掉。
// 仅在隔离 PostgreSQL 执行故障注入，测试结束后移除临时触发器。

#[tokio::test]
async fn test_pg_worker_rollup_failure_does_not_rollback_expiry() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    history_fixture(&store).await;
    sqlx::query("UPDATE usage_ledgers SET collected_at=now()-interval '30 days' WHERE xray_user_key='history-regression'")
        .execute(store.pool()).await.unwrap();
    sqlx::query(
        "UPDATE user_subscriptions SET expires_at=now()-interval '1 hour' WHERE user_id=$1",
    )
    .bind(uuid("00000000-0000-0000-0000-000000000001"))
    .execute(store.pool())
    .await
    .unwrap();
    let before = history_totals(&store).await;
    sqlx::query(
        "CREATE FUNCTION test_reject_rollup() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'test rollup fault'; END; $$",
    )
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER test_reject_rollup BEFORE INSERT ON usage_daily_rollups
        FOR EACH ROW EXECUTE FUNCTION test_reject_rollup()",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.run_worker_maintenance(14).await;
    // 清除故障点后才断言，以免一个失败污染其余测试。
    sqlx::query("DROP TRIGGER test_reject_rollup ON usage_daily_rollups")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION test_reject_rollup()")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(result.is_err());
    assert_eq!(history_totals(&store).await, before);
    let renewed: bool =
        sqlx::query_scalar("SELECT expires_at>now() FROM user_subscriptions WHERE user_id=$1")
            .bind(uuid("00000000-0000-0000-0000-000000000001"))
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(renewed, "汇总事务失败不得回滚已完成的套餐周期更新");
}

#[tokio::test]
async fn test_pg_worker_retains_idle_counter_baseline_for_correct_billing() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    store
        .generate_subscription_yaml("demo-token")
        .await
        .unwrap();
    let line = uuid("00000000-0000-0000-0000-000000000501");
    let user = uuid("00000000-0000-0000-0000-000000000001");
    let mut report = TrafficReport {
        access_line_id: line,
        xray_user_key: format!("u-{}@xrayc.local", user.simple()),
        uplink_total: 1000,
        downlink_total: 2000,
        collected_at: Utc::now() - Duration::days(30),
    };
    assert!(
        store
            .apply_report(report.clone())
            .await
            .unwrap()
            .baseline_only
    );
    let result = store
        .run_worker_job(WorkerMaintenanceJob::RuntimeRetention, 14)
        .await
        .unwrap();
    assert_eq!(result.pruned_traffic_snapshots, 0);
    report.uplink_total += 100;
    report.downlink_total += 200;
    report.collected_at = Utc::now();
    let result = store.apply_report(report).await.unwrap();
    assert!(!result.baseline_only, "保留基准才能计算本次增量");
    assert_eq!(result.delta_uplink, 100);
    assert_eq!(result.delta_downlink, 200);
    assert_eq!(result.billed_bytes, 300);
}

#[tokio::test]
async fn test_pg_worker_job_lock_does_not_block_other_jobs() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let mut held = store.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(20261005, 5)")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE user_subscriptions SET expires_at=now()-interval '1 hour' WHERE user_id=$1",
    )
    .bind(uuid("00000000-0000-0000-0000-000000000001"))
    .execute(store.pool())
    .await
    .unwrap();
    let skipped = store
        .run_worker_job(WorkerMaintenanceJob::UsageRollups, 14)
        .await
        .unwrap();
    assert!(!skipped.has_changes());
    let lifecycle = store
        .run_worker_job(WorkerMaintenanceJob::Lifecycle, 14)
        .await
        .unwrap();
    assert_eq!(lifecycle.expired_subscriptions, 1);
    held.rollback().await.unwrap();
}

#[tokio::test]
async fn test_pg_history_protection_migration_marks_old_rows_only() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    let tables = [
        "usage_ledgers",
        "access_line_metric_snapshots",
        "node_runtime_metrics",
        "access_user_sessions",
        "access_user_session_events",
        "access_line_probes",
        "access_exit_probes",
    ];
    // 临时表遮蔽正式测试表，直接执行本次迁移原文验证旧/新默认值，不重置公开表。
    for table in tables {
        sqlx::query(&format!(
            "CREATE TEMP TABLE {table} (id INT PRIMARY KEY, original_value TEXT) ON COMMIT DROP"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO {table} VALUES (1,'original-content')"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    sqlx::raw_sql(include_str!(
        "../../../../migrations/202610060003_preserve_existing_history.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    for table in tables {
        sqlx::query(&format!(
            "INSERT INTO {table} (id,original_value) VALUES (2,'new-content')"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
        let rows = sqlx::query_as::<_, (i32, String, bool)>(&format!(
            "SELECT id,original_value,preserve_on_cleanup FROM {table} ORDER BY id"
        ))
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (1, "original-content".to_string(), true),
                (2, "new-content".to_string(), false)
            ]
        );
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn test_pg_worker_keeps_protected_history_and_allows_date_queries() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    history_fixture(&store).await;
    sqlx::query(
        "UPDATE usage_ledgers SET preserve_on_cleanup=TRUE,
        collected_at=now()-interval '30 days',recorded_at=now()-interval '30 days'
        WHERE xray_user_key='history-regression'",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let before = history_totals(&store).await;
    let result = store
        .run_worker_job(WorkerMaintenanceJob::UsageRollups, 14)
        .await
        .unwrap();
    assert_eq!(result.pruned_usage_ledgers, 0);
    assert_eq!(history_totals(&store).await, before);
    let logs = store
        .admin_user_traffic_logs_json(
            uuid("00000000-0000-0000-0000-000000000001"),
            1,
            20,
            Some(Utc::now() - Duration::days(40)),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(logs["total"], 2);
}
