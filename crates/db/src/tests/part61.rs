// 数据库测试分片 61:install 自带管理员 + 默认套餐 bootstrap 幂等性。
// 规则:无管理员时 bootstrap_admin 建账号 admin(is_admin=TRUE、真 Argon2)返回 true;
// 已存在管理员再次调用返回 false 不重复建。真实 PostgreSQL,缺 DATABASE_URL 跳过。
// 共享 xrayc_test 库由 pg_test_guard 串行化且其它分片会 seed 管理员,故先把已有管理员
// 临时降级为非管理员(UPDATE 不触发 RESTRICT 外键)造无管理员态,断言后复原以免污染他用例。
// 父 tests 模块 include! 引入,串行(--test-threads=1)。不在此保存真实地址或凭据。本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_bootstrap_admin_creates_once_idempotent() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 共享库可能已被其它分片 seed 出管理员;临时降级造"无管理员"态(避免删除触发 usage_ledgers 等 RESTRICT 外键)。
    let demoted: Vec<uuid::Uuid> =
        sqlx::query_scalar("UPDATE users SET is_admin = FALSE WHERE is_admin = TRUE RETURNING id")
            .fetch_all(store.pool())
            .await
            .unwrap();

    // 无管理员:首次建管理员返回 true。
    let created = store.bootstrap_admin("Str0ng-Boot-Pass").await.unwrap();
    assert!(created, "无管理员时首次应建管理员");

    // 库里正好存在一个 is_admin=TRUE 用户(即刚建的 admin)。
    let admin_count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE is_admin = TRUE")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(admin_count, 1, "应正好一个管理员");

    // 再次调用幂等:已存在管理员返回 false,不新增。
    let again = store.bootstrap_admin("Another-Pass-9999").await.unwrap();
    assert!(!again, "已有管理员应跳过");
    let admin_count2: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE is_admin = TRUE")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(admin_count2, 1, "幂等:管理员数不变");

    // 清理:删掉本测试建的 bootstrap 管理员(它无 usage_ledgers 等依赖,可安全删),并复原被降级的原管理员。
    sqlx::query("DELETE FROM users WHERE email = 'admin@xrayc.local'")
        .execute(store.pool())
        .await
        .unwrap();
    for id in demoted {
        sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = $1")
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_bootstrap_default_plan_creates_once_idempotent() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 共享库可能已有默认套餐;把它 is_default 置 FALSE 造"无 is_default 套餐"态。
    // (只改 enabled 会留 is_default=TRUE,bootstrap 再插会撞 plans_single_default 唯一约束。)
    let original: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE plans SET is_default = FALSE WHERE is_default = TRUE RETURNING id",
    )
    .fetch_optional(store.pool())
    .await
    .unwrap();

    // 无 is_default 套餐:新建一个,返回 true,且正好一个启用默认套餐。
    let created = store.bootstrap_default_plan().await.unwrap();
    assert!(created, "无默认套餐时应建一个基础套餐");
    let cnt: i64 =
        sqlx::query_scalar("SELECT count(*) FROM plans WHERE is_default = TRUE AND enabled = TRUE")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(cnt, 1, "应正好一个启用默认套餐");

    // 幂等:已有可用默认套餐再调返回 false,不新增。
    let again = store.bootstrap_default_plan().await.unwrap();
    assert!(!again, "已有默认套餐应跳过");

    // 清理:删本测试新建的默认套餐(它无订阅依赖,且此刻是唯一 is_default=TRUE),复原原套餐。
    sqlx::query("DELETE FROM plans WHERE is_default = TRUE")
        .execute(store.pool())
        .await
        .unwrap();
    if let Some(id) = original {
        sqlx::query("UPDATE plans SET is_default = TRUE, enabled = TRUE WHERE id = $1")
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
    }
}

// 无默认套餐时 worker maintenance 应优雅跳过(返回 Ok 全 0),不报 DefaultPlanNotFound。
// 对应修复:刚装机 api 尚未 bootstrap 默认套餐时,worker 首轮维护不刷误导性 ERROR。
#[tokio::test]
async fn test_worker_maintenance_skips_when_no_default_plan() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 造"无默认套餐"态(现有默认套餐 is_default 置 FALSE,避开 plans_single_default 唯一约束)。
    let original: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE plans SET is_default = FALSE WHERE is_default = TRUE RETURNING id",
    )
    .fetch_optional(store.pool())
    .await
    .unwrap();

    // 无默认套餐:maintenance 返回 Ok(跳过),不报错。
    let result = store.run_worker_maintenance(14).await;
    assert!(
        result.is_ok(),
        "无默认套餐时 maintenance 应优雅跳过,实际: {result:?}"
    );
    assert_eq!(result.unwrap().expired_subscriptions, 0, "跳过时不应处理订阅");

    // 复原原默认套餐。
    if let Some(id) = original {
        sqlx::query("UPDATE plans SET is_default = TRUE WHERE id = $1")
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
    }
}
