// 数据库测试分片 65。
// 本文件覆盖监控中心数据层:节点运行态指标时序表 node_runtime_metrics 的写入/读取/级联。
// 时序表为 append-only,record 单条 INSERT 并钳制非法值(pct 0..100000、字节 >=0)。
// attach 用 DISTINCT ON 取每节点最新指标,merge 进读模型 access_nodes[].runtime_metrics(无数据为 null)。
// database_storage_json 返回库总字节 + 前 10 大表字节,供监控中心存储占用展示。
// 缺少 DATABASE_URL 时跳过,避免无 PostgreSQL 环境误报。
// 测试只用 example.test 域名和随机 token/UUID,不写真实主机/凭据。
// SQL 与断言必须围绕真实 PostgreSQL 行为,读回以写入侧落库为唯一真相。
// 后续监控中心数据层调整时优先维护本分片。
// 本头部满足前十行中文注释约束。

// 创建一个用于本分片测试的中转节点,返回其 UUID。
async fn pg_create_monitor_test_node(store: &PgStore, suffix: &str) -> uuid::Uuid {
    store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("monitor-node-{suffix}"),
            public_host: format!("monitor-{suffix}.example.test"),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("test-monitor-token-{suffix}"),
            cert_domain: None,
            acme_email: None,
            cf_enabled: false,
            cf_domain: None,
            ip_direct_address: None,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn test_pg_node_runtime_metrics_table_exists_and_idempotent_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node_runtime_metrics migration test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    // 重复跑迁移必须幂等(IF NOT EXISTS),第二次 migrate 不应报错。
    store.migrate("../../migrations").await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    let columns = sqlx::query_scalar::<_, String>(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'node_runtime_metrics'
        ORDER BY column_name
        "#,
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(
        columns,
        vec![
            "access_node_id".to_string(),
            "collected_at".to_string(),
            "cpu_pct_milli".to_string(),
            "disk_total_bytes".to_string(),
            "disk_used_bytes".to_string(),
            "id".to_string(),
            "mem_total_bytes".to_string(),
            "mem_used_bytes".to_string(),
            "preserve_on_cleanup".to_string(),
        ]
    );
}

#[tokio::test]
async fn test_pg_record_node_runtime_metrics_inserts_and_clamps_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL record_node_runtime_metrics test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = pg_create_monitor_test_node(&store, "record").await;

    // 写入越界值:pct 超上限、字节为负,应被钳制为 [0,100000] 与 >=0。
    store
        .record_node_runtime_metrics(
            node_id,
            NodeRuntimeMetricsReport {
                cpu_pct_milli: 250_000,
                mem_used_bytes: -5,
                mem_total_bytes: 8_000_000_000,
                disk_used_bytes: 10_000_000_000,
                disk_total_bytes: -1,
                collected_at_unix: 1_700_000_000,
            },
        )
        .await
        .unwrap();

    let row = sqlx::query_as::<_, (i32, i64, i64, i64, i64)>(
        r#"
        SELECT cpu_pct_milli, mem_used_bytes, mem_total_bytes, disk_used_bytes, disk_total_bytes
        FROM node_runtime_metrics
        WHERE access_node_id = $1
        ORDER BY collected_at DESC
        LIMIT 1
        "#,
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(row.0, 100_000, "cpu_pct_milli clamped to upper bound");
    assert_eq!(row.1, 0, "negative mem_used_bytes clamped to 0");
    assert_eq!(row.2, 8_000_000_000);
    assert_eq!(row.3, 10_000_000_000);
    assert_eq!(row.4, 0, "negative disk_total_bytes clamped to 0");
}

#[tokio::test]
async fn test_pg_attach_latest_node_runtime_picks_newest_per_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL attach_latest_node_runtime test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_with = pg_create_monitor_test_node(&store, "attach-with").await;
    let node_without = pg_create_monitor_test_node(&store, "attach-without").await;

    // 先写旧指标再写新指标,attach 必须取最新一条。
    store
        .record_node_runtime_metrics(
            node_with,
            NodeRuntimeMetricsReport {
                cpu_pct_milli: 10_000,
                mem_used_bytes: 1,
                mem_total_bytes: 2,
                disk_used_bytes: 3,
                disk_total_bytes: 4,
                collected_at_unix: 1_700_000_000,
            },
        )
        .await
        .unwrap();
    store
        .record_node_runtime_metrics(
            node_with,
            NodeRuntimeMetricsReport {
                cpu_pct_milli: 42_500,
                mem_used_bytes: 5_000_000_000,
                mem_total_bytes: 8_000_000_000,
                disk_used_bytes: 20_000_000_000,
                disk_total_bytes: 50_000_000_000,
                collected_at_unix: 1_700_000_600,
            },
        )
        .await
        .unwrap();

    let mut value = serde_json::json!({
        "access_nodes": [
            { "id": node_with.to_string() },
            { "id": node_without.to_string() },
        ]
    });
    store.attach_latest_node_runtime(&mut value).await.unwrap();

    let with = &value["access_nodes"][0]["runtime_metrics"];
    assert_eq!(with["cpu_pct_milli"].as_i64(), Some(42_500));
    assert_eq!(with["mem_used_bytes"].as_i64(), Some(5_000_000_000));
    assert_eq!(with["mem_total_bytes"].as_i64(), Some(8_000_000_000));
    assert_eq!(with["disk_used_bytes"].as_i64(), Some(20_000_000_000));
    assert_eq!(with["disk_total_bytes"].as_i64(), Some(50_000_000_000));
    assert!(with["collected_at"].is_string());

    // 无指标节点应给 null,不漏键也不串其他节点数据。
    assert!(
        value["access_nodes"][1]["runtime_metrics"].is_null(),
        "node without metrics must get null runtime_metrics"
    );
}

