/// 数据库测试分片 20。
// 本文件覆盖已有中转节点重部署时的 Agent Token 轮换。
// 重部署表单会提交新的 agent_token，数据库必须同步更新哈希。
// 否则远端 access-agent 会启动成功但无法通过中心鉴权。
// 鉴权失败后 agent 无法持久化 desired config version，部署会超时回滚。
// 测试只在 DATABASE_URL 存在时运行，不连接真实测试服务器。
// 这里使用示例 token，不保存任何真实服务器地址、账号或密码。
// 修改本文件时继续保持中文注释和文件长度限制。
// SQL 与断言必须围绕真实 PostgreSQL 行为。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_access_node_redeploy_token_rotation_updates_agent_hash_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL access node token rotation test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    sqlx::query("DELETE FROM access_nodes WHERE name = 'redeploy-token-rotation-node'")
        .execute(store.pool())
        .await
        .unwrap();

    let access_node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "redeploy-token-rotation-node".to_string(),
            public_host: "192.0.2.44".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-redeploy-token-old".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE access_nodes
        SET config_dirty = FALSE,
            config_dirty_at = NULL,
            config_dirty_reason = ''
        WHERE id = $1
        "#,
    )
    .bind(access_node_id)
    .execute(store.pool())
    .await
    .unwrap();

    assert!(store
        .verify_agent_token(Some(access_node_id), "test-redeploy-token-old")
        .await
        .unwrap());

    store
        .rotate_admin_access_node_agent_token(access_node_id, "test-redeploy-token-new")
        .await
        .unwrap();

    assert!(store
        .verify_agent_token(Some(access_node_id), "test-redeploy-token-new")
        .await
        .unwrap());
    assert!(!store
        .verify_agent_token(Some(access_node_id), "test-redeploy-token-old")
        .await
        .unwrap());

    let (config_dirty, dirty_reason) = sqlx::query_as::<_, (bool, String)>(
        "SELECT config_dirty, config_dirty_reason FROM access_nodes WHERE id = $1",
    )
    .bind(access_node_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(config_dirty);
    assert_eq!(dirty_reason, "admin_redeploy_rotated_agent_token");

    sqlx::query("DELETE FROM access_nodes WHERE id = $1")
        .bind(access_node_id)
        .execute(store.pool())
        .await
        .unwrap();
}
