/// 数据库测试分片 21。
// 本文件覆盖管理员按用户查看流量日志的 PostgreSQL 行为。
// 流量日志必须以用户为边界，不能混入其它用户账本。
// 日志需要返回账本上下行、扣费流量、中转入口和出口端点摘要。
// 新版 agent 上报会话时会保存明文客户端 IP 与哈希，后台按账本时间关联。
// 旧历史账本允许没有 IP，但有会话事件时必须显示到用户明细。
// 测试只使用示例地址和临时账本，不保存真实服务器信息。
// 修改本文件时继续保持中文注释和文件长度限制。
// SQL 与断言必须围绕真实 PostgreSQL 行为。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_admin_user_traffic_logs_return_user_scoped_rows_with_client_ip_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL user traffic log test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let other_user_id = uuid("00000000-0000-0000-0000-000000000002");
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let user_key = "u-00000000000000000000000000000001@xrayc.local";
    let other_key = "u-admin@xrayc.local";
    let client_ip_hash =
        "sha256:ddbea5471056690e5b1dcfe0c39ffca2f91ee81710048d066e2d53c7d012e14e";
    let collected_at = Utc::now() - Duration::seconds(30);

    sqlx::query("DELETE FROM usage_ledgers WHERE user_id IN ($1, $2)")
        .bind(user_id)
        .bind(other_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id IN ($1, $2)")
        .bind(user_id)
        .bind(other_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let session_payload = json!({
        "access_node_id": node_id,
        "sessions": [{
            "access_line_id": line_id,
            "xray_user_key": user_key,
            "client_ip": "203.0.113.8",
            "client_ip_hash": client_ip_hash,
            "active_connection_count": 2,
            "started_at_unix": collected_at.timestamp() - 1,
            "last_seen_at_unix": collected_at.timestamp()
        }]
    });
    assert_eq!(
        store
            .record_agent_sessions(node_id, &session_payload)
            .await
            .unwrap(),
        1
    );

    for (target_user, key, uplink, downlink) in [
        (user_id, user_key, 1024_i64, 2048_i64),
        (other_user_id, other_key, 999_i64, 999_i64),
    ] {
        sqlx::query(
            r#"
            INSERT INTO usage_ledgers (
                access_line_id, user_id, xray_user_key, traffic_source,
                delta_uplink, delta_downlink, billing_multiplier,
                billed_bytes, delta_total, billed_uplink, billed_downlink,
                collected_at, recorded_at, exit_endpoint_id
            )
            VALUES ($1, $2, $3, 'access_line', $4, $5, 1, $6, $6, $4, $5, $7, $7, $8)
            "#,
        )
        .bind(line_id)
        .bind(target_user)
        .bind(key)
        .bind(uplink)
        .bind(downlink)
        .bind(uplink + downlink)
        .bind(collected_at)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
    }
    for (source, observed_at, billed) in [
        ("internal", collected_at, 4000_i64),
        ("access_line", collected_at - Duration::days(20), 5000_i64),
    ] {
        sqlx::query(
            r#"
            INSERT INTO usage_ledgers (
                access_line_id, user_id, xray_user_key, traffic_source,
                delta_uplink, delta_downlink, billing_multiplier,
                billed_bytes, delta_total, billed_uplink, billed_downlink,
                collected_at, recorded_at, exit_endpoint_id
            )
            VALUES ($1, $2, $3, $4, 100, 100, 1, $5, 200, 100, 100, $6, $6, $7)
            "#,
        )
        .bind(line_id)
        .bind(user_id)
        .bind(user_key)
        .bind(source)
        .bind(billed)
        .bind(observed_at)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
    }

    let page = store
        .admin_user_traffic_logs_json(user_id, 1, 20, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(page["total"], 1);
    let item = &page["items"][0];
    assert_eq!(item["user_id"], user_id.to_string());
    assert_eq!(item["client_ip"], "203.0.113.8");
    assert_eq!(item["client_ip_hash"], client_ip_hash);
    assert_eq!(item["delta_uplink"], 1024);
    assert_eq!(item["delta_downlink"], 2048);
    assert_eq!(item["delta_total"], 3072);
    assert_eq!(item["billed_bytes"], 3072);
    assert_eq!(item["access_line"]["id"], line_id.to_string());
    assert_eq!(item["access_node"]["id"], node_id.to_string());
    assert_eq!(item["exit_endpoint"]["id"], endpoint_id.to_string());

    sqlx::query("DELETE FROM usage_ledgers WHERE user_id IN ($1, $2)")
        .bind(user_id)
        .bind(other_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM access_user_session_events WHERE user_id IN ($1, $2)")
        .bind(user_id)
        .bind(other_user_id)
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pg_admin_delete_user_removes_session_events_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL user delete session cleanup test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let email = format!("delete-session-{}@example.test", Uuid::new_v4().simple());
    let created = store
        .create_admin_user_json(AdminUserCreate {
            email,
            password: "created123".to_string(),
            disabled: false,
            is_admin: false,
            plan_id: None,
            rate_limit_bps: None,
            rate_limit_up_bps: None,
            rate_limit_down_bps: None,
        })
        .await
        .unwrap();
    let user_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let user_key: String = sqlx::query_scalar("SELECT xray_user_key FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");

    sqlx::query(
        r#"
        INSERT INTO access_user_session_events (
            access_node_id, access_line_id, user_id, xray_user_key,
            client_ip, client_ip_hash, active_connection_count, status, observed_at
        )
        VALUES ($1, $2, $3, $4, '203.0.113.9', 'sha256:test-delete-session', 1, 'online', now())
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(user_id)
    .bind(&user_key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO access_user_sessions (
            access_node_id, access_line_id, xray_user_key,
            client_ip_hash, started_at, last_seen_at
        )
        VALUES ($1, $2, $3, 'sha256:test-delete-session', now(), now())
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(&user_key)
    .execute(store.pool())
    .await
    .unwrap();

    let deleted = store.delete_admin_user_json(user_id).await.unwrap();
    assert_eq!(deleted["deleted"], true);
    assert_eq!(deleted["deleted_session_count"], 1);
    assert_eq!(deleted["deleted_session_event_count"], 1);

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM access_user_session_events WHERE user_id = $1 OR xray_user_key = $2",
    )
    .bind(user_id)
    .bind(&user_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    let remaining_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM access_user_sessions WHERE xray_user_key = $1",
    )
    .bind(&user_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining_sessions, 0);
}
