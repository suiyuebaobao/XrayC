/// 数据库测试分片 26。
// 本文件覆盖线路池出口修改后的级联一致性。
// 管理员停用出口端点或出口资源后，相关入口不能继续出现在订阅。
// 用户出口映射和入口授权分配必须同步清理。
// 关联中转节点要标记配置待同步，等待 agent 下一次拉取。
// 测试只使用 RFC 5737 示例地址，不保存真实服务器信息。
// 每个用例都在 DATABASE_URL 存在时运行。
// 这里不测试 UI，只验证 PostgreSQL 事务闭环。
// 维护时继续保持中文注释和文件长度限制。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_disabling_exit_endpoint_clears_related_line_state_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping endpoint consistency test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let (access_node_id, _resource_id, endpoint_id, _pool_id, access_line_id) =
        create_exit_consistency_fixture(&store, "endpoint").await;
    attach_user_to_exit_consistency_fixture(&store, endpoint_id, access_line_id).await;

    store
        .update_admin_exit_endpoint(
            endpoint_id,
            AdminExitEndpointUpdate {
                exit_resource_id: None,
                name: None,
                outbound_type: None,
                host: None,
                port: None,
                outbound_config: None,
                stream_config: None,
                probe_config: None,
                enabled: Some(false),
            },
        )
        .await
        .unwrap();

    assert_exit_consistency_pruned(&store, access_node_id, endpoint_id, access_line_id).await;
}

#[tokio::test]
async fn test_pg_disabling_exit_resource_clears_related_line_state_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping resource consistency test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let (access_node_id, resource_id, endpoint_id, _pool_id, access_line_id) =
        create_exit_consistency_fixture(&store, "resource").await;
    attach_user_to_exit_consistency_fixture(&store, endpoint_id, access_line_id).await;

    store
        .update_admin_exit_resource(
            resource_id,
            AdminExitResourceUpdate {
                name: None,
                region_code: None,
                provider_name: None,
                ownership: None,
                enabled: Some(false),
            },
        )
        .await
        .unwrap();

    assert_exit_consistency_pruned(&store, access_node_id, endpoint_id, access_line_id).await;
}

#[tokio::test]
async fn test_pg_offline_pool_member_clears_related_line_state_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping pool member consistency test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let (access_node_id, _resource_id, endpoint_id, pool_id, access_line_id) =
        create_exit_consistency_fixture(&store, "pool-member").await;
    attach_user_to_exit_consistency_fixture(&store, endpoint_id, access_line_id).await;

    store
        .replace_admin_exit_pool_members(
            pool_id,
            vec![AdminExitPoolMemberInput {
                exit_endpoint_id: endpoint_id,
                weight: 100,
                priority: 100,
                status: "offline".to_string(),
                allow_new_assignments: true,
            }],
        )
        .await
        .unwrap();

    assert_exit_consistency_pruned(&store, access_node_id, endpoint_id, access_line_id).await;
}

#[tokio::test]
async fn test_pg_offline_exit_resource_is_not_loaded_as_healthy_member_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping resource health load test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let (_access_node_id, resource_id, endpoint_id, pool_id, _access_line_id) =
        create_exit_consistency_fixture(&store, "resource-status").await;
    sqlx::query("UPDATE exit_resources SET status = 'offline' WHERE id = $1")
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();

    let data = store.load_store_data().await.unwrap();
    let member = data
        .exit_pools
        .get(&pool_id)
        .unwrap()
        .members
        .iter()
        .find(|member| member.id == endpoint_id)
        .unwrap();
    assert!(!member.healthy);
}

