/// 数据库测试分片 14。
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
    async fn test_pg_worker_marks_expired_probe_task_timeout_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL probe timeout test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let access_line_id = uuid("00000000-0000-0000-0000-000000000501");
        let access_node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");

        store
            .update_access_operations_settings_json(json!({
                "probe_policy": {
                    "exit_failure_threshold": 1,
                    "exit_recovery_threshold": 1,
                    "exit_probe_interval_seconds": 300,
                    "probe_queue_batch_size": 10,
                    "max_pending_probe_tasks": 5,
                    "max_probe_task_delivery_count": 1
                }
            }))
            .await
            .unwrap();
        store
            .update_subscription_settings_json(json!({
                "block_unhealthy_lines": true
            }))
            .await
            .unwrap();

        sqlx::query("DELETE FROM access_exit_probes WHERE access_node_id = $1")
            .bind(access_node_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM access_exit_probe_states WHERE access_node_id = $1")
            .bind(access_node_id)
            .execute(store.pool())
            .await
            .unwrap();
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
            INSERT INTO user_exit_assignments (
                user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
            )
            VALUES ($1, $2, $3, $4, 'worker-timeout-test')
            ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                failover_reason = EXCLUDED.failover_reason
            "#,
        )
        .bind(user_id)
        .bind(access_line_id)
        .bind(exit_pool_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE access_nodes SET config_dirty = FALSE, config_dirty_reason = '' WHERE id = $1",
        )
        .bind(access_node_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO access_exit_probes (
                access_node_id, exit_endpoint_id, status, error_summary, probed_at,
                task_claimed_at, task_lease_expires_at, task_delivery_count
            )
            VALUES (
                $1, $2, 'queued', 'worker timeout test', now() - interval '10 minutes',
                now() - interval '2 minutes', now() - interval '1 minute', 1
            )
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.run_worker_maintenance(14).await.unwrap();
        assert!(result.timed_out_exit_probe_tasks >= 1);

        let latest_status = sqlx::query_scalar::<_, String>(
            r#"
            SELECT status
            FROM access_exit_probes
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            ORDER BY probed_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(latest_status, "timeout");

        let effective_status = sqlx::query_scalar::<_, String>(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(access_node_id)
        .bind(exit_endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(effective_status, "offline");

        let assignment_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::BIGINT
            FROM user_exit_assignments
            WHERE user_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(user_id)
        .bind(exit_endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(assignment_count, 0);
    }

    #[tokio::test]
    async fn test_pg_record_audit_log_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL audit log test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let actor_user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM users WHERE is_admin = TRUE ORDER BY created_at ASC LIMIT 1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        let resource_id = Uuid::new_v4();
        let client_ip_hash =
            "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let audit_id = store
            .record_audit_log(AuditLogInput {
                actor_user_id,
                actor_email: "admin@example.test".to_string(),
                action: "plan.create".to_string(),
                resource_type: "plan".to_string(),
                resource_id: Some(resource_id),
                client_ip: client_ip_hash.to_string(),
                request_summary: json!({
                    "name": "audit-test",
                    "password": "must-not-be-stored",
                    "ssh_password_redacted": true,
                    "proxy_url": "socks5://user:must-not-be-stored@example.test:1080",
                    "safe": {
                        "port": 1080
                    }
                }),
                result: "success".to_string(),
            })
            .await
            .unwrap();

        let row = sqlx::query_as::<_, AuditLogTestRow>(
            r#"
            SELECT action, resource_type, resource_id, client_ip, request_summary
            FROM audit_logs
            WHERE id = $1
            "#,
        )
        .bind(audit_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(row.action, "plan.create");
        assert_eq!(row.resource_type, "plan");
        assert_eq!(row.resource_id, Some(resource_id));
        assert_eq!(row.client_ip, client_ip_hash);
        assert_eq!(row.request_summary["name"], "audit-test");
        assert_eq!(row.request_summary["safe"]["port"], 1080);
        assert_eq!(row.request_summary["ssh_password_redacted"], true);
        assert_eq!(row.request_summary["sensitive_fields_redacted"], true);
        let stored_summary = row.request_summary.to_string();
        assert!(!stored_summary.contains("must-not-be-stored"));
        assert!(!stored_summary.contains("socks5://"));
        assert!(!stored_summary.contains("proxy_url"));
        assert!(!stored_summary.contains("\"password\""));
    }

    #[tokio::test]
    async fn test_pg_admin_orders_filter_pagination_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL admin orders filter test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email = 'demo@example.test'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let order = store.create_order_json(user_id, plan_id).await.unwrap();
        let order_no = order["order_no"].as_str().unwrap().to_string();

        let first_page = store
            .admin_orders_filtered_json(AdminOrderFilters {
                page: Some(1),
                page_size: Some(500),
                status: Some("pending".to_string()),
                email: Some("demo@example.test".to_string()),
                order_no: Some(order_no.clone()),
                ..AdminOrderFilters::default()
            })
            .await
            .unwrap();
        assert_eq!(first_page["total"], 1);
        assert_eq!(first_page["page"], 1);
        assert_eq!(first_page["page_size"], ADMIN_ORDER_MAX_PAGE_SIZE);
        assert_eq!(first_page["items"][0]["order_no"], order_no);
        assert_eq!(first_page["items"][0]["user_email"], "demo@example.test");

        let second_page = store
            .admin_orders_filtered_json(AdminOrderFilters {
                page: Some(2),
                page_size: Some(1),
                keyword: Some(order_no),
                ..AdminOrderFilters::default()
            })
            .await
            .unwrap();
        assert_eq!(second_page["total"], 1);
        assert_eq!(second_page["page"], 2);
        assert!(second_page["items"]
            .as_array()
            .is_some_and(|items| items.is_empty()));
    }

    fn test_exit_endpoint(
        id: &str,
        resource_name: &str,
        outbound_type: EndpointType,
        host: &str,
        port: u16,
        outbound_config: Value,
    ) -> ExitEndpoint {
        ExitEndpoint {
            id: uuid(id),
            resource_name: resource_name.to_string(),
            ownership: "third_party".to_string(),
            owner_access_node_id: None,
            outbound_type,
            host: host.to_string(),
            port,
            outbound_config,
            stream_config: json!({}),
            weight: 100,
            priority: 100,
            allow_new_assignments: true,
            healthy: true,
            status: "healthy".to_string(),
        }
    }

    fn heartbeat_and_render_for_endpoint(endpoint: ExitEndpoint) -> (Value, Value, Uuid) {
        let store = MemoryStore::seeded();
        let (data, node_id, endpoint_id) = store.write(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let line_id = *data.access_lines.keys().next().unwrap();
            let pool_id = data.access_lines.get(&line_id).unwrap().exit_pool_id;
            let user_id = *data.users.keys().next().unwrap();
            let endpoint_id = endpoint.id;
            data.exit_pools.get_mut(&pool_id).unwrap().members = vec![endpoint];
            data.user_exit_assignments = vec![UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: endpoint_id,
                assigned_at: Utc::now(),
                failover_reason: "initial".to_string(),
            }];
            (data.clone(), node_id, endpoint_id)
        });

        let heartbeat = heartbeat_json(&data, Some(node_id), None);
        let config: XrayAccessConfig =
            serde_json::from_value(heartbeat["config"].clone()).expect("config should deserialize");
        let rendered =
            xrayc_xray_config::compile_xray_config(&config).expect("config should compile");
        (heartbeat, rendered, endpoint_id)
    }

    struct PgOutboundCase {
        name: &'static str,
        resource_id: Uuid,
        endpoint_id: Uuid,
        outbound_type: &'static str,
        host: &'static str,
        port: u16,
        config: Value,
        heartbeat_type: &'static str,
        xray_protocol: &'static str,
        expected_protocol_fields: Vec<(&'static str, &'static str)>,
        leaked_markers: Vec<&'static str>,
    }