#[tokio::test]
async fn test_pg_access_routing_json_node_includes_runtime_metrics_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL routing json runtime_metrics test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = pg_create_monitor_test_node(&store, "routing").await;
    store
        .record_node_runtime_metrics(
            node_id,
            NodeRuntimeMetricsReport {
                cpu_pct_milli: 33_300,
                mem_used_bytes: 4_000_000_000,
                mem_total_bytes: 8_000_000_000,
                disk_used_bytes: 30_000_000_000,
                disk_total_bytes: 100_000_000_000,
                collected_at_unix: 1_700_000_000,
            },
        )
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let nodes = routing["access_nodes"]
        .as_array()
        .expect("access_nodes array present");

    let node = nodes
        .iter()
        .find(|node| node["id"].as_str() == Some(node_id.to_string().as_str()))
        .expect("created node present in routing json");
    let metrics = &node["runtime_metrics"];
    assert_eq!(metrics["cpu_pct_milli"].as_i64(), Some(33_300));
    assert_eq!(metrics["mem_total_bytes"].as_i64(), Some(8_000_000_000));
    assert_eq!(metrics["disk_total_bytes"].as_i64(), Some(100_000_000_000));

    // 读模型必须含 runtime_metrics 键;无指标的种子节点取 null。
    let seeded_without = nodes
        .iter()
        .find(|node| node["id"].as_str() != Some(node_id.to_string().as_str()));
    if let Some(seeded_without) = seeded_without {
        assert!(
            seeded_without.get("runtime_metrics").is_some(),
            "every node must expose runtime_metrics key"
        );
        assert!(
            seeded_without["runtime_metrics"].is_null(),
            "node without metrics must expose null runtime_metrics"
        );
    }
}

#[tokio::test]
async fn test_pg_node_runtime_metrics_cascade_deletes_with_node_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node_runtime_metrics cascade test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = pg_create_monitor_test_node(&store, "cascade").await;
    store
        .record_node_runtime_metrics(
            node_id,
            NodeRuntimeMetricsReport {
                cpu_pct_milli: 1_000,
                mem_used_bytes: 1,
                mem_total_bytes: 2,
                disk_used_bytes: 3,
                disk_total_bytes: 4,
                collected_at_unix: 1_700_000_000,
            },
        )
        .await
        .unwrap();

    let before = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*)::BIGINT FROM node_runtime_metrics WHERE access_node_id = $1"#,
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(before, 1);

    // 删除节点应经 ON DELETE CASCADE 清掉其运行态指标。
    sqlx::query(r#"DELETE FROM access_nodes WHERE id = $1"#)
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

    let after = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*)::BIGINT FROM node_runtime_metrics WHERE access_node_id = $1"#,
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(after, 0, "node deletion must cascade to runtime metrics");
}

#[tokio::test]
async fn test_pg_database_storage_json_returns_positive_bytes_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL database_storage_json test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let storage = store.database_storage_json().await.unwrap();
    let total = storage["total_bytes"].as_i64().expect("total_bytes present");
    assert!(total > 0, "pg_database_size should be positive, got {total}");

    let tables = storage["tables"]
        .as_array()
        .expect("tables array present");
    assert!(!tables.is_empty(), "should list at least one table");
    assert!(tables.len() <= 10, "should cap at top 10 tables");
    for table in tables {
        assert!(table["name"].is_string(), "table name present");
        assert!(
            table["bytes"].as_i64().unwrap_or(-1) >= 0,
            "table bytes non-negative"
        );
    }
}
