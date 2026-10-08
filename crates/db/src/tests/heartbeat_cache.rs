// 心跳缓存回归：无变更不得改写全体授权；自然到期、禁用、配额和令牌失效必须撤销配置。
// 数据库触发器只用于证明热路径没有偷偷执行全量同步，离开用例前移除。

async fn applied_heartbeat(store: &PgStore, node: Uuid) -> String {
    let first = store.heartbeat_json(Some(node), None).await.unwrap();
    assert!(!first["authorized_users"].as_array().unwrap().is_empty());
    let hash = first["desired_config_version"]
        .as_str()
        .unwrap()
        .to_string();
    store
        .record_config_result(Some(node), Some(&hash), true, None)
        .await
        .unwrap();
    hash
}

#[tokio::test]
async fn test_pg_heartbeat_unchanged_does_not_resync_assignments() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    let hash = applied_heartbeat(&store, node).await;
    let built: DateTime<Utc> =
        sqlx::query_scalar("SELECT config_built_at FROM access_nodes WHERE id=$1")
            .bind(node)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "CREATE FUNCTION test_block_resync() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'unchanged heartbeat performed global assignment writes'; END; $$",
    )
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER test_block_resync BEFORE INSERT OR UPDATE OR DELETE
        ON user_access_line_assignments FOR EACH STATEMENT EXECUTE FUNCTION test_block_resync()",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.heartbeat_json(Some(node), Some(&hash)).await;
    sqlx::query("DROP TRIGGER test_block_resync ON user_access_line_assignments")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION test_block_resync()")
        .execute(store.pool())
        .await
        .unwrap();
    let response = result.unwrap();
    assert_eq!(response["config_status"]["required"], false);
    assert_eq!(response["authorized_users"].as_array().unwrap().len(), 1);
    let after: DateTime<Utc> =
        sqlx::query_scalar("SELECT config_built_at FROM access_nodes WHERE id=$1")
            .bind(node)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(built, after, "无变化心跳不得重新编译配置");
    // 新装 Agent 未携带本地版本，仍必须获得完整配置。
    assert_eq!(
        store.heartbeat_json(Some(node), None).await.unwrap()["config_status"]["required"],
        true
    );
}

#[tokio::test]
async fn test_pg_heartbeat_cache_never_keeps_expired_or_disabled_authorization() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    let user = uuid("00000000-0000-0000-0000-000000000001");
    for mutation in [
        "UPDATE user_subscriptions SET expires_at=now()-interval '1 second' WHERE user_id=$1",
        "UPDATE user_subscriptions SET used_bytes=limit_bytes WHERE user_id=$1",
        "UPDATE users SET disabled=TRUE WHERE id=$1",
        "UPDATE subscription_tokens SET expires_at=now()-interval '1 second' WHERE user_id=$1",
    ] {
        store.seed_demo_data().await.unwrap();
        let hash = applied_heartbeat(&store, node).await;
        // 刻意不置 dirty，模拟时间流逝或另一节点耗尽共享配额。
        sqlx::query(mutation)
            .bind(user)
            .execute(store.pool())
            .await
            .unwrap();
        let response = store.heartbeat_json(Some(node), Some(&hash)).await.unwrap();
        assert_eq!(response["config_status"]["required"], true, "{mutation}");
        assert!(
            response["authorized_users"].as_array().unwrap().is_empty(),
            "{mutation}"
        );
        let empty_hash = response["desired_config_version"].as_str().unwrap();
        assert_ne!(empty_hash, hash);
        store
            .record_config_result(Some(node), Some(empty_hash), true, None)
            .await
            .unwrap();
        let settled = store
            .heartbeat_json(Some(node), Some(empty_hash))
            .await
            .unwrap();
        assert_eq!(settled["config_status"]["required"], false);
    }
}

#[tokio::test]
async fn test_pg_heartbeat_policy_change_invalidates_cached_config() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    let hash = applied_heartbeat(&store, node).await;
    let before: DateTime<Utc> =
        sqlx::query_scalar("SELECT config_built_at FROM access_nodes WHERE id=$1")
            .bind(node)
            .fetch_one(store.pool())
            .await
            .unwrap();
    store
        .update_subscription_settings_json(json!({"block_unhealthy_lines":true}))
        .await
        .unwrap();
    store.heartbeat_json(Some(node), Some(&hash)).await.unwrap();
    let after: DateTime<Utc> =
        sqlx::query_scalar("SELECT config_built_at FROM access_nodes WHERE id=$1")
            .bind(node)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(after > before);
}

#[tokio::test]
async fn test_pg_heartbeat_rejects_a_config_built_across_a_new_revision() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    sqlx::query(
        "CREATE FUNCTION test_wait_config_build() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(20261006,777); RETURN NULL; END; $$",
    )
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER test_wait_config_build BEFORE INSERT OR UPDATE OR DELETE
        ON user_access_line_assignments FOR EACH STATEMENT EXECUTE FUNCTION test_wait_config_build()")
        .execute(store.pool()).await.unwrap();
    let mut held = store.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(20261006,777)")
        .execute(&mut *held)
        .await
        .unwrap();
    let other = store.clone();
    let building = tokio::spawn(async move { other.heartbeat_json(Some(node), None).await });
    let mut waiting = false;
    for _ in 0..250 {
        waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM pg_locks
            WHERE locktype='advisory' AND classid=20261006 AND objid=777 AND NOT granted)",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    sqlx::query(
        "UPDATE access_nodes SET config_dirty=TRUE, desired_config_hash=NULL,
        config_dirty_at=clock_timestamp() WHERE id=$1",
    )
    .bind(node)
    .execute(store.pool())
    .await
    .unwrap();
    held.commit().await.unwrap();
    let response = building.await.unwrap();
    sqlx::query("DROP TRIGGER test_wait_config_build ON user_access_line_assignments")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION test_wait_config_build()")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(waiting, "配置生成必须确实进入被暂停的同步步骤");
    assert!(response.is_err(), "跨越新修订的配置不能标成已生成");
    let desired: Option<String> =
        sqlx::query_scalar("SELECT desired_config_hash FROM access_nodes WHERE id=$1")
            .bind(node)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(desired.is_none());
    assert!(store.heartbeat_json(Some(node), None).await.is_ok());
}
