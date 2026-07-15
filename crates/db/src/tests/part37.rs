// 数据库测试分片 37。
// 本文件覆盖后台用户设备/IP 观察列表。
// 设备管理只展示订阅拉取 IP 和真实节点使用 IP。
// 它不恢复旧客户端设备绑定、专属订阅或设备 token。
// 相同明文 IP 应去重为一行，并合并来源与计数。
// 没有明文 IP 时才使用 client_ip_hash 作为去重键。
// 测试只使用 RFC 5737 示例地址和临时数据。
// 修改本文件时继续保持中文注释和文件长度限制。
// SQL 与断言必须围绕真实 PostgreSQL 行为。
// 本头部满足前十行中文注释约束。

use sha2::Digest;

#[tokio::test]
async fn test_pg_admin_user_devices_merge_subscription_and_usage_ip_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL user device IP test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let user_key = "u-00000000000000000000000000000001@xrayc.local";
    let client_ip_hash =
        "sha256:ddbea5471056690e5b1dcfe0c39ffca2f91ee81710048d066e2d53c7d012e14e";
    let observed_at = Utc::now() - Duration::seconds(20);

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();

    store
        .record_subscription_pull_event_for_user(
            user_id,
            "203.0.113.8",
            client_ip_hash,
            "mihomo-test",
        )
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO access_user_session_events (
            access_node_id, access_line_id, user_id, xray_user_key,
            client_ip, client_ip_hash, active_connection_count, status, observed_at
        )
        VALUES ($1, $2, $3, $4, '203.0.113.8', $5, 3, 'online', $6)
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(user_id)
    .bind(user_key)
    .bind(client_ip_hash)
    .bind(observed_at)
    .execute(store.pool())
    .await
    .unwrap();

    let page = store.admin_user_devices_json(user_id, 1, 20).await.unwrap();
    assert_eq!(page["total"], 1);
    let item = &page["items"][0];
    assert_eq!(item["client_ip"], "203.0.113.8");
    assert_eq!(item["client_ip_hash"], client_ip_hash);
    assert_eq!(item["source"], "both");
    assert_eq!(item["subscription_pull_count"], 1);
    assert_eq!(item["node_use_count"], 1);
    assert_eq!(item["last_access_line"]["id"], line_id.to_string());
    assert_eq!(item["last_access_node"]["id"], node_id.to_string());
    assert_eq!(item["last_session_status"], "online");
    assert_eq!(item["last_active_connection_count"], 3);

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pg_admin_user_devices_keep_distinct_hash_when_plain_ip_missing() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL user device hash test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let hash_one = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let hash_two = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();

    store
        .record_subscription_pull_event_for_user(user_id, "", hash_one, "mihomo-test")
        .await
        .unwrap();
    store
        .record_subscription_pull_event_for_user(user_id, "", hash_two, "mihomo-test")
        .await
        .unwrap();

    let page = store.admin_user_devices_json(user_id, 1, 20).await.unwrap();
    assert_eq!(page["total"], 2);
    assert!(page["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["source"] == "subscription_pull"));

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pg_admin_user_devices_merge_plain_subscription_with_hash_only_usage() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL user device mixed IP/hash test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let user_key = "u-00000000000000000000000000000001@xrayc.local";
    let client_ip = "203.0.113.10";
    let client_ip_hash = format!("sha256:{:x}", sha2::Sha256::digest(client_ip.as_bytes()));

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();

    store
        .record_subscription_pull_event_for_user(user_id, client_ip, "", "mihomo-test")
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO access_user_session_events (
            access_node_id, access_line_id, user_id, xray_user_key,
            client_ip, client_ip_hash, active_connection_count, status, observed_at
        )
        VALUES ($1, $2, $3, $4, '', $5, 2, 'online', now())
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(user_id)
    .bind(user_key)
    .bind(&client_ip_hash)
    .execute(store.pool())
    .await
    .unwrap();

    let page = store.admin_user_devices_json(user_id, 1, 20).await.unwrap();
    assert_eq!(page["total"], 1);
    let item = &page["items"][0];
    assert_eq!(item["client_ip"], client_ip);
    assert_eq!(item["client_ip_hash"], client_ip_hash);
    assert_eq!(item["source"], "both");
    assert_eq!(item["subscription_pull_count"], 1);
    assert_eq!(item["node_use_count"], 1);

    sqlx::query("DELETE FROM subscription_pull_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id = $1")
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();
}
