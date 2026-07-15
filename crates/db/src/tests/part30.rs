/// 数据库测试分片 30。
// 本文件承接 VLESS Reality 与线路绑定测试的尾部分片。
// 测试只使用示例域名、示例端口和仓库内演示数据。
// 这里验证同一出口线路可多次绑定中转节点并同时进入订阅。
// 分组只维护出口线路，订阅按套餐授权分组匹配绑定线路。
// 本文件不访问远端服务器，不写真实代理地址或私有凭据。
// 所有 helper 和导入由父级 tests/mod.rs 统一提供。
// 新增同类 PostgreSQL 用例时优先保持本分片不超过五百行。
// 注释使用中文，符合仓库源码头部约束。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_same_group_multiple_entry_bindings_are_all_in_subscription_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL multi entry subscription test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        sqlx::query(
            "UPDATE access_lines SET enabled = FALSE WHERE line_group_id = $1 OR exit_endpoint_id = $2",
        )
            .bind(group_id)
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();

        let result = store
            .create_admin_access_node_group_entries(
                node_id,
                vec![
                    AdminAccessNodeGroupEntryInput {
                        name: "multi-entry-a".to_string(),
                        exit_endpoint_id: endpoint_id,
                        listen_host: "access.example.test".to_string(),
                        listen_port: 42_300,
                        protocol: "vless".to_string(),
                        network_mode: "tcp".to_string(),
                        inbound_config: json!({}),
                        xhttp_mode: "auto".to_string(),
                        enabled: true,
                    },
                    AdminAccessNodeGroupEntryInput {
                        name: "multi-entry-b".to_string(),
                        exit_endpoint_id: endpoint_id,
                        listen_host: "access.example.test".to_string(),
                        listen_port: 42_301,
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

        let bound_rows = sqlx::query_as::<_, (Option<uuid::Uuid>, Option<uuid::Uuid>, uuid::Uuid)>(
            r#"
            SELECT line_group_id, exit_endpoint_id, exit_pool_id
            FROM access_lines
            WHERE id = ANY($1)
            ORDER BY listen_port
            "#,
        )
        .bind(&result.created_line_ids)
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(bound_rows.len(), 2);
        assert!(bound_rows
            .iter()
            .all(|(legacy_group_id, bound_endpoint_id, _exit_pool_id)| {
                legacy_group_id.is_none() && *bound_endpoint_id == Some(endpoint_id)
            }));
        assert_eq!(
            bound_rows
                .iter()
                .map(|(_legacy_group_id, _bound_endpoint_id, exit_pool_id)| *exit_pool_id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            1
        );

        let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
        let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let mut ports = profile["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|proxy| proxy["port"].as_i64())
            .collect::<Vec<_>>();
        ports.sort_unstable();
        assert_eq!(ports, vec![42_300, 42_301]);

        let subscription_json = store.user_subscription_json_for_user(user_id).await.unwrap();
        let mut json_ports = subscription_json["access_lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|line| line["listen_port"].as_i64())
            .collect::<Vec<_>>();
        json_ports.sort_unstable();
        assert_eq!(json_ports, vec![42_300, 42_301]);
    }
