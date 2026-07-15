/// 数据库测试分片 35。
// 本文件覆盖线路池线路删除后的中转入口同步清理。
// 新入口绑定的是具体 exit_endpoint，不再允许删除后留下空出口入口。
// 删除线路池 endpoint 时，关联的 access_lines 必须一起删除。
// 用户入口授权和用户出口分配依赖 access_lines 与 endpoint 外键级联清理。
// 测试通过公开 store API 创建中转节点、本机出口线路和入口绑定。
// 这里不访问真实服务器，不保存真实地址、账号或密码。
// 只在 DATABASE_URL 存在时运行，避免无 PostgreSQL 环境报错。
// 后续删除语义调整时优先维护本分片。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_delete_exit_endpoint_deletes_bound_access_entries_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL endpoint-bound entry delete test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("delete-bound-entry-node-{}", Uuid::new_v4().simple()),
            public_host: "delete-bound-entry.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("delete-bound-entry-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            access_node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "delete-bound-entry-resource".to_string(),
                    endpoint_name: "delete-bound-entry-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.61".to_string(),
                    port: 39_061,
                    outbound_config: json!({
                        "username": "delete-bound-entry-user",
                        "password": "delete-bound-entry-pass",
                    }),
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
    let line_group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("删除绑定线路组-{}", Uuid::new_v4().simple()),
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
        .replace_admin_line_group_lines(line_group_id, vec![exit_endpoint_id])
        .await
        .unwrap();

    let entry_result = store
        .create_admin_access_node_group_entries(
            access_node_id,
            vec![AdminAccessNodeGroupEntryInput {
                name: "真实恢复入口-delete-test".to_string(),
                exit_endpoint_id,
                listen_host: "delete-bound-entry.example.test".to_string(),
                listen_port: 41_361,
                protocol: "shadowsocks".to_string(),
                network_mode: "tcp".to_string(),
                inbound_config: json!({}),
                xhttp_mode: "auto".to_string(),
                enabled: true,
            }],
        )
        .await
        .unwrap();
    let access_line_id = entry_result.created_line_ids[0];
    let exit_pool_id =
        sqlx::query_scalar::<_, Uuid>("SELECT exit_pool_id FROM access_lines WHERE id = $1")
            .bind(access_line_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let endpoint_fk_delete_action = sqlx::query_scalar::<_, String>(
        r#"
        SELECT confdeltype::text
        FROM pg_constraint
        WHERE conname = 'access_lines_exit_endpoint_id_fkey'
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(endpoint_fk_delete_action, "c");
    let user_id = uuid("00000000-0000-0000-0000-000000000001");
    sqlx::query(
        r#"
        INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(line_group_id)
    .bind(access_line_id)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO user_exit_assignments (user_id, access_line_id, exit_pool_id, exit_endpoint_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT DO NOTHING
        "#,
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

    let access_line_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM access_lines WHERE id = $1")
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
    let exit_assignment_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_exit_assignments WHERE access_line_id = $1")
            .bind(access_line_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let group_member_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM line_group_exit_endpoints WHERE exit_endpoint_id = $1",
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

    assert_eq!(access_line_count, 0);
    assert_eq!(access_assignment_count, 0);
    assert_eq!(exit_assignment_count, 0);
    assert_eq!(group_member_count, 0);
    assert_eq!(dirty_reason, "admin_deleted_exit_endpoint");
}
