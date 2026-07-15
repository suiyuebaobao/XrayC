// 数据库测试分片 23。
// 本文件覆盖 agent 探测上报的重复数据防御。
// 压测和真实 agent 都可能在重试时带来重复探测项。
// 数据库层应只处理同一线路或出口的最新探测结果。
// 这里复用运行时上报测试上下文，避免重复准备基础数据。
// 测试需要真实 PostgreSQL，未设置 DATABASE_URL 时自动跳过。
// 所有断言只检查行为，不依赖外部网络或私有服务器。
// 新增测试请继续保持单文件不超过五百行。
// 注释使用中文，方便后续维护和审查。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_agent_probes_deduplicate_repeated_probe_items_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL probe dedupe test");
        return;
    };
    let _guard = pg_test_guard().await;
    let ctx = setup_runtime_report_test_context(&database_url).await;
    let store = &ctx.store;
    let node_id = ctx.node_id;
    let line_id = ctx.line_id;
    let endpoint_id = ctx.endpoint_id;
    let payload = json!({
        "access_node_id": node_id,
        "line_probes": [
            {
                "access_line_id": line_id,
                "status": "healthy",
                "latency_ms": 111,
                "probed_at_unix": 1_700_100_001
            },
            {
                "access_line_id": line_id,
                "status": "healthy",
                "latency_ms": 222,
                "probed_at_unix": 1_700_100_002
            }
        ],
        "exit_probes": [
            {
                "exit_endpoint_id": endpoint_id,
                "status": "healthy",
                "latency_ms": 333,
                "probed_at_unix": 1_700_100_001
            },
            {
                "exit_endpoint_id": endpoint_id,
                "status": "healthy",
                "latency_ms": 444,
                "probed_at_unix": 1_700_100_002
            }
        ]
    });

    let result = store.record_agent_probes(node_id, &payload).await.unwrap();
    assert_eq!(result["line_probes"], 1);
    assert_eq!(result["exit_probes"], 1);

    let retry_result = store.record_agent_probes(node_id, &payload).await.unwrap();
    assert_eq!(retry_result["line_probes"], 0);
    assert_eq!(retry_result["exit_probes"], 0);

    let line_rows: (i64, Option<i32>) = sqlx::query_as(
        r#"
        SELECT COUNT(*), MAX(latency_ms)
        FROM access_line_probes
        WHERE access_line_id = $1
        "#,
    )
    .bind(line_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(line_rows, (1, Some(222)));

    let exit_rows: (i64, Option<i32>) = sqlx::query_as(
        r#"
        SELECT COUNT(*), MAX(latency_ms)
        FROM access_exit_probes
        WHERE access_node_id = $1 AND exit_endpoint_id = $2
        "#,
    )
    .bind(node_id)
    .bind(endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(exit_rows, (1, Some(444)));
}

#[tokio::test]
async fn test_pg_agent_sessions_keep_previous_users_when_snapshot_is_partial() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL partial session test");
        return;
    };
    let _guard = pg_test_guard().await;
    let ctx = setup_runtime_report_test_context(&database_url).await;
    let store = &ctx.store;
    let node_id = ctx.node_id;
    let line_id = ctx.line_id;
    let first_seen = Utc::now().timestamp();
    let second_user_key = "partial-session-user-2@xrayc.local";
    let first_payload = json!({
        "access_node_id": node_id,
        "sessions": [
            {
                "access_line_id": line_id,
                "xray_user_key": ctx.user_key,
                "client_ip_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "active_connection_count": 1,
                "started_at_unix": first_seen - 1,
                "last_seen_at_unix": first_seen
            },
            {
                "access_line_id": line_id,
                "xray_user_key": second_user_key,
                "client_ip_hash": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "active_connection_count": 1,
                "started_at_unix": first_seen - 1,
                "last_seen_at_unix": first_seen
            }
        ]
    });
    assert_eq!(
        store
            .record_agent_sessions(node_id, &first_payload)
            .await
            .unwrap(),
        2
    );

    let partial_payload = json!({
        "access_node_id": node_id,
        "sessions": [{
            "access_line_id": line_id,
            "xray_user_key": ctx.user_key,
            "client_ip_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "active_connection_count": 2,
            "started_at_unix": first_seen - 1,
            "last_seen_at_unix": first_seen + 5
        }]
    });
    assert_eq!(
        store
            .record_agent_sessions(node_id, &partial_payload)
            .await
            .unwrap(),
        1
    );

    let rows: Vec<(String, i32)> = sqlx::query_as(
        r#"
        SELECT xray_user_key, active_connection_count
        FROM access_user_sessions
        WHERE access_node_id = $1
        ORDER BY xray_user_key
        "#,
    )
    .bind(node_id)
    .fetch_all(store.pool())
    .await
    .unwrap();

    assert_eq!(
        rows,
        vec![
            (second_user_key.to_string(), 1),
            (ctx.user_key.to_string(), 2),
        ]
    );
}

