/// 数据库测试分片 17。
// 本文件覆盖本机出口服务的 PostgreSQL 创建和显式绑定闭环。
// 测试只在 DATABASE_URL 存在时运行，避免本地无数据库时报错。
// 本机出口服务应保存为 self_hosted 资源并绑定承载中转节点。
// SOCKS/HTTP 账号密码通过字段加密 helper 写入并在读取模型展开。
// 心跳配置必须下发所属节点的本机出口服务。
// 该线路进入线路池后，仍只能通过中转节点绑定具体线路后生成订阅节点。
// 本文件不保存任何真实服务器地址、账号或密码。
// 维护时继续保持中文注释和文件长度限制。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_local_socks_line_is_only_added_to_line_pool_until_explicitly_bound_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local socks service test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let owner_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-socks-owner".to_string(),
            public_host: "192.0.2.1".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-owner-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let created = store
        .create_admin_local_exit_lines(
            owner_node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-socks-resource".to_string(),
                    endpoint_name: "local-socks-endpoint".to_string(),
                    region_code: "LOCAL".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "192.0.2.1".to_string(),
                    port: 38_081,
                    outbound_config: json!({
                        "username": "local-user",
                        "password": "local-pass",
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("local socks line should be created");

    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .expect("created endpoint id"),
    )
    .unwrap();

    let row = sqlx::query_as::<_, (String, Option<uuid::Uuid>, String, String, i32)>(
        r#"
        SELECT r.ownership, r.access_node_id, e.outbound_type::text AS outbound_type,
               e.host, e.port
        FROM exit_endpoints e
        JOIN exit_resources r ON r.id = e.exit_resource_id
        WHERE e.id = $1
        "#,
    )
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(row.0, "self_hosted");
    assert_eq!(row.1, Some(owner_node_id));
    assert_eq!(row.2, "socks");
    assert_eq!(row.3, "192.0.2.1");
    assert_eq!(row.4, 38_081);

    let access_line_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM access_lines WHERE access_node_id = $1",
    )
    .bind(owner_node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(access_line_count, 0);
    let line_group_line_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM line_group_lines lgl
        JOIN access_lines al ON al.id = lgl.access_line_id
        WHERE al.access_node_id = $1
        "#,
    )
    .bind(owner_node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(line_group_line_count, 0);
    let exit_pool_member_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM exit_pool_members WHERE exit_endpoint_id = $1",
    )
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(exit_pool_member_count, 0);

    let owner_heartbeat = store.heartbeat_json(Some(owner_node_id), None).await.unwrap();
    assert_eq!(owner_heartbeat["config_status"]["required"], true);
    assert_eq!(
        owner_heartbeat["config"]["local_exit_services"][0]["protocol"],
        "socks"
    );
    assert_eq!(
        owner_heartbeat["config"]["local_exit_services"][0]["listen_port"],
        38_081
    );
    assert_eq!(
        owner_heartbeat["config"]["local_exit_services"][0]["username"],
        "local-user"
    );

    let consumer_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-socks-consumer".to_string(),
            public_host: "192.0.2.2".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-consumer-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let line_group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("本机出口测试分组-{}", uuid::Uuid::new_v4().simple()),
            country_code: "LOCAL".to_string(),
            icon: "🌐".to_string(),
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
        .replace_admin_line_group_lines(line_group_id, vec![exit_endpoint_id])
        .await
        .unwrap();
    let result = store
        .create_admin_access_node_group_entries(
            consumer_node_id,
            vec![AdminAccessNodeGroupEntryInput {
                name: "consumer-line".to_string(),
                exit_endpoint_id,
                listen_host: "192.0.2.2".to_string(),
                listen_port: 24_443,
                protocol: "vless".to_string(),
                network_mode: "tcp".to_string(),
                inbound_config: json!({}),
                xhttp_mode: "auto".to_string(),
                enabled: true,
            }],
        )
        .await
        .unwrap();
    let line_id = result.created_line_ids[0];
    let (line_node_id, legacy_group_id, bound_endpoint_id, line_group_refs) =
        sqlx::query_as::<_, (uuid::Uuid, Option<uuid::Uuid>, Option<uuid::Uuid>, i64)>(
        r#"
        SELECT al.access_node_id, al.line_group_id, al.exit_endpoint_id, COUNT(lgl.access_line_id)
        FROM access_lines al
        LEFT JOIN line_group_lines lgl ON lgl.access_line_id = al.id
        WHERE al.id = $1
        GROUP BY al.access_node_id, al.line_group_id, al.exit_endpoint_id
        "#,
    )
    .bind(line_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(line_node_id, consumer_node_id);
    assert_eq!(legacy_group_id, None);
    assert_eq!(bound_endpoint_id, Some(exit_endpoint_id));
    assert_eq!(line_group_refs, 0);
    let demo_user_id = uuid("00000000-0000-0000-0000-000000000001");
    let demo_plan_id = uuid("00000000-0000-0000-0000-000000000101");
    store
        .replace_admin_plan_line_groups(
            demo_plan_id,
            vec![AdminPlanLineGroupInput {
                line_group_id,
                billing_multiplier: None,
            }],
        )
        .await
        .unwrap();
    sqlx::query(
        r#"
        DELETE FROM user_access_line_assignments
        WHERE user_id = $1
        "#,
    )
    .bind(demo_user_id)
    .execute(store.pool())
    .await
    .unwrap();
    store.generate_subscription_yaml("demo-token").await.unwrap();
    store.sync_user_exit_assignments(demo_user_id).await.unwrap();

    let consumer_heartbeat = store
        .heartbeat_json(Some(consumer_node_id), None)
        .await
        .unwrap();
    assert_eq!(
        consumer_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
        "socks"
    );
    assert_eq!(
        consumer_heartbeat["config"]["exit_endpoints"][0]["protocol"]["username"],
        "local-user"
    );
}

#[tokio::test]
async fn test_pg_local_exit_shadowsocks_access_line_gets_subscription_config_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local shadowsocks line test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-ss-owner".to_string(),
            public_host: "192.0.2.3".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-ss-owner-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-ss-resource".to_string(),
                    endpoint_name: "local-ss-endpoint".to_string(),
                    region_code: "LOCAL".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "192.0.2.3".to_string(),
                    port: 34_571,
                    outbound_config: json!({
                        "username": "local-user",
                        "password": "local-pass",
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("local shadowsocks upstream line should be created");
    let exit_endpoint_id =
        uuid::Uuid::parse_str(created["created_lines"][0]["exit_endpoint_id"].as_str().unwrap())
            .unwrap();
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    store
        .replace_admin_line_group_lines(line_group_id, vec![exit_endpoint_id])
        .await
        .unwrap();
    let result = store
        .create_admin_access_node_group_entries(
            node_id,
            vec![AdminAccessNodeGroupEntryInput {
                name: "local-ss-line".to_string(),
                exit_endpoint_id,
                listen_host: "192.0.2.3".to_string(),
                listen_port: 34_570,
                protocol: "shadowsocks".to_string(),
                network_mode: "tcp".to_string(),
                inbound_config: json!({}),
                xhttp_mode: "auto".to_string(),
                enabled: true,
            }],
        )
        .await
        .expect("local shadowsocks access line should be explicitly bound");
    let access_line_id = result.created_line_ids[0];
    let inbound_config =
        sqlx::query_scalar::<_, serde_json::Value>("SELECT inbound_config FROM access_lines WHERE id = $1")
            .bind(access_line_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    assert_eq!(inbound_config["method"], "2022-blake3-aes-128-gcm");
    assert!(inbound_config["password"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty()));
    assert_eq!(inbound_config["network"], "tcp,udp");
}

#[tokio::test]
async fn test_pg_local_exit_shadowsocks_2022_password_is_auto_generated_when_database_url_is_set()
{
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local shadowsocks password test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-ss-password-owner".to_string(),
            public_host: "192.0.2.31".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-ss-password-owner-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-ss-password-resource".to_string(),
                    endpoint_name: "local-ss-password-endpoint".to_string(),
                    region_code: "LOCAL".to_string(),
                    outbound_type: "shadowsocks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "192.0.2.31".to_string(),
                    port: 34_573,
                    outbound_config: json!({
                        "method": "2022-blake3-aes-128-gcm",
                        "password": "ss-not-base64"
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("local shadowsocks password should be normalized");
    let exit_endpoint_id =
        uuid::Uuid::parse_str(created["created_lines"][0]["exit_endpoint_id"].as_str().unwrap())
            .unwrap();

    let raw_config =
        sqlx::query_scalar::<_, serde_json::Value>("SELECT outbound_config FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let config = raw_config;
    let password = config["password"].as_str().unwrap();
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        password,
    )
    .expect("generated shadowsocks password should be base64");
    assert_eq!(decoded.len(), 16);

    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    assert_eq!(
        heartbeat["config"]["local_exit_services"][0]["protocol"],
        "shadowsocks"
    );
    assert_eq!(
        heartbeat["config"]["local_exit_services"][0]["method"],
        "2022-blake3-aes-128-gcm"
    );
}
