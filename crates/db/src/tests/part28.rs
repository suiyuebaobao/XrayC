/// 数据库测试分片 28。
// 本文件覆盖中转节点按入口行绑定具体线路。
// 新模型要求每个入口协议、网络模式、端口都是一条独立入口。
// 保存时只绑定出口线路和入口参数，不把中转入口绑定到分组。
// 分组只维护线路池成员，并供套餐授权和倍率计算使用。
// 该测试只使用演示数据和保留地址，不写入真实服务器资料。
// PostgreSQL 连接通过 DATABASE_URL 控制，未配置时跳过。
// 断言关注事务结果、单线路运行集合和节点 dirty 状态。
// 后续新增入口模式时可以在这里补充更多组合。
// 文件保持短小，避免超过源码长度门禁。

#[tokio::test]
async fn test_pg_access_node_line_entries_bind_exit_endpoint_not_group() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL line entry test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let data = store.load_store_data().await.unwrap();
    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let exit_endpoint_id = data
        .exit_pools
        .values()
        .find_map(|pool| pool.members.first().map(|member| member.id))
        .unwrap();
    let port_base = 20_000 + (Uuid::new_v4().as_u128() % 30_000) as u16;

    let result = store
        .create_admin_access_node_group_entries(
            node_id,
            vec![
                AdminAccessNodeGroupEntryInput {
                    name: "入口行 VLESS TCP".to_string(),
                    exit_endpoint_id,
                    listen_host: "entry.example.test".to_string(),
                    listen_port: port_base,
                    protocol: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    inbound_config: json!({}),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                },
                AdminAccessNodeGroupEntryInput {
                    name: "入口行 VLESS XHTTP".to_string(),
                    exit_endpoint_id,
                    listen_host: "entry.example.test".to_string(),
                    listen_port: port_base + 1,
                    protocol: "vless".to_string(),
                    network_mode: "xhttp".to_string(),
                    inbound_config: json!({}),
                    xhttp_mode: "stream-one".to_string(),
                    enabled: true,
                },
            ],
        )
        .await
        .unwrap();

    assert_eq!(result.created_line_ids.len(), 2);
    assert_eq!(result.exit_endpoint_ids, vec![exit_endpoint_id]);
    let bound_endpoint_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(DISTINCT l.exit_endpoint_id)
        FROM access_lines l
        WHERE l.id = ANY($1)
          AND l.exit_endpoint_id = $2
        "#,
    )
    .bind(&result.created_line_ids)
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(bound_endpoint_count, 1);
    let unrelated_runtime_member_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM access_lines l
        JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
        WHERE l.id = ANY($1)
          AND m.exit_endpoint_id <> $2
        "#,
    )
    .bind(&result.created_line_ids)
    .bind(exit_endpoint_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(unrelated_runtime_member_count, 0);
    let dirty = sqlx::query_scalar::<_, bool>(
        "SELECT config_dirty FROM access_nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(dirty);
}
