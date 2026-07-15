/// 数据库测试分片 64。
// 本分片覆盖“已登录用户邮箱验证码改密”的数据库层闭环。
// 验证弱密码被拒、改密后旧哈希失效新哈希生效、改密撤销该用户全部刷新令牌。
// 改密只在已校验邮箱验证码后调用，本分片直接验证密码更新与会话失效落库。
// 缺少 DATABASE_URL 时跳过，避免无 PostgreSQL 环境误报。
// 测试只用 example.test 域名和随机邮箱，不写真实主机/凭据/明文密码到日志。
// SQL 与断言必须围绕真实 PostgreSQL 行为，不用内存替身。
// 改密与刷新令牌撤销共享一个事务，保证“改完即踢其它会话”。
// 后续改密策略调整时优先维护本分片。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_set_user_password_rejects_weak_password_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL change-password weak test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("chpwd-weak-{}@example.test", Uuid::new_v4().simple());
    let registered = store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();

    // 弱密码（短于 8 位）必须被拒，且不改动 password_hash。
    let result = store
        .set_user_password_and_revoke_sessions(registered.user.id, "短")
        .await;
    assert!(matches!(result, Err(DbError::WeakPassword)));

    // 旧密码仍可登录，证明 password_hash 没有被弱密码请求破坏。
    let still = store
        .authenticate_user(&email, "old-password-123")
        .await
        .unwrap();
    assert!(still.is_some());
}

#[tokio::test]
async fn test_pg_set_user_password_updates_hash_and_revokes_sessions_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL change-password success test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("chpwd-ok-{}@example.test", Uuid::new_v4().simple());
    let registered = store
        .register_user(&email, "old-password-123")
        .await
        .unwrap();
    let user_id = registered.user.id;

    // 模拟用户已有两个活跃会话：颁两个刷新令牌。
    let refresh_a = store
        .create_refresh_token(user_id, Duration::days(7))
        .await
        .unwrap();
    let refresh_b = store
        .create_refresh_token(user_id, Duration::days(7))
        .await
        .unwrap();
    assert!(store
        .authenticate_refresh_token(&refresh_a)
        .await
        .unwrap()
        .is_some());

    store
        .set_user_password_and_revoke_sessions(user_id, "new-strong-password-9")
        .await
        .unwrap();

    // 新密码可登录、旧密码登录失败。
    assert!(store
        .authenticate_user(&email, "new-strong-password-9")
        .await
        .unwrap()
        .is_some());
    assert!(store
        .authenticate_user(&email, "old-password-123")
        .await
        .unwrap()
        .is_none());

    // 改密后该用户全部刷新令牌失效，旧会话无法续期。
    assert!(store
        .authenticate_refresh_token(&refresh_a)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .authenticate_refresh_token(&refresh_b)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .rotate_refresh_token(&refresh_a, Duration::days(7))
        .await
        .unwrap()
        .is_none());
}
