// 数据库测试分片 67（压测 ordering BUG 取证 + 回归）。
// 本文件复现：建好「入口出口绑定」后，再做分组绑定节点 / 套餐授权分组，
// 不得删掉该绑定 per-binding exit_pool 的 exit_pool_members（否则线路无出口、订阅整单不可用）。
// 取证阶段在每个候选写操作后逐步断言成员存活，钉死真正删成员的那一步。
// 测试只使用隔离 PostgreSQL，不访问远端节点。
// DATABASE_URL 缺失时只输出脱敏跳过原因。
// 断言不依赖真实服务器地址或私有凭据。
// 后续 ordering / 级联回归测试优先追加到本分片。
// 本文件通过父模块 include 聚合，不定义独立模块。
// 本头部满足前十行中文注释约束。

/// 取本绑定 per-binding exit_pool 内、指向本出口、且 healthy 的成员条数。
/// 正确状态恒为 1；变成 0 即说明出口被误删，线路渲染不出出口。
async fn binding_pool_healthy_member_count(
    store: &PgStore,
    binding_id: Uuid,
    exit_endpoint_id: Uuid,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM access_lines l
        JOIN exit_pool_members m
          ON m.exit_pool_id = l.exit_pool_id
         AND m.exit_endpoint_id = l.exit_endpoint_id
        WHERE l.id = $1
          AND l.exit_endpoint_id = $2
          AND m.status = 'healthy'
          AND m.allow_new_assignments = TRUE
        "#,
    )
    .bind(binding_id)
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn test_pg_binding_exit_survives_group_and_plan_authorization_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL binding-exit survival test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("ordering-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.40".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("ordering-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "ordering-resource".to_string(),
                    endpoint_name: "ordering-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.40".to_string(),
                    port: 38_240,
                    outbound_config: json!({"username": "u", "password": "p"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "ordering-vless-entry".to_string(),
            listen_host: String::new(),
            listen_port: 443,
            protocol: "vless".to_string(),
            transport: "tcp".to_string(),
            security: "reality".to_string(),
            server_name: "www.cloudflare.com".to_string(),
            ws_path: String::new(),
            ws_host: String::new(),
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            enabled: true,
            sort_weight: 100,
            node_domain_id: None,
            vless_quantum_encryption: false,
        })
        .await
        .unwrap();
    let binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id,
                name: "ordering-binding".to_string(),
                enabled: true,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();

    // 建好绑定后：per-binding exit_pool 必须恰有一条 healthy 成员（线路渲染出出口的前提）。
    assert_eq!(
        binding_pool_healthy_member_count(&store, binding_id, exit_endpoint_id).await,
        1,
        "建绑定后该线路的 per-binding 出口成员应存在"
    );

    // 一层分组：绑定该节点。
    let group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("ordering-group-{}", Uuid::new_v4().simple()),
            country_code: "US".to_string(),
            icon: String::new(),
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

    // 候选 1：主路径「分组绑定节点」replace。
    store
        .replace_admin_line_group_binding_nodes(group_id, vec![binding_id])
        .await
        .unwrap();
    assert_eq!(
        binding_pool_healthy_member_count(&store, binding_id, exit_endpoint_id).await,
        1,
        "分组绑定节点后出口成员被删了（候选 1 命中）"
    );

    // 候选 2：兼容路径「分组绑定线路」replace（用同一出口端点）。
    store
        .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
        .await
        .unwrap();
    assert_eq!(
        binding_pool_healthy_member_count(&store, binding_id, exit_endpoint_id).await,
        1,
        "分组绑定线路后出口成员被删了（候选 2 命中）"
    );

    // 候选 3：套餐授权分组（把分组挂到一个套餐上）。
    let plan_id = Uuid::parse_str(
        store
            .create_admin_plan_json(AdminPlanInput {
                name: format!("ordering-plan-{}", Uuid::new_v4().simple()),
                traffic_limit_bytes: 100_i64 * 1024 * 1024 * 1024,
                rate_limit_bps: 0,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
                billing_multiplier: 1.0,
                enabled: true,
                price_cents: 0,
                currency: "USDT".to_string(),
                duration_days: 30,
                sort_weight: 100,
            })
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    store
        .replace_admin_plan_line_groups(
            plan_id,
            vec![AdminPlanLineGroupInput {
                line_group_id: group_id,
                billing_multiplier: None,
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        binding_pool_healthy_member_count(&store, binding_id, exit_endpoint_id).await,
        1,
        "套餐授权分组后出口成员被删了（候选 3 命中）"
    );
}

/// 取证（命中 BUG）：seed 把 line_groups.exit_pool_id 与该绑定 access_line.exit_pool_id 设成同一池
/// （历史「分组池」模型），分组里又含一个具体出口端点 binding（501→302）。
/// 此时对该分组走「分组绑定线路」replace 成另一个出口端点，`sync_line_group_exit_pool_in_tx`
/// 会在这个被共享的池上执行 `DELETE exit_pool_members WHERE pool=共享池 AND NOT endpoint=ANY(新端点)`，
/// 把原 binding 端点(302)的成员一并删掉 → 该 binding 的 access_line 没有出口 → 入口不可用。
/// 这就是压测发现的 ordering BUG 的最小复现（未修复时本断言失败）。
#[tokio::test]
async fn test_pg_seeded_group_line_replace_keeps_binding_exit_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL seeded shared-pool repro");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let group_id = uuid("00000000-0000-0000-0000-000000000601");
    let binding_id = uuid("00000000-0000-0000-0000-000000000501");
    let seeded_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");

    // 前置取证：seed 让分组池 == 该 binding 线路池（共享同一 exit_pool）。
    let group_pool = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT exit_pool_id FROM line_groups WHERE id = $1",
    )
    .bind(group_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let line_pool = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT exit_pool_id FROM access_lines WHERE id = $1",
    )
    .bind(binding_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    eprintln!(
        "[diagnostic] seeded group_pool={group_pool:?} binding_line_pool={line_pool:?} shared={}",
        group_pool == line_pool && group_pool.is_some()
    );
    assert_eq!(
        binding_pool_healthy_member_count(&store, binding_id, seeded_endpoint_id).await,
        1,
        "seed 后该 binding 应有出口成员"
    );

    // 在同节点再建一个不同的出口端点，作为分组绑定线路 replace 的新成员。
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "seeded-repro-resource".to_string(),
                    endpoint_name: "seeded-repro-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.50".to_string(),
                    port: 38_250,
                    outbound_config: json!({"username": "u", "password": "p"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let other_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    // 触发：把分组成员 replace 成「另一个出口端点」（不含原 binding 端点 302）。
    store
        .replace_admin_line_group_lines(group_id, vec![other_endpoint_id])
        .await
        .unwrap();

    // 期望（修复后）：原 binding 的 access_line 仍保有自己的出口成员。
    let after = binding_pool_healthy_member_count(&store, binding_id, seeded_endpoint_id).await;
    eprintln!("[diagnostic] after group-lines replace: binding healthy_members={after}");
    assert_eq!(
        after, 1,
        "分组绑定线路 replace 把共享池里原 binding 端点的成员删掉了 → 线路无出口（BUG 复现）"
    );
}

/// 取证：两个绑定共享同一出口端点 → 共享同一 per-binding exit_pool。
/// 对其中一个绑定走「分组绑定线路」/重存等路径时，另一个绑定的出口成员是否被波及。
#[tokio::test]
async fn test_pg_shared_endpoint_bindings_diagnostic_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL shared-endpoint binding diagnostic");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("shared-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.41".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("shared-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "shared-resource".to_string(),
                    endpoint_name: "shared-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.41".to_string(),
                    port: 38_241,
                    outbound_config: json!({"username": "u", "password": "p"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    // 两个入口都绑定同一出口端点。
    let mut binding_ids = Vec::new();
    for (idx, port) in [(0usize, 443u16), (1usize, 8443u16)] {
        let entry_id = store
            .create_admin_access_entry(AdminAccessEntryInput {
                access_node_id: node_id,
                name: format!("shared-entry-{idx}"),
                listen_host: String::new(),
                listen_port: port,
                protocol: "vless".to_string(),
                transport: "tcp".to_string(),
                security: "reality".to_string(),
                server_name: "www.cloudflare.com".to_string(),
                ws_path: String::new(),
                ws_host: String::new(),
                cdn_enabled: false,
                cdn_provider: String::new(),
                cdn_hostname: String::new(),
                cdn_server: String::new(),
                enabled: true,
                sort_weight: 100,
                node_domain_id: None,
                vless_quantum_encryption: false,
            })
            .await
            .unwrap();
        let binding_id = store
            .create_admin_access_entry_exit_binding(
                entry_id,
                AdminAccessEntryExitBindingInput {
                    exit_endpoint_id,
                    name: format!("shared-binding-{idx}"),
                    enabled: true,
                    sort_weight: 100,
                    remark: String::new(),
                },
            )
            .await
            .unwrap();
        binding_ids.push(binding_id);
    }

    // 取证：两个绑定的 access_line 是否落在同一 exit_pool（共享池）。
    let pool_a = sqlx::query_scalar::<_, Uuid>("SELECT exit_pool_id FROM access_lines WHERE id = $1")
        .bind(binding_ids[0])
        .fetch_one(store.pool())
        .await
        .unwrap();
    let pool_b = sqlx::query_scalar::<_, Uuid>("SELECT exit_pool_id FROM access_lines WHERE id = $1")
        .bind(binding_ids[1])
        .fetch_one(store.pool())
        .await
        .unwrap();
    eprintln!(
        "[diagnostic] shared-endpoint pools: a={pool_a} b={pool_b} same={}",
        pool_a == pool_b
    );

    for binding_id in &binding_ids {
        assert_eq!(
            binding_pool_healthy_member_count(&store, *binding_id, exit_endpoint_id).await,
            1,
            "两个共享端点绑定建好后都应各有出口成员"
        );
    }

    // 分组只绑定第一个 binding 的 line（走兼容「分组绑定线路」），观察第二个 binding 是否受损。
    let group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("shared-group-{}", Uuid::new_v4().simple()),
            country_code: "US".to_string(),
            icon: String::new(),
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
    store
        .replace_admin_line_group_lines(group_id, vec![exit_endpoint_id])
        .await
        .unwrap();

    let group_pool = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT exit_pool_id FROM line_groups WHERE id = $1",
    )
    .bind(group_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    eprintln!(
        "[diagnostic] group_pool={group_pool:?} pool_a={pool_a} collide={}",
        group_pool == Some(pool_a)
    );

    for binding_id in &binding_ids {
        let count = binding_pool_healthy_member_count(&store, *binding_id, exit_endpoint_id).await;
        eprintln!("[diagnostic] after group-lines: binding {binding_id} healthy_members={count}");
        assert_eq!(count, 1, "分组绑定线路后共享端点绑定 {binding_id} 出口成员被删");
    }
}
