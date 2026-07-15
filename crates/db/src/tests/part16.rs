/// 数据库测试分片 16。
// 本文件覆盖中转节点删除闭环的数据库行为。
// 删除节点时从属入口应被级联删除。
// 历史 usage_ledgers 应保留并断开已删除入口外键。
// 本机出口资源应随所属节点级联清理。
// 统计字段应反映节点、入口、账本和本机资源处理数量。
// 测试仅使用演示数据和随机名称，不保存外部主机资料。
// PostgreSQL 未配置时保持跳过语义。
// 文件头部使用中文注释满足仓库拆分约束。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_delete_access_node_preserves_usage_ledgers_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL access node delete ledger test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let user_key = format!("u-{}@xrayc.local", user_id.simple());

        let usage_line_delete_action = sqlx::query_scalar::<_, String>(
            r#"
            SELECT c.confdeltype::text
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            WHERE t.relname = 'usage_ledgers'
              AND c.conname = 'usage_ledgers_access_line_id_fkey'
            "#,
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(usage_line_delete_action, "n");

        let resource_node_delete_action = sqlx::query_scalar::<_, String>(
            r#"
            SELECT c.confdeltype::text
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            WHERE t.relname = 'exit_resources'
              AND c.conname = 'exit_resources_access_node_id_fkey'
            "#,
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(resource_node_delete_action, "c");

        sqlx::query(
            r#"
            INSERT INTO usage_ledgers (
                access_line_id, user_id, xray_user_key, traffic_source,
                delta_uplink, delta_downlink, billing_multiplier,
                billed_bytes, delta_total, billed_uplink, billed_downlink,
                collected_at, recorded_at
            )
            VALUES ($1, $2, $3, 'access_line', 1024, 2048, 1.000, 3072, 3072, 1024, 2048, now(), now())
            "#,
        )
        .bind(line_id)
        .bind(user_id)
        .bind(&user_key)
        .execute(store.pool())
        .await
        .unwrap();

        store
            .create_admin_local_exit_lines(
                node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "本机出口删除闭环".to_string(),
                        endpoint_name: "本机出口删除闭环".to_string(),
                        region_code: "LOCAL".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "127.0.0.1".to_string(),
                        port: 34_570,
                        outbound_config: json!({
                            "username": "delete-local-user",
                            "password": "local-delete-pass",
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

        let result = store
            .delete_admin_access_nodes(vec![node_id, node_id])
            .await
            .unwrap();
        assert_eq!(result.deleted_node_count, 1);
        assert_eq!(result.deleted_node_ids, vec![node_id]);
        assert_eq!(result.deleted_access_line_count, 1);
        assert_eq!(result.retained_usage_ledger_count, 1);
        assert_eq!(result.deleted_local_resource_count, 1);
        assert_eq!(result.deleted_local_pool_count, 0);

        let access_line_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM access_lines WHERE id = $1",
        )
        .bind(line_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(access_line_count, 0);

        let retained_ledger_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM usage_ledgers
            WHERE user_id = $1
              AND access_line_id IS NULL
              AND billed_bytes = 3072
            "#,
        )
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(retained_ledger_count, 1);
    }

    #[tokio::test]
    async fn test_pg_delete_access_node_detaches_ledgers_for_owned_exit_endpoint_lines_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!(
                "DATABASE_URL not set; skipping PostgreSQL owned endpoint line delete ledger test"
            );
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let owner_node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: format!("owned-endpoint-owner-{}", Uuid::new_v4().simple()),
                public_host: "198.51.100.81".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: format!("owned-endpoint-owner-token-{}", Uuid::new_v4().simple()),
                ..Default::default()
            })
            .await
            .unwrap();
        let entry_node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: format!("owned-endpoint-entry-{}", Uuid::new_v4().simple()),
                public_host: "198.51.100.82".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: format!("owned-endpoint-entry-token-{}", Uuid::new_v4().simple()),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                owner_node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "owned-endpoint-resource".to_string(),
                        endpoint_name: "owned-endpoint-exit".to_string(),
                        region_code: "US".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "198.51.100.83".to_string(),
                        port: 34_581,
                        outbound_config: json!({
                            "username": "owned-endpoint-user",
                            "password": "owned-endpoint-pass",
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
        let exit_endpoint_id = Uuid::parse_str(
            created["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let entry_id = store
            .create_admin_access_entry(AdminAccessEntryInput {
                access_node_id: entry_node_id,
                name: "owned-endpoint-entry".to_string(),
                listen_host: "198.51.100.82".to_string(),
                listen_port: 34_582,
                protocol: "vless".to_string(),
                transport: "tcp".to_string(),
                security: "reality".to_string(),
                server_name: "owned-endpoint.example.test".to_string(),
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
                    name: "owned-endpoint-binding".to_string(),
                    enabled: true,
                    sort_weight: 100,
                    remark: String::new(),
                },
            )
            .await
            .unwrap();
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let ledger_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO usage_ledgers (
                access_line_id, user_id, xray_user_key, traffic_source,
                delta_uplink, delta_downlink, billing_multiplier,
                billed_bytes, delta_total, billed_uplink, billed_downlink,
                collected_at, recorded_at
            )
            VALUES (
                $1, $2, 'owned-endpoint-ledger', 'access_line',
                11, 22, 1.000, 33, 33, 11, 22, now(), now()
            )
            RETURNING id
            "#,
        )
        .bind(binding_id)
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

        let result = store
            .delete_admin_access_nodes(vec![owner_node_id])
            .await
            .unwrap();

        assert_eq!(result.deleted_node_count, 1);
        assert_eq!(result.deleted_access_line_count, 1);
        assert_eq!(result.retained_usage_ledger_count, 1);

        let runtime_line_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM access_lines WHERE id = $1")
                .bind(binding_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(runtime_line_count, 0);

        let ledger_line_id = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT access_line_id FROM usage_ledgers WHERE id = $1",
        )
        .bind(ledger_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(ledger_line_id, None);
    }
