// 实例心跳与版本兼容回归：离线实例不伪装在线，旧 Agent 不能沿用新版本的 Xray 回报。

#[tokio::test]
async fn test_pg_runtime_versions_preserve_unknown_and_detect_stale_workers() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    store
        .record_agent_versions(
            node,
            Some("0.1.0+release-test"),
            Some("Xray fixture-version"),
        )
        .await
        .unwrap();
    let first = store.runtime_versions_json().await.unwrap();
    assert_eq!(first["nodes"][0]["xray_version"], "Xray fixture-version");
    store
        .record_agent_versions(node, Some("0.1.0"), None)
        .await
        .unwrap();
    assert_eq!(
        store.runtime_versions_json().await.unwrap()["nodes"][0]["xray_version"],
        ""
    );
    let worker = store
        .register_worker_version("0.1.0", "release-test", 60)
        .await
        .unwrap();
    assert_eq!(
        store.runtime_versions_json().await.unwrap()["workers"][0]["fresh"],
        true
    );
    sqlx::query("UPDATE service_runtime_versions SET heartbeat_at=now()-interval '1 hour' WHERE instance_id=$1")
        .bind(worker).execute(store.pool()).await.unwrap();
    assert_eq!(
        store.runtime_versions_json().await.unwrap()["workers"][0]["fresh"],
        false
    );
    store.touch_worker_version(worker).await.unwrap();
    assert_eq!(
        store.runtime_versions_json().await.unwrap()["workers"][0]["fresh"],
        true
    );
}
