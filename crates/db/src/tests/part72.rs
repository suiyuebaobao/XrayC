// 数据库测试分片 72:监控中心「节点资源」流量聚合读接口。
// 覆盖 node_traffic_summary_json 与 node_traffic_trend_json 两个只读聚合。
// ① 按中转节点聚合上下行正确(delta 原始差值,非 billed_bytes)。
// ② 时间区间 [from, to) 过滤正确:区间外与 to 边界的账本不计入。
// ③ 趋势按 hour/day 分桶正确,points 升序,summary 为整段合计。
// ④ 无流量节点也返回,数值为 0(与 access_nodes 对齐)。
// 测试仅用演示数据与固定占位地址,不保存任何真实主机资料。
// PostgreSQL 未配置(缺 DATABASE_URL)时保持跳过语义,不误报失败。
// 文件头部使用中文注释满足仓库拆分约束。
// 本头部满足前十行中文注释约束。

    // 从汇总结果里按节点 id 找出对应节点对象,找不到即断言失败。
    fn nt_find_node(nodes: &[Value], id: Uuid) -> &Value {
        let want = id.to_string();
        nodes
            .iter()
            .find(|node| node["access_node_id"].as_str() == Some(want.as_str()))
            .expect("中转节点应出现在汇总结果中")
    }

    // 插入一个额外的中转节点(占位地址,不含真实资料)。
    async fn nt_insert_node(store: &PgStore, node_id: Uuid, name: &str) {
        sqlx::query(
            r#"
            INSERT INTO access_nodes (id, name, public_host, agent_token_hash, config_dirty)
            VALUES ($1, $2, 'node.example.test', 'test-agent-token-hash', TRUE)
            "#,
        )
        .bind(node_id)
        .bind(name)
        .execute(store.pool())
        .await
        .unwrap();
    }

    // 插入一条从属指定节点的中转线路,复用种子出口池。
    async fn nt_insert_line(
        store: &PgStore,
        line_id: Uuid,
        node_id: Uuid,
        exit_pool_id: Uuid,
        listen_port: i32,
    ) {
        sqlx::query(
            r#"
            INSERT INTO access_lines (
                id, name, access_node_id, exit_pool_id, listen_host, listen_port,
                protocol, transport, user_uuid, server_name, public_key, short_id,
                enabled, udp_packet_encoding
            )
            VALUES (
                $1, '测试线路', $2, $3, 'listen.example.test', $4,
                'vless', 'tcp', $5, '', '', '', TRUE, ''
            )
            "#,
        )
        .bind(line_id)
        .bind(node_id)
        .bind(exit_pool_id)
        .bind(listen_port)
        .bind(line_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    }

    // 插入一条 usage_ledgers 账本:只写原始 delta 上下行与采集时间。
    // traffic_source 走默认 'access_line',billed_bytes 仅占位、聚合不读取。
    async fn nt_insert_ledger(
        store: &PgStore,
        line_id: Uuid,
        user_id: Uuid,
        delta_uplink: i64,
        delta_downlink: i64,
        collected_at: DateTime<Utc>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO usage_ledgers (
                access_line_id, user_id, xray_user_key,
                delta_uplink, delta_downlink, billed_bytes, collected_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(line_id)
        .bind(user_id)
        .bind(format!("u-{}@xrayc.local", user_id.simple()))
        .bind(delta_uplink)
        .bind(delta_downlink)
        .bind(delta_uplink + delta_downlink)
        .bind(collected_at)
        .execute(store.pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_node_traffic_summary_aggregates_by_node_and_filters_time_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping node traffic summary test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let node1 = uuid("00000000-0000-0000-0000-000000000201"); // 种子节点
        let line1 = uuid("00000000-0000-0000-0000-000000000501"); // 种子线路
        let node2 = uuid("00000000-0000-0000-0000-000000000202");
        let line2 = uuid("00000000-0000-0000-0000-000000000502");
        let node3 = uuid("00000000-0000-0000-0000-000000000203"); // 无线路无流量

        nt_insert_node(&store, node2, "节点二").await;
        nt_insert_line(&store, line2, node2, exit_pool_id, 20_002).await;
        nt_insert_node(&store, node3, "节点三").await;

        // 固定基准 2026-01-01T00:00:00Z,区间取 [base, base+2h)。
        let base_ms: i64 = 1_767_225_600_000;
        let from = DateTime::<Utc>::from_timestamp_millis(base_ms).unwrap();
        let to = DateTime::<Utc>::from_timestamp_millis(base_ms + 2 * 3_600_000).unwrap();
        let at = |offset_ms: i64| DateTime::<Utc>::from_timestamp_millis(base_ms + offset_ms).unwrap();

        // node1 区间内两条:up 100+300=400,down 200+400=600,total 1000。
        nt_insert_ledger(&store, line1, user_id, 100, 200, at(10 * 60_000)).await;
        nt_insert_ledger(&store, line1, user_id, 300, 400, at(70 * 60_000)).await;
        // node1 区间外(前一小时)与上界(等于 to,排他)各一条,均不得计入。
        nt_insert_ledger(&store, line1, user_id, 9_999, 9_999, at(-60 * 60_000)).await;
        nt_insert_ledger(&store, line1, user_id, 5_555, 5_555, at(120 * 60_000)).await;
        // node2 区间内一条:up 50,down 70,total 120。
        nt_insert_ledger(&store, line2, user_id, 50, 70, at(20 * 60_000)).await;

        let data = store.node_traffic_summary_json(from, to).await.unwrap();
        let nodes = data["nodes"].as_array().unwrap();
        // 三个中转节点都应出现(含零流量 node3),便于前端与节点清单对齐。
        assert_eq!(nodes.len(), 3);

        let n1 = nt_find_node(nodes, node1);
        assert_eq!(n1["uplink_bytes"], 400);
        assert_eq!(n1["downlink_bytes"], 600);
        assert_eq!(n1["total_bytes"], 1_000);

        let n2 = nt_find_node(nodes, node2);
        assert_eq!(n2["uplink_bytes"], 50);
        assert_eq!(n2["downlink_bytes"], 70);
        assert_eq!(n2["total_bytes"], 120);

        let n3 = nt_find_node(nodes, node3);
        assert_eq!(n3["uplink_bytes"], 0);
        assert_eq!(n3["downlink_bytes"], 0);
        assert_eq!(n3["total_bytes"], 0);
    }

    #[tokio::test]
    async fn test_node_traffic_trend_hour_buckets_and_summary_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping node traffic hour trend test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node1 = uuid("00000000-0000-0000-0000-000000000201");
        let line1 = uuid("00000000-0000-0000-0000-000000000501");

        let base_ms: i64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z
        let from = DateTime::<Utc>::from_timestamp_millis(base_ms).unwrap();
        let to = DateTime::<Utc>::from_timestamp_millis(base_ms + 2 * 3_600_000).unwrap();
        let at = |offset_ms: i64| DateTime::<Utc>::from_timestamp_millis(base_ms + offset_ms).unwrap();

        // 第 0 小时桶:+10min(100/200);第 1 小时桶:+70min(300/400)。
        nt_insert_ledger(&store, line1, user_id, 100, 200, at(10 * 60_000)).await;
        nt_insert_ledger(&store, line1, user_id, 300, 400, at(70 * 60_000)).await;
        // 区间外噪声(前 30 分钟)不得计入。
        nt_insert_ledger(&store, line1, user_id, 7_000, 8_000, at(-30 * 60_000)).await;

        let data = store
            .node_traffic_trend_json(node1, from, to, "hour")
            .await
            .unwrap();
        let points = data["points"].as_array().unwrap();
        assert_eq!(points.len(), 2);

        // points 按桶起点升序。
        assert_eq!(points[0]["bucket_start_ms"], base_ms);
        assert_eq!(points[0]["uplink_bytes"], 100);
        assert_eq!(points[0]["downlink_bytes"], 200);
        assert_eq!(points[0]["total_bytes"], 300);

        assert_eq!(points[1]["bucket_start_ms"], base_ms + 3_600_000);
        assert_eq!(points[1]["uplink_bytes"], 300);
        assert_eq!(points[1]["downlink_bytes"], 400);
        assert_eq!(points[1]["total_bytes"], 700);

        let summary = &data["summary"];
        assert_eq!(summary["uplink_bytes"], 400);
        assert_eq!(summary["downlink_bytes"], 600);
        assert_eq!(summary["total_bytes"], 1_000);
    }

    #[tokio::test]
    async fn test_node_traffic_trend_day_buckets_and_summary_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping node traffic day trend test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node1 = uuid("00000000-0000-0000-0000-000000000201");
        let line1 = uuid("00000000-0000-0000-0000-000000000501");

        let base_ms: i64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z
        let day_ms: i64 = 86_400_000;
        let from = DateTime::<Utc>::from_timestamp_millis(base_ms).unwrap();
        let to = DateTime::<Utc>::from_timestamp_millis(base_ms + 2 * day_ms).unwrap();
        let at = |offset_ms: i64| DateTime::<Utc>::from_timestamp_millis(base_ms + offset_ms).unwrap();

        // 第 0 天:+10min(100/200)与 +2h(300/400)→ up 400,down 600。
        nt_insert_ledger(&store, line1, user_id, 100, 200, at(10 * 60_000)).await;
        nt_insert_ledger(&store, line1, user_id, 300, 400, at(2 * 3_600_000)).await;
        // 第 1 天:+1day5min(1000/2000)。
        nt_insert_ledger(&store, line1, user_id, 1_000, 2_000, at(day_ms + 5 * 60_000)).await;
        // 区间外噪声(前 1 小时)不得计入。
        nt_insert_ledger(&store, line1, user_id, 9_999, 9_999, at(-3_600_000)).await;

        let data = store
            .node_traffic_trend_json(node1, from, to, "day")
            .await
            .unwrap();
        let points = data["points"].as_array().unwrap();
        assert_eq!(points.len(), 2);

        assert_eq!(points[0]["bucket_start_ms"], base_ms);
        assert_eq!(points[0]["uplink_bytes"], 400);
        assert_eq!(points[0]["downlink_bytes"], 600);
        assert_eq!(points[0]["total_bytes"], 1_000);

        assert_eq!(points[1]["bucket_start_ms"], base_ms + day_ms);
        assert_eq!(points[1]["uplink_bytes"], 1_000);
        assert_eq!(points[1]["downlink_bytes"], 2_000);
        assert_eq!(points[1]["total_bytes"], 3_000);

        let summary = &data["summary"];
        assert_eq!(summary["uplink_bytes"], 1_400);
        assert_eq!(summary["downlink_bytes"], 2_600);
        assert_eq!(summary["total_bytes"], 4_000);
    }