#[tokio::test]
async fn test_pg_agent_sessions_accepts_large_snapshot_and_resolves_users_in_bulk() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL bulk session test");
        return;
    };
    let _guard = pg_test_guard().await;
    let ctx = setup_runtime_report_test_context(&database_url).await;
    let store = &ctx.store;
    let node_id = ctx.node_id;
    let line_id = ctx.line_id;
    let marker = format!("bulk-session-{}", Uuid::new_v4().simple());
    let now_ts = Utc::now().timestamp();
    let sessions = (0..200)
        .map(|index| {
            let user_key = if index == 0 {
                ctx.user_key.to_string()
            } else {
                format!("{marker}-{index}@xrayc.local")
            };
            json!({
                "access_line_id": line_id,
                "xray_user_key": user_key,
                "client_ip_hash": format!("sha256:{index:064x}"),
                "active_connection_count": index + 1,
                "started_at_unix": now_ts - 1,
                "last_seen_at_unix": now_ts + i64::from(index),
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "access_node_id": node_id,
        "sessions": sessions,
    });

    assert_eq!(
        store.record_agent_sessions(node_id, &payload).await.unwrap(),
        200
    );

    let online_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM access_user_sessions
        WHERE access_node_id = $1
          AND access_line_id = $2
          AND (xray_user_key = $3 OR xray_user_key LIKE $4)
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(ctx.user_key)
    .bind(format!("{marker}-%"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(online_count, 200);

    let event_rows: (i64, i64) = sqlx::query_as(
        r#"
        SELECT COUNT(*), COUNT(*) FILTER (WHERE user_id = $3)
        FROM access_user_session_events
        WHERE access_node_id = $1
          AND access_line_id = $2
          AND (xray_user_key = $4 OR xray_user_key LIKE $5)
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(ctx.user_id)
    .bind(ctx.user_key)
    .bind(format!("{marker}-%"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(event_rows, (200, 1));
}

#[tokio::test]
async fn test_pg_agent_sessions_concurrent_reports_are_idempotent_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL concurrent session test");
        return;
    };
    let _guard = pg_test_guard().await;
    let ctx = setup_runtime_report_test_context(&database_url).await;
    let store = &ctx.store;
    let node_id = ctx.node_id;
    let line_id = ctx.line_id;
    let marker = format!("concurrent-session-{}", Uuid::new_v4().simple());
    let now_ts = Utc::now().timestamp();
    let sessions = (0..200)
        .map(|index| {
            json!({
                "access_line_id": line_id,
                "xray_user_key": format!("{marker}-{index}@xrayc.local"),
                "client_ip_hash": format!("sha256:{index:064x}"),
                "active_connection_count": index + 1,
                "started_at_unix": now_ts - 1,
                "last_seen_at_unix": now_ts + i64::from(index),
            })
        })
        .collect::<Vec<_>>();
    let payload = Arc::new(json!({
        "access_node_id": node_id,
        "sessions": sessions,
    }));
    let barrier = Arc::new(Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        let payload = Arc::clone(&payload);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.record_agent_sessions(node_id, payload.as_ref()).await
        }));
    }

    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap(), 200);
    }

    let online_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM access_user_sessions
        WHERE access_node_id = $1
          AND access_line_id = $2
          AND xray_user_key LIKE $3
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(format!("{marker}-%"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(online_count, 200);

    let event_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM access_user_session_events
        WHERE access_node_id = $1
          AND access_line_id = $2
          AND xray_user_key LIKE $3
        "#,
    )
    .bind(node_id)
    .bind(line_id)
    .bind(format!("{marker}-%"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(event_count, 1600);
}

#[tokio::test]
async fn test_pg_usage_rollup_tracks_insert_and_delete_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL usage rollup test");
        return;
    };
    let _guard = pg_test_guard().await;
    let ctx = setup_runtime_report_test_context(&database_url).await;
    let store = &ctx.store;
    let marker = format!("usage-rollup-{}", Uuid::new_v4());
    sqlx::query("DELETE FROM access_line_usage_rollups WHERE access_line_id = $1")
        .bind(ctx.line_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO usage_ledgers (
            access_line_id, user_id, xray_user_key, traffic_source,
            delta_uplink, delta_downlink, billing_multiplier,
            billed_bytes, delta_total, billed_uplink, billed_downlink,
            collected_at, recorded_at, exit_endpoint_id
        )
        VALUES
            ($1, $2, $3 || '-a', 'access_line', 100, 200, 1.000, 300, 300, 100, 200, now() - interval '1 minute', now(), $4),
            ($1, $2, $3 || '-b', 'access_line', 400, 500, 1.000, 900, 900, 400, 500, now(), now(), $4)
        "#,
    )
    .bind(ctx.line_id)
    .bind(ctx.user_id)
    .bind(&marker)
    .bind(ctx.endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();

    let ranking = store.operations_ledger_ranking_json(10).await.unwrap();
    let line = ranking["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["access_line_id"] == ctx.line_id.to_string())
        .unwrap();
    assert_eq!(line["ledger_count"], 2);
    assert_eq!(line["billed_bytes"], 1200);
    assert_eq!(ranking["totals"]["billed_bytes"], 1200);

    sqlx::query("DELETE FROM usage_ledgers WHERE xray_user_key LIKE $1")
        .bind(format!("{marker}%"))
        .execute(store.pool())
        .await
        .unwrap();
    let remaining: Option<i64> = sqlx::query_scalar(
        "SELECT billed_bytes FROM access_line_usage_rollups WHERE access_line_id = $1",
    )
    .bind(ctx.line_id)
    .fetch_optional(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining, None);
}

#[tokio::test]
async fn test_pg_subscription_offline_probe_filter_is_admin_switch_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL subscription probe filter test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let demo_user_id = uuid("00000000-0000-0000-0000-000000000001");
    let plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let line_id = uuid("00000000-0000-0000-0000-000000000501");
    let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
    let access_node_id = uuid("00000000-0000-0000-0000-000000000201");
    let line_group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("probe-filter-group-{}", uuid::Uuid::new_v4().simple()),
            country_code: "HK".to_string(),
            icon: "🇭🇰".to_string(),
            group_level: None,
            parent_group_id: None,
            sort_weight: Some(100),
            billing_multiplier: None,
            enabled: Some(true),
                dedicated_rules: None,
                rule_set_bindings: None,
        })
        .await
        .unwrap();
    sqlx::query("UPDATE access_lines SET line_group_id = $2, enabled = TRUE WHERE id = $1")
        .bind(line_id)
        .bind(line_group_id)
        .execute(store.pool())
        .await
        .unwrap();
    store
        .replace_admin_line_group_lines(line_group_id, vec![endpoint_id])
        .await
        .unwrap();
    store
        .replace_admin_plan_line_groups(
            plan_id,
            vec![AdminPlanLineGroupInput {
                line_group_id,
                billing_multiplier: None,
            }],
        )
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO access_exit_probe_states (
            access_node_id, exit_endpoint_id, effective_status,
            consecutive_failures, last_probe_status, updated_at
        )
        VALUES ($1, $2, 'offline', 3, 'failed', now())
        ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
            effective_status = EXCLUDED.effective_status,
            consecutive_failures = EXCLUDED.consecutive_failures,
            last_probe_status = EXCLUDED.last_probe_status,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(access_node_id)
    .bind(endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();

    let default_yaml = store
        .generate_subscription_yaml("demo-token")
        .await
        .expect("offline probe should not hide subscription lines by default");
    assert!(default_yaml.contains("香港 01"));
    assert!(!default_yaml.contains("🇭🇰 香港 01"));

    let default_assignments = store
        .user_access_line_assignments(demo_user_id)
        .await
        .unwrap();
    assert!(default_assignments
        .iter()
        .any(|(_, assigned_line_id)| *assigned_line_id == line_id));

    store
        .update_subscription_settings_json(json!({
            "block_unhealthy_lines": true
        }))
        .await
        .unwrap();

    let blocked = store.generate_subscription_yaml("demo-token").await;
    assert!(matches!(
        blocked,
        Err(DbError::Subscription(SubscriptionError::NoAvailableLines))
    ));
}
