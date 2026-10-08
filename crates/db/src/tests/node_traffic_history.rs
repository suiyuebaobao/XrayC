// 节点统计必须合并明细与小时归档，线路删除不丢归属；不完整历史小时明确提示而不估算。

#[tokio::test]
async fn test_pg_node_traffic_includes_archived_and_deleted_line_usage() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (_, consumer, endpoints, _) = history_fixture(&store).await;
    let from = Utc::now() - Duration::days(40);
    let to = Utc::now() + Duration::hours(1);
    let before = store
        .node_traffic_trend_json(consumer, from, to, "day")
        .await
        .unwrap();
    assert_eq!(before["summary"]["total_bytes"], 2200);
    assert_eq!(before["points"].as_array().unwrap().len(), 2);
    for endpoint in endpoints {
        store.delete_admin_exit_endpoint(endpoint).await.unwrap();
    }
    let after = store
        .node_traffic_trend_json(consumer, from, to, "day")
        .await
        .unwrap();
    assert_eq!(after, before);
    let summary = store.node_traffic_summary_json(from, to).await.unwrap();
    let node = summary["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["access_node_id"] == consumer.to_string())
        .unwrap();
    assert_eq!(node["total_bytes"], 2200);
}

#[tokio::test]
async fn test_pg_node_traffic_partial_archived_hour_is_explicit() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node = uuid("00000000-0000-0000-0000-000000000201");
    let hour = DateTime::<Utc>::from_timestamp(
        (Utc::now() - Duration::days(30)).timestamp() / 3600 * 3600,
        0,
    )
    .unwrap();
    sqlx::query("INSERT INTO usage_hourly_rollups (hour_start, user_id, xray_user_key,
        access_line_id, access_node_id, exit_endpoint_id, ledger_count, delta_uplink, delta_downlink,
        delta_total, first_collected_at, last_collected_at) VALUES ($1,$2,'boundary-history',$3,$4,$5,2,30,70,100,$6,$7)")
        .bind(hour).bind(uuid("00000000-0000-0000-0000-000000000001"))
        .bind(uuid("00000000-0000-0000-0000-000000000501")).bind(node)
        .bind(uuid("00000000-0000-0000-0000-000000000302"))
        .bind(hour+Duration::minutes(10)).bind(hour+Duration::minutes(50))
        .execute(store.pool()).await.unwrap();
    let partial = store
        .node_traffic_trend_json(
            node,
            hour + Duration::minutes(20),
            hour + Duration::hours(1),
            "hour",
        )
        .await
        .unwrap();
    assert_eq!(partial["partial_archived_hours"], 1);
    assert_eq!(partial["summary"]["total_bytes"], 0);
    let complete = store
        .node_traffic_trend_json(node, hour, hour + Duration::hours(1), "hour")
        .await
        .unwrap();
    assert_eq!(complete["partial_archived_hours"], 0);
    assert_eq!(complete["summary"]["total_bytes"], 100);
    assert_eq!(complete["points"][0]["total_bytes"], 100);
}
