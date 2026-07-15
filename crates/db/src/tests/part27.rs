/// 数据库测试分片 27。
// 本文件覆盖线路组倍率计费的 PostgreSQL 闭环。
// 业务规则是套餐基础倍率和套餐授权分组倍率都可设置。
// 用户流量经过某条线路时，应按两者中的最高倍率扣费。
// 测试只使用演示数据和保留地址，不写入真实服务器信息。
// 该分片先验证计费结果，再检查账本记录的倍率。
// 所有数据库测试仍通过 DATABASE_URL 控制是否运行。
// 维护时保持文件短小，避免超过仓库单文件行数规则。
// 注释必须中文，方便后续开发者理解业务意图。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_plan_group_multiplier_migration_copies_existing_group_multiplier() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL multiplier migration test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    let mut conn = store.pool().acquire().await.unwrap();
    let plan_id = uuid::Uuid::new_v4();
    let line_group_id = uuid::Uuid::new_v4();

    sqlx::query(
        r#"
        CREATE TEMP TABLE line_groups (
            id UUID PRIMARY KEY,
            billing_multiplier NUMERIC(10, 3) NOT NULL DEFAULT 1.000
        ) ON COMMIT PRESERVE ROWS
        "#,
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        r#"
        CREATE TEMP TABLE plan_line_groups (
            plan_id UUID NOT NULL,
            line_group_id UUID NOT NULL,
            PRIMARY KEY (plan_id, line_group_id)
        ) ON COMMIT PRESERVE ROWS
        "#,
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO line_groups (id, billing_multiplier) VALUES ($1, 3.000)")
        .bind(line_group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO plan_line_groups (plan_id, line_group_id) VALUES ($1, $2)")
        .bind(plan_id)
        .bind(line_group_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let migration_sql =
        std::fs::read_to_string("../../migrations/202606010008_plan_line_group_billing_multiplier.sql")
            .unwrap();
    sqlx::raw_sql(&migration_sql)
        .execute(&mut *conn)
        .await
        .unwrap();

    let multiplier = sqlx::query_scalar::<_, f64>(
        "SELECT billing_multiplier::float8 FROM plan_line_groups WHERE plan_id = $1",
    )
    .bind(plan_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(multiplier, 3.0);
}

#[tokio::test]
async fn test_pg_traffic_bills_by_highest_plan_group_multiplier() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL line group multiplier test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let data = store.load_store_data().await.unwrap();
    let plan_id = *data.plans.keys().next().unwrap();
    let line = data.access_lines.values().next().unwrap();
    let user_id = data.tokens.get("demo-token").unwrap().user_id;
    let user_key = data.users.get(&user_id).unwrap().xray_user_key.clone();
    let line_group_id = line.line_group_id.unwrap();

    sqlx::query("UPDATE plans SET billing_multiplier = 1.250 WHERE id = $1")
        .bind(plan_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE plan_line_groups SET billing_multiplier = 3.000 WHERE plan_id = $1 AND line_group_id = $2",
    )
    .bind(plan_id)
    .bind(line_group_id)
    .execute(store.pool())
    .await
    .unwrap();
    let other_plan_id = sqlx::query_scalar::<_, uuid::Uuid>(
        r#"
        INSERT INTO plans (
            name, traffic_limit_bytes, billing_multiplier,
            enabled, price_cents, currency, duration_days, sort_weight
        )
        VALUES ('倍率隔离套餐', 1073741824, 1.000, TRUE, 0, 'USDT', 30, 999)
        RETURNING id
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
        VALUES ($1, $2, 5.000)
        "#,
    )
    .bind(other_plan_id)
    .bind(line_group_id)
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
    .bind(line.id)
    .execute(store.pool())
    .await
    .unwrap();

    let first = TrafficReport {
        access_line_id: line.id,
        xray_user_key: user_key.clone(),
        uplink_total: 10,
        downlink_total: 20,
        collected_at: Utc::now(),
    };
    let mut second = first.clone();
    second.uplink_total = 110;
    second.downlink_total = 220;
    second.collected_at += Duration::seconds(10);

    assert!(store.apply_report(first).await.unwrap().baseline_only);
    let result = store.apply_report(second).await.unwrap();

    assert_eq!(result.billed_bytes, 900);
    let (billing_multiplier, billed_bytes) = sqlx::query_as::<_, (f64, i64)>(
        r#"
        SELECT billing_multiplier::float8, billed_bytes
        FROM usage_ledgers
        WHERE access_line_id = $1 AND xray_user_key = $2
        ORDER BY collected_at DESC
        LIMIT 1
        "#,
    )
    .bind(line.id)
    .bind(user_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(billing_multiplier, 3.0);
    assert_eq!(billed_bytes, 900);
}
