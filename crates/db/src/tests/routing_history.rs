// 历史路由回归测试：覆盖日/小时聚合、明细、共用出口、批次删除与重新启用。
// 所有数据位于隔离测试库，绝不对正式节点执行删除或改写。

async fn history_fixture(store: &PgStore) -> (Uuid, Uuid, Vec<Uuid>, Vec<Uuid>) {
    let owner = create_port_conflict_node(store, "history-owner").await;
    let consumer = create_port_conflict_node(store, "history-consumer").await;
    let created = store
        .create_admin_local_exit_lines(
            owner,
            AdminLocalExitLinesInput {
                lines: (0..2)
                    .map(|index| AdminLocalExitLineInput {
                        resource_name: format!("历史资源-{index}"),
                        endpoint_name: format!("历史出口-{index}"),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "203.0.113.90".to_string(),
                        port: 41000 + index,
                        outbound_config: json!({"username":"test", "password":"fixture"}),
                        stream_config: json!({}),
                        probe_config: json!({}),
                        enabled: true,
                        node_domain_id: None,
                    })
                    .collect(),
            },
        )
        .await
        .unwrap();
    let endpoints: Vec<Uuid> = created["created_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| Uuid::parse_str(item["exit_endpoint_id"].as_str().unwrap()).unwrap())
        .collect();
    let entry = store
        .create_admin_access_entry(reality_entry_input(consumer, "历史入口", 43333, true))
        .await
        .unwrap();
    let mut lines = Vec::new();
    for (index, endpoint) in endpoints.iter().enumerate() {
        let line = store
            .create_admin_access_entry_exit_binding(
                entry,
                AdminAccessEntryExitBindingInput {
                    exit_endpoint_id: *endpoint,
                    name: format!("历史线路-{index}"),
                    enabled: true,
                    sort_weight: 100,
                    remark: String::new(),
                },
            )
            .await
            .unwrap();
        lines.push(line);
        for (days, amount) in [(30_i64, 1000_i64), (0, 100)] {
            sqlx::query("INSERT INTO usage_ledgers (user_id, xray_user_key, traffic_source,
                access_line_id, exit_endpoint_id, delta_uplink, delta_downlink, delta_total,
                billing_multiplier, billed_uplink, billed_downlink, billed_bytes, collected_at, recorded_at)
                VALUES ($1, 'history-regression', 'access_line', $2, $3, $4, 0, $4, 2, $4*2, 0, $4*2,
                now()-$5::bigint*interval '1 day', now()-$5::bigint*interval '1 day')")
                .bind(uuid("00000000-0000-0000-0000-000000000001"))
                .bind(line).bind(endpoint).bind(amount).bind(days).execute(store.pool()).await.unwrap();
        }
    }
    let mut tx = store.pool().begin().await.unwrap();
    crate::store::maintenance_usage_rollups::roll_up_and_prune_usage_ledgers_in_tx(
        &mut tx,
        TrafficLogRetentionPolicy {
            detail_retention_days: 14,
            prune_enabled: true,
            delete_batch_size: 5000,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (owner, consumer, endpoints, lines)
}

async fn history_totals(store: &PgStore) -> Vec<(String, i64, i64, i64)> {
    sqlx::query_as("SELECT 'details'::text, count(*)::bigint, COALESCE(sum(delta_total),0)::bigint,
            COALESCE(sum(billed_bytes),0)::bigint FROM usage_ledgers WHERE xray_user_key='history-regression'
        UNION ALL SELECT 'daily', count(*)::bigint, COALESCE(sum(delta_total),0)::bigint,
            COALESCE(sum(billed_bytes),0)::bigint FROM usage_daily_rollups WHERE xray_user_key='history-regression'
        UNION ALL SELECT 'hourly', count(*)::bigint, COALESCE(sum(delta_total),0)::bigint,
            COALESCE(sum(billed_bytes),0)::bigint FROM usage_hourly_rollups WHERE xray_user_key='history-regression'
        ORDER BY 1").fetch_all(store.pool()).await.unwrap()
}

#[tokio::test]
async fn test_pg_history_node_delete_preserves_every_amount_and_identity() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (owner, consumer, endpoints, lines) = history_fixture(&store).await;
    let before = history_totals(&store).await;
    // 只删出口所属节点也会删其他节点引用这些出口的线路，消费者节点本身必须保留。
    let result = store
        .delete_admin_access_nodes(vec![owner, owner])
        .await
        .unwrap();
    assert_eq!(result.deleted_node_count, 1);
    assert_eq!(result.deleted_access_line_count, 2);
    assert_eq!(history_totals(&store).await, before);
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT config_dirty FROM access_nodes WHERE id=$1")
            .bind(consumer)
            .fetch_one(store.pool())
            .await
            .unwrap()
    );
    let snapshots = sqlx::query_as::<_, (Uuid, Uuid, String)>(
        "SELECT hs.access_line_id, hs.exit_endpoint_id, hs.access_line_name
         FROM usage_daily_rollups r JOIN usage_routing_snapshots hs ON hs.id=r.routing_snapshot_id
         WHERE xray_user_key='history-regression' ORDER BY hs.access_line_name",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    for (index, (line, endpoint, name)) in snapshots.iter().enumerate() {
        assert_eq!(*line, lines[index]);
        assert_eq!(*endpoint, endpoints[index]);
        assert_eq!(*name, format!("历史线路-{index}"));
    }
    // 后续再删消费者，日/小时的多行不得因 SET NULL 变成同一唯一维度。
    store
        .delete_admin_access_nodes(vec![consumer])
        .await
        .unwrap();
    assert_eq!(history_totals(&store).await, before);
    let logs = store
        .admin_user_traffic_logs_json(
            uuid("00000000-0000-0000-0000-000000000001"),
            1,
            20,
            None,
            None,
            Some(lines[0]),
            Some(endpoints[0]),
        )
        .await
        .unwrap();
    assert_eq!(logs["total"], 1);
    assert_eq!(logs["items"][0]["access_line"]["name"], "历史线路-0");
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM access_lines WHERE id=$1)")
            .bind(uuid("00000000-0000-0000-0000-000000000501"))
            .fetch_one(store.pool())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_pg_history_endpoint_delete_paths_and_name_snapshots() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (_, _, endpoints, lines) = history_fixture(&store).await;
    let before = history_totals(&store).await;
    sqlx::query("UPDATE access_lines SET name='新的显示名称' WHERE id=$1")
        .bind(lines[0])
        .execute(store.pool())
        .await
        .unwrap();
    let logs = store
        .admin_user_traffic_logs_json(
            uuid("00000000-0000-0000-0000-000000000001"),
            1,
            20,
            None,
            None,
            Some(lines[0]),
            None,
        )
        .await
        .unwrap();
    assert_eq!(logs["items"][0]["access_line"]["name"], "历史线路-0");
    store.delete_local_exit_line(endpoints[0]).await.unwrap();
    store
        .delete_admin_exit_endpoint(endpoints[1])
        .await
        .unwrap();
    assert_eq!(history_totals(&store).await, before);
}

#[tokio::test]
async fn test_pg_endpoint_reenable_respects_binding_intent() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (_, _, endpoints, lines) = history_fixture(&store).await;
    for enabled in [false, true] {
        store
            .update_admin_exit_endpoint(
                endpoints[0],
                AdminExitEndpointUpdate {
                    enabled: Some(enabled),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let actual = sqlx::query_scalar::<_, bool>("SELECT enabled FROM access_lines WHERE id=$1")
            .bind(lines[0])
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(actual, enabled);
    }
    store
        .update_admin_access_entry_exit_binding(
            lines[0],
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id: endpoints[0],
                name: "管理员停用".to_string(),
                enabled: false,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();
    for enabled in [false, true] {
        store
            .update_local_exit_line(
                endpoints[0],
                AdminLocalExitLineUpdate {
                    enabled: Some(enabled),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT enabled FROM access_lines WHERE id=$1")
            .bind(lines[0])
            .fetch_one(store.pool())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_pg_history_batch_delete_waits_for_rollup_transaction() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (owner, consumer, _, _) = history_fixture(&store).await;
    // 把剩余明细转入同一天的已存快照维度，模拟 Worker 与批次删除交错。
    sqlx::query(
        "UPDATE usage_ledgers SET collected_at=now()-interval '30 days'
        WHERE xray_user_key='history-regression'",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    crate::store::maintenance_usage_rollups::roll_up_and_prune_usage_ledgers_in_tx(
        &mut tx,
        TrafficLogRetentionPolicy {
            detail_retention_days: 14,
            prune_enabled: true,
            delete_batch_size: 5000,
        },
    )
    .await
    .unwrap();
    let other = store.clone();
    let mut deleting =
        tokio::spawn(async move { other.delete_admin_access_nodes(vec![owner, consumer]).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(80), &mut deleting)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    let deleted = deleting.await.unwrap().unwrap();
    assert_eq!(deleted.deleted_node_count, 2);
    assert_eq!(
        history_totals(&store).await,
        vec![
            ("daily".to_string(), 2, 2200, 4400),
            ("details".to_string(), 0, 0, 0),
            ("hourly".to_string(), 2, 2200, 4400),
        ]
    );
}
