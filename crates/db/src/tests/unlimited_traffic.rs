// 不限流量的数据库回归测试。
// 只在隔离测试库运行，保留零额度的原有含义。
// 验证超过既有配额后仍可订阅和计费，有限额仍耗尽撤销。
#[tokio::test]
async fn test_pg_unlimited_traffic_preserves_accounting_and_finite_quota() {
    let Ok(url) = std::env::var("DATABASE_URL") else { return; };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let data = store.load_store_data().await.unwrap();
    let plan_id = data.subscriptions[&user_id].plan_id;
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let key = data.users[&user_id].xray_user_key.clone();
    store.update_admin_plan_json(plan_id, AdminPlanUpdate {
        traffic_limit_bytes: Some(-1), ..Default::default()
    }).await.unwrap();
    assert!(store.update_admin_plan_json(plan_id, AdminPlanUpdate {
        traffic_limit_bytes: Some(-2), ..Default::default()
    }).await.is_err());
    sqlx::query("UPDATE user_subscriptions SET used_bytes = 1099511627776 WHERE user_id = $1")
        .bind(user_id).execute(store.pool()).await.unwrap();
    let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
    assert!(yaml.contains("access.example.test"));
    assert!(store.subscription_userinfo_header("demo-token").await.unwrap().contains("total=0;"));
    assert_eq!(store.admin_user_json(user_id).await.unwrap()["traffic_unlimited"], true);
    assert_eq!(store.load_store_data().await.unwrap().subscriptions[&user_id].remaining_bytes(), u64::MAX);
    sqlx::query("DELETE FROM access_traffic_snapshots WHERE access_line_id=$1")
        .bind(line_id).execute(store.pool()).await.unwrap();
    let mut report = TrafficReport {
        access_line_id: line_id, xray_user_key: key,
        uplink_total: 10, downlink_total: 20, collected_at: Utc::now(),
    };
    assert!(store.apply_report(report.clone()).await.unwrap().baseline_only);
    report.uplink_total += 100;
    report.downlink_total += 200;
    report.collected_at += Duration::seconds(10);
    let result = store.apply_report(report.clone()).await.unwrap();
    assert_eq!(result.billed_bytes, 300);
    assert!(!result.config_refresh_required);
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_subscriptions WHERE user_id=$1")
        .bind(user_id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(used, 1099511627776 + 300);
    store.update_admin_plan_json(plan_id, AdminPlanUpdate {
        traffic_limit_bytes: Some(100), ..Default::default()
    }).await.unwrap();
    sqlx::query("UPDATE user_subscriptions SET used_bytes=0 WHERE user_id=$1")
        .bind(user_id).execute(store.pool()).await.unwrap();
    store.generate_subscription_yaml("demo-token").await.unwrap();
    report.downlink_total += 200;
    report.collected_at += Duration::seconds(10);
    let result = store.apply_report(report).await.unwrap();
    assert_eq!(result.billed_bytes, 100);
    assert!(result.config_refresh_required);
    assert_eq!(store.load_store_data().await.unwrap().subscriptions[&user_id].remaining_bytes(), 0);
}
