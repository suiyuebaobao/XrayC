/// 数据库测试分片 19。
// 本文件覆盖后台编辑套餐额度后的现有订阅同步。
// 管理员修改 plans.traffic_limit_bytes 时，已有订阅必须立刻更新 limit_bytes。
// 已用流量 used_bytes 不能被重置，否则会影响真实扣费账本。
// 订阅页和 Subscription-Userinfo 都必须读取同步后的总额度。
// 同步后需要清理旧的用户入口授权，避免旧授权继续生效。
// 中转节点配置必须标记 dirty，让 access-agent 下一轮刷新。
// 测试只在 DATABASE_URL 存在时执行，不连接任何真实测试服务器。
// 这里不写入外部账号、密码、Token 或真实主机信息。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_admin_plan_update_syncs_existing_subscription_quota_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL plan quota sync test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
    let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let new_limit = 44_i64 * 1024 * 1024;

    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET used_bytes = 123,
            limit_bytes = 999,
            plan_id = $2,
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
        INSERT INTO user_exit_assignments (user_id, access_line_id, exit_pool_id, exit_endpoint_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
            exit_endpoint_id = EXCLUDED.exit_endpoint_id
        "#,
    )
    .bind(user_id)
    .bind(line_id)
    .bind(exit_pool_id)
    .bind(exit_endpoint_id)
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

    let updated = store
        .update_admin_plan_json(
            plan_id,
            AdminPlanUpdate {
                traffic_limit_bytes: Some(new_limit),
                billing_multiplier: Some(1.5),
                ..AdminPlanUpdate::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated["traffic_limit_bytes"], new_limit);
    assert_eq!(updated["affected_subscriptions"], 1);

    let (used_bytes, limit_bytes) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT used_bytes, limit_bytes FROM user_subscriptions WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(used_bytes, 123);
    assert_eq!(limit_bytes, new_limit);

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

    let subscription = store.user_subscription_json_for_user(user_id).await.unwrap();
    assert_eq!(subscription["traffic"]["total_gb"], bytes_to_gb(new_limit as u64));
    let userinfo = store.subscription_userinfo_header("demo-token").await.unwrap();
    assert!(userinfo.contains(&format!("download={used_bytes}")));
    assert!(userinfo.contains(&format!("total={new_limit}")));

    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(dirty_reason, "admin_updated_plan");
}

#[tokio::test]
async fn test_pg_admin_plan_group_replace_clears_existing_runtime_assignments_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL plan group assignment cleanup test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
    let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");

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
    .bind(exit_endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();

    let replaced = store
        .replace_admin_plan_line_groups(plan_id, Vec::new())
        .await
        .unwrap();
    assert_eq!(replaced, 0);

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

    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(dirty_reason, "admin_updated_plan_line_groups");
}