#[tokio::test]
async fn test_pg_self_hosted_hy2_probe_task_includes_udp_target_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping self-hosted HY2 probe task test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("consistency-node-hy2-{}", Uuid::new_v4().simple()),
            public_host: "198.51.100.92".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("consistency-token-hy2-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let resource_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO exit_resources (
            id, name, region_code, provider_name, ownership,
            access_node_id, enabled, status
        )
        VALUES ($1, $2, 'TST', 'consistency-test', 'self_hosted', $3, TRUE, 'unknown')
        "#,
    )
    .bind(resource_id)
    .bind(format!(
        "consistency-resource-hy2-{}",
        Uuid::new_v4().simple()
    ))
    .bind(access_node_id)
    .execute(store.pool())
    .await
    .unwrap();
    let endpoint_id = store
        .create_admin_exit_endpoint(AdminExitEndpointInput {
            exit_resource_id: resource_id,
            name: "consistency-endpoint-hy2".to_string(),
            outbound_type: "hy2".to_string(),
            host: "127.0.0.1".to_string(),
            port: 38_443,
            outbound_config: json!({
                "password": "hy2-test-password",
                "server_name": "hy2.example.test",
                "allow_insecure": true
            }),
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
        })
        .await
        .unwrap();

    let queued = store
        .record_admin_exit_endpoint_probe_request(endpoint_id)
        .await
        .unwrap();
    let access_node_id_text = access_node_id.to_string();
    assert_eq!(
        queued["queued_access_nodes"][0].as_str(),
        Some(access_node_id_text.as_str())
    );
    let heartbeat = store.heartbeat_json(Some(access_node_id), None).await.unwrap();
    let task = &heartbeat["probe_tasks"][0];
    let endpoint_id_text = endpoint_id.to_string();

    assert_eq!(
        task["exit_endpoint_id"].as_str(),
        Some(endpoint_id_text.as_str())
    );
    assert_eq!(task["target"]["protocol"], "hysteria");
    assert_eq!(task["target"]["address"], "127.0.0.1");
    assert_eq!(task["target"]["port"], 38_443);
}

async fn create_exit_consistency_fixture(
    store: &PgStore,
    suffix: &str,
) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("consistency-node-{suffix}-{}", Uuid::new_v4().simple()),
            public_host: "198.51.100.91".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("consistency-token-{suffix}-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let resource_id = store
        .create_admin_exit_resource(AdminExitResourceInput {
            name: format!("consistency-resource-{suffix}-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            provider_name: "consistency-test".to_string(),
            ownership: "third_party".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    let endpoint_id = store
        .create_admin_exit_endpoint(AdminExitEndpointInput {
            exit_resource_id: resource_id,
            name: format!("consistency-endpoint-{suffix}"),
            outbound_type: "socks".to_string(),
            host: "203.0.113.91".to_string(),
            port: 10_891,
            outbound_config: json!({"username": "test-user", "password": "test-pass"}),
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
        })
        .await
        .unwrap();
    let pool_id = store
        .create_admin_exit_pool(AdminExitPoolInput {
            name: format!("consistency-pool-{suffix}-{}", Uuid::new_v4().simple()),
            region_code: "TST".to_string(),
            strategy: "priority".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    store
        .replace_admin_exit_pool_members(
            pool_id,
            vec![AdminExitPoolMemberInput {
                exit_endpoint_id: endpoint_id,
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
            name: format!("consistency-line-{suffix}"),
            access_node_id,
            exit_pool_id: pool_id,
            listen_host: "198.51.100.91".to_string(),
            listen_port: 34_891,
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
    (access_node_id, resource_id, endpoint_id, pool_id, access_line_id)
}

async fn attach_user_to_exit_consistency_fixture(
    store: &PgStore,
    endpoint_id: Uuid,
    access_line_id: Uuid,
) {
    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    let pool_id =
        sqlx::query_scalar::<_, Uuid>("SELECT exit_pool_id FROM access_lines WHERE id = $1")
            .bind(access_line_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
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
    .bind(pool_id)
    .bind(endpoint_id)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn assert_exit_consistency_pruned(
    store: &PgStore,
    access_node_id: Uuid,
    endpoint_id: Uuid,
    access_line_id: Uuid,
) {
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
    .bind(endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let dirty_reason =
        sqlx::query_scalar::<_, String>("SELECT config_dirty_reason FROM access_nodes WHERE id = $1")
            .bind(access_node_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    assert!(!line_enabled);
    assert_eq!(access_assignment_count, 0);
    assert_eq!(exit_assignment_count, 0);
    assert!(dirty_reason.starts_with("admin_updated_exit_"));
}
