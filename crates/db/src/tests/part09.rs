/// 数据库测试分片 09。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_heartbeat_returns_access_config_and_clears_dirty_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL heartbeat config test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let direct_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        sqlx::query("UPDATE exit_endpoints SET enabled = TRUE WHERE id = $1")
            .bind(direct_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE WHEN exit_endpoint_id = $2 THEN 'healthy' ELSE 'offline' END,
                allow_new_assignments = CASE WHEN exit_endpoint_id = $2 THEN TRUE ELSE FALSE END
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(exit_pool_id)
        .bind(direct_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM user_exit_assignments
            WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(exit_pool_id)
        .execute(store.pool())
        .await
        .unwrap();
        let first = store.heartbeat_json(Some(node_id), None).await.unwrap();
        assert_eq!(first["accepted"], true);
        assert_eq!(first["config_status"]["required"], true);
        assert_eq!(first["config"]["node_id"], node_id.to_string());
        assert_eq!(first["config"]["stats_enabled"], true);
        assert_eq!(first["config"]["access_lines"][0]["listen_port"], 443);
        let config_user_email = first["config"]["access_lines"][0]["users"][0]["email"]
            .as_str()
            .unwrap();
        let (_, config_user_key) =
            xrayc_xray_config::parse_stats_user_email(config_user_email).unwrap();
        let authorized_keys = first["authorized_users"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|user| user["xray_user_key"].as_str())
            .collect::<Vec<_>>();
        assert!(authorized_keys.contains(&config_user_key.as_str()));
        assert_eq!(
            first["config"]["exit_endpoints"][0]["protocol"]["type"],
            "socks"
        );
        assert_eq!(
            first["config"]["routing_rules"][0]["user_email"],
            config_user_email
        );
        let exit_tags = first["config"]["exit_endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|endpoint| endpoint["tag"].as_str())
            .collect::<Vec<_>>();
        let routed_exit_tag = first["config"]["routing_rules"][0]["outbound_tag"]
            .as_str()
            .unwrap();
        assert!(exit_tags.contains(&routed_exit_tag));

        let config_hash = first["config_hash"].as_str().unwrap();
        assert_eq!(first["desired_config_version"], config_hash);

        store
            .record_config_result(Some(node_id), None, true, None)
            .await
            .unwrap();
        let still_dirty =
            sqlx::query_scalar::<_, bool>("SELECT config_dirty FROM access_nodes WHERE id = $1")
                .bind(node_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(still_dirty);

        sqlx::query(
            r#"
            UPDATE access_nodes
            SET config_dirty = TRUE,
                config_dirty_at = last_heartbeat_at + interval '1 second',
                config_dirty_reason = 'stale_config_result_test'
            WHERE id = $1
            "#,
        )
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();
        store
            .record_config_result(Some(node_id), Some(config_hash), true, None)
            .await
            .unwrap();
        let stale_result_dirty = sqlx::query_scalar::<_, bool>(
            "SELECT config_dirty FROM access_nodes WHERE id = $1",
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(
            stale_result_dirty,
            "旧配置回执不能清掉 dirty 之后产生的新配置刷新"
        );
        sqlx::query(
            r#"
            UPDATE access_nodes
            SET config_dirty = TRUE,
                config_dirty_at = last_heartbeat_at,
                config_dirty_reason = 'fresh_config_result_test'
            WHERE id = $1
            "#,
        )
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

        store
            .record_config_result(Some(node_id), Some(config_hash), true, None)
            .await
            .unwrap();

        let second = store
            .heartbeat_json(Some(node_id), Some(config_hash))
            .await
            .unwrap();
        assert_eq!(second["config_status"]["required"], false);
        assert!(second["config"].is_null());

        let fresh_agent = store.heartbeat_json(Some(node_id), None).await.unwrap();
        assert_eq!(fresh_agent["config_status"]["required"], true);
        assert_eq!(fresh_agent["config"]["node_id"], node_id.to_string());

        let dirty =
            sqlx::query_scalar::<_, bool>("SELECT config_dirty FROM access_nodes WHERE id = $1")
                .bind(node_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(!dirty);

        let endpoint_id = Uuid::parse_str(
            first["config"]["exit_endpoints"][0]["id"]
                .as_str()
                .expect("endpoint id exists"),
        )
        .unwrap();
        store
            .record_admin_exit_endpoint_probe_request(endpoint_id)
            .await
            .unwrap();
        store
            .record_admin_exit_endpoint_probe_request(endpoint_id)
            .await
            .unwrap();
        let queued_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::BIGINT
            FROM access_exit_probes p
            WHERE p.access_node_id = $1
              AND p.exit_endpoint_id = $2
              AND p.status = 'queued'
              AND NOT EXISTS (
                  SELECT 1
                  FROM access_exit_probes r
                  WHERE r.access_node_id = p.access_node_id
                    AND r.exit_endpoint_id = p.exit_endpoint_id
                    AND r.status <> 'queued'
                    AND r.probed_at >= p.probed_at
              )
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(queued_count, 1);
        let with_probe_task = store
            .heartbeat_json(Some(node_id), Some(config_hash))
            .await
            .unwrap();
        assert_eq!(with_probe_task["config_status"]["required"], true);
        assert_eq!(
            with_probe_task["probe_tasks"][0]["exit_endpoint_id"],
            endpoint_id.to_string()
        );
        let leased_probe_task = store
            .heartbeat_json(Some(node_id), Some(config_hash))
            .await
            .unwrap();
        assert_eq!(
            leased_probe_task["probe_tasks"]
                .as_array()
                .expect("probe tasks array")
                .len(),
            0
        );
        sqlx::query(
            r#"
            UPDATE access_exit_probes
            SET task_lease_expires_at = now() - interval '1 second'
            WHERE access_node_id = $1
              AND exit_endpoint_id = $2
              AND status = 'queued'
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        let expired_probe_task = store
            .heartbeat_json(Some(node_id), Some(config_hash))
            .await
            .unwrap();
        assert_eq!(
            expired_probe_task["probe_tasks"][0]["exit_endpoint_id"],
            endpoint_id.to_string()
        );

        store
            .record_agent_probes(
                node_id,
                &json!({
                    "access_node_id": node_id,
                    "exit_probes": [{
                        "exit_endpoint_id": endpoint_id,
                        "status": "healthy",
                        "latency_ms": 1,
                        "probed_at_unix": Utc::now().timestamp() + 1
                    }]
                }),
            )
            .await
            .unwrap();
        let after_probe_report = store
            .heartbeat_json(Some(node_id), Some(config_hash))
            .await
            .unwrap();
        assert_eq!(
            after_probe_report["probe_tasks"]
                .as_array()
                .expect("probe tasks array")
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn test_pg_local_socks_line_creates_node_local_heartbeat_service_when_database_url_is_set()
    {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL new access node heartbeat test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let access_node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "heartbeat-new-node".to_string(),
                public_host: "127.0.0.1".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "xrayc-node-test-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let created = store
            .create_admin_local_exit_lines(
                access_node_id,
                AdminLocalExitLinesInput {
                    lines: vec![AdminLocalExitLineInput {
                        resource_name: "heartbeat-socks-resource".to_string(),
                        endpoint_name: "heartbeat-socks-endpoint".to_string(),
                        region_code: "E2E".to_string(),
                        outbound_type: "socks".to_string(),
                        network_mode: "tcp".to_string(),
                        host: "127.0.0.1".to_string(),
                        port: 34_569,
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
            .expect("local socks line should be created by node-scoped API");
        let exit_endpoint_id = uuid::Uuid::parse_str(
            created["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .expect("created exit endpoint id"),
        )
        .unwrap();

        let local_access_line_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM access_lines WHERE access_node_id = $1",
        )
        .bind(access_node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(local_access_line_count, 0);
        let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
        assert!(!yaml.contains("heartbeat-socks-resource"));
        assert!(!yaml.contains("local-user"));
        assert!(!yaml.contains("local-pass"));

        let heartbeat = store
            .heartbeat_json(Some(access_node_id), None)
            .await
            .unwrap();
        assert_eq!(heartbeat["config_status"]["required"], true);
        let local_services = heartbeat["config"]["local_exit_services"]
            .as_array()
            .expect("local exit services should be rendered");
        assert_eq!(local_services.len(), 1);
        assert_eq!(local_services[0]["protocol"], "socks");
        assert_eq!(local_services[0]["listen_port"], 34_569);

        let consumer_node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "heartbeat-consumer-node".to_string(),
                public_host: "heartbeat-consumer.example.test".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: "test-consumer-token".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        store
            .replace_admin_line_group_lines(line_group_id, vec![exit_endpoint_id])
            .await
            .unwrap();
        let result = store
            .create_admin_access_node_group_entries(
                consumer_node_id,
                vec![AdminAccessNodeGroupEntryInput {
                    name: "heartbeat-socks-line".to_string(),
                    exit_endpoint_id,
                    listen_host: "0.0.0.0".to_string(),
                    listen_port: 34_568,
                    protocol: "vless".to_string(),
                    network_mode: "tcp".to_string(),
                    inbound_config: json!({}),
                    xhttp_mode: "auto".to_string(),
                    enabled: true,
                }],
            )
            .await
            .expect("explicit line binding should create access line");
        assert_eq!(result.created_line_ids.len(), 1);
    }

    #[tokio::test]
    async fn test_pg_heartbeat_returns_credentialed_socks_endpoint_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL socks config test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let resource_id = uuid("00000000-0000-0000-0000-000000009201");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000009202");

        sqlx::query(
            r#"
            INSERT INTO exit_resources (id, name, region_code, enabled)
            VALUES ($1, 'third-party-socks', 'US', TRUE)
            ON CONFLICT (id) DO UPDATE SET enabled = TRUE
            "#,
        )
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, outbound_type, host, port,
                outbound_config, enabled
            )
            VALUES (
                $1, $2, 'socks'::endpoint_type, 'socks.example.test', 1080,
                '{"username":"agent","password":"secret"}'::jsonb, TRUE
            )
            ON CONFLICT (id) DO UPDATE SET
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(endpoint_id)
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status,
                allow_new_assignments
            )
            VALUES ($1, $2, 100, 'healthy', TRUE)
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                status = EXCLUDED.status,
                allow_new_assignments = EXCLUDED.allow_new_assignments
            "#,
        )
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE WHEN exit_endpoint_id = $2 THEN 'healthy' ELSE 'offline' END,
                allow_new_assignments = CASE WHEN exit_endpoint_id = $2 THEN TRUE ELSE FALSE END
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM user_exit_assignments
            WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE access_lines SET exit_endpoint_id = $2 WHERE id = $1")
            .bind(line_id)
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(line_group_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml.contains("access.example.test"));
        assert!(!yaml.contains("socks.example.test"));
        assert!(!yaml.contains("agent"));
        assert!(!yaml.contains("secret"));
        let assigned = assigned_exit_assignment(&store, user_id, line_id, pool_id).await;
        assert_eq!(assigned.exit_endpoint_id, endpoint_id);

        let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        let exit_tag = exit_endpoint_tag(endpoint_id);
        let socks_endpoint = heartbeat["config"]["exit_endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|endpoint| endpoint["tag"] == exit_tag)
            .expect("socks endpoint should be present");
        assert_eq!(socks_endpoint["protocol"]["type"], "socks");
        assert_eq!(socks_endpoint["protocol"]["address"], "socks.example.test");
        assert_eq!(socks_endpoint["protocol"]["port"], 1080);
        assert_eq!(socks_endpoint["protocol"]["username"], "agent");
        assert_eq!(socks_endpoint["protocol"]["password"], "secret");
        let has_socks_route = heartbeat["config"]["routing_rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["outbound_tag"] == exit_tag);
        assert!(has_socks_route);
    }
