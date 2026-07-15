/// 数据库测试分片 18。
// 本文件覆盖运行出口集合整组删除的 PostgreSQL 闭环。
// 删除运行出口集合时只清理集合、成员关系和中转入口。
// 线路池里的出口资源、出口 endpoint、地址和凭据必须保留。
// 绑定该分组的中转节点需要标记配置待同步。
// 历史账本不能阻塞删除，access_line_id 应按迁移规则置空。
// 测试只在 DATABASE_URL 存在时运行，避免无数据库环境报错。
// 这里不保存任何真实服务器地址、账号或密码。
// 维护时继续保持中文注释和文件长度限制。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_delete_exit_pool_keeps_line_pool_endpoint_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL exit pool delete test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("delete-pool-node-{}", Uuid::new_v4().simple()),
            public_host: "198.51.100.71".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("delete-pool-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let owner_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("delete-pool-owner-{}", Uuid::new_v4().simple()),
            public_host: "198.51.100.72".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("delete-pool-owner-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let exit_resource_id = store
        .create_admin_exit_resource(AdminExitResourceInput {
            name: format!("delete-pool-resource-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            provider_name: "delete-pool-test".to_string(),
            ownership: "third_party".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    sqlx::query(
        "UPDATE exit_resources SET ownership = 'self_hosted', access_node_id = $2 WHERE id = $1",
    )
    .bind(exit_resource_id)
    .bind(owner_node_id)
    .execute(store.pool())
    .await
    .unwrap();
    let exit_endpoint_id = store
        .create_admin_exit_endpoint(AdminExitEndpointInput {
            exit_resource_id,
            name: "delete-pool-endpoint".to_string(),
            outbound_type: "socks".to_string(),
            host: "203.0.113.71".to_string(),
            port: 10_871,
            outbound_config: json!({"username": "test-user", "password": "test-pass"}),
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
        })
        .await
        .unwrap();
    let exit_pool_id = store
        .create_admin_exit_pool(AdminExitPoolInput {
            name: format!("delete-pool-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            strategy: "priority".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    store
        .replace_admin_exit_pool_members(
            exit_pool_id,
            vec![AdminExitPoolMemberInput {
                exit_endpoint_id,
                weight: 100,
                priority: 100,
                status: "healthy".to_string(),
                allow_new_assignments: true,
            }],
        )
        .await
        .unwrap();
    let access_line_id = store
        .create_admin_access_line(AdminAccessLineInput {
            name: "delete-pool-line".to_string(),
            access_node_id,
            exit_pool_id,
            listen_host: "198.51.100.71".to_string(),
            listen_port: 34_871,
            protocol: "vless".to_string(),
            transport: "tcp".to_string(),
            user_uuid: Uuid::new_v4().to_string(),
            server_name: String::new(),
            public_key: String::new(),
            short_id: String::new(),
            enabled: true,
            region_code: "TST".to_string(),
            region_name: "TST".to_string(),
            region_flag: String::new(),
            flow: String::new(),
            udp_enabled: true,
            udp_packet_encoding: String::new(),
            xhttp_path: String::new(),
            xhttp_host: String::new(),
            xhttp_mode: "auto".to_string(),
            identity_mode: "credential".to_string(),
            user_key_source: "xray_email".to_string(),
            inbound_config: json!({}),
            visibility_weight: 100,
        })
        .await
        .unwrap();
    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    sqlx::query(
        r#"
        INSERT INTO user_exit_assignments (
            user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
        )
        VALUES ($1, $2, $3, $4, 'delete-pool-test')
        ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
            exit_endpoint_id = EXCLUDED.exit_endpoint_id
        "#,
    )
    .bind(user_id)
    .bind(access_line_id)
    .bind(exit_pool_id)
    .bind(exit_endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();
    let ledger_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO usage_ledgers (
            access_line_id, user_id, xray_user_key, traffic_source,
            delta_uplink, delta_downlink, billing_multiplier, billed_bytes, collected_at
        )
        VALUES ($1, $2, 'delete-pool-key', 'access_line', 1, 2, 1.000, 3, now())
        RETURNING id
        "#,
    )
    .bind(access_line_id)
    .bind(user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();

    let result = store.delete_admin_exit_pool(exit_pool_id).await.unwrap();
    assert!(result.deleted);
    assert_eq!(result.deleted_access_line_count, 1);
    assert_eq!(result.deleted_member_count, 1);
    assert_eq!(result.deleted_assignment_count, 1);
    assert_eq!(result.retained_usage_ledger_count, 1);
    assert_eq!(result.affected_access_node_count, 2);

    let pool_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM exit_pools WHERE id = $1")
        .bind(exit_pool_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let line_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM access_lines WHERE id = $1")
            .bind(access_line_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let endpoint_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let resource_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM exit_resources WHERE id = $1")
            .bind(exit_resource_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(pool_count, 0);
    assert_eq!(line_count, 0);
    assert_eq!(endpoint_count, 1);
    assert_eq!(resource_count, 1);

    let ledger_line_id =
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT access_line_id FROM usage_ledgers WHERE id = $1")
            .bind(ledger_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(ledger_line_id, None);
    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(access_node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(dirty_reason, "admin_deleted_exit_pool");
    let owner_dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(owner_node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(owner_dirty_reason, "admin_deleted_exit_pool");
}

#[tokio::test]
async fn test_pg_delete_exit_endpoint_disables_empty_pool_lines_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL exit endpoint delete test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("delete-endpoint-node-{}", Uuid::new_v4().simple()),
            public_host: "198.51.100.81".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("delete-endpoint-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let exit_resource_id = store
        .create_admin_exit_resource(AdminExitResourceInput {
            name: format!("delete-endpoint-resource-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            provider_name: "delete-endpoint-test".to_string(),
            ownership: "third_party".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    let exit_endpoint_id = store
        .create_admin_exit_endpoint(AdminExitEndpointInput {
            exit_resource_id,
            name: "delete-endpoint-target".to_string(),
            outbound_type: "socks".to_string(),
            host: "203.0.113.81".to_string(),
            port: 10_881,
            outbound_config: json!({"username": "test-user", "password": "test-pass"}),
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
        })
        .await
        .unwrap();
    let exit_pool_id = store
        .create_admin_exit_pool(AdminExitPoolInput {
            name: format!("delete-endpoint-pool-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            strategy: "priority".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    store
        .replace_admin_exit_pool_members(
            exit_pool_id,
            vec![AdminExitPoolMemberInput {
                exit_endpoint_id,
                weight: 100,
                priority: 100,
                status: "healthy".to_string(),
                allow_new_assignments: true,
            }],
        )
        .await
        .unwrap();
    let access_line_id = store
        .create_admin_access_line(AdminAccessLineInput {
            name: "delete-endpoint-line".to_string(),
            access_node_id,
            exit_pool_id,
            listen_host: "198.51.100.81".to_string(),
            listen_port: 34_881,
            protocol: "vless".to_string(),
            transport: "tcp".to_string(),
            user_uuid: Uuid::new_v4().to_string(),
            server_name: String::new(),
            public_key: String::new(),
            short_id: String::new(),
            enabled: true,
            region_code: "TST".to_string(),
            region_name: "TST".to_string(),
            region_flag: String::new(),
            flow: String::new(),
            udp_enabled: true,
            udp_packet_encoding: String::new(),
            xhttp_path: String::new(),
            xhttp_host: String::new(),
            xhttp_mode: "auto".to_string(),
            identity_mode: "credential".to_string(),
            user_key_source: "xray_email".to_string(),
            inbound_config: json!({}),
            visibility_weight: 100,
        })
        .await
        .unwrap();
    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    sqlx::query(
        "INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id) VALUES ($1, (SELECT line_group_id FROM plan_line_groups LIMIT 1), $2) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(access_line_id)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_exit_assignments (user_id, access_line_id, exit_pool_id, exit_endpoint_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(access_line_id)
    .bind(exit_pool_id)
    .bind(exit_endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();

    let deleted_count = store.delete_admin_exit_endpoint(exit_endpoint_id).await.unwrap();
    assert_eq!(deleted_count, 1);

    let endpoint_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let resource_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM exit_resources WHERE id = $1")
            .bind(exit_resource_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let line_enabled = sqlx::query_scalar::<_, bool>("SELECT enabled FROM access_lines WHERE id = $1")
        .bind(access_line_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let access_assignment_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_access_line_assignments WHERE access_line_id = $1",
    )
    .bind(access_line_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let exit_assignment_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_exit_assignments WHERE exit_endpoint_id = $1",
    )
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(access_node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    assert_eq!(endpoint_count, 0);
    assert_eq!(resource_count, 0);
    assert!(!line_enabled);
    assert_eq!(access_assignment_count, 0);
    assert_eq!(exit_assignment_count, 0);
    assert_eq!(dirty_reason, "admin_deleted_exit_endpoint");
}
