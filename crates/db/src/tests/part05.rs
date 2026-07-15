// 数据库测试分片 05。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    async fn assert_runtime_metrics_and_ledger(ctx: &RuntimeReportTestContext) {
        let store = &ctx.store;
        let node_id = ctx.node_id;
        let line_id = ctx.line_id;
        let user_id = ctx.user_id;
        let exit_pool_id = ctx.exit_pool_id;
        let endpoint_id = ctx.endpoint_id;
        let user_key = ctx.user_key;
        let now_ts = Utc::now().timestamp();
        let metric_payload = json!({
            "access_node_id": node_id,
            "metrics": [{
                "access_line_id": line_id,
                "online_users": 2,
                "active_connections": 3,
                "unique_client_ips": 2,
                "uplink_rate_bps": 1024,
                "downlink_rate_bps": 2048,
                "collected_at_unix": now_ts
            }]
        });
        assert_eq!(
            store
                .record_agent_metrics(node_id, &metric_payload)
                .await
                .unwrap(),
            1
        );

        let session_payload = json!({
            "access_node_id": node_id,
                "sessions": [{
                    "access_line_id": line_id,
                    "xray_user_key": user_key,
                    "client_ip_hash": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    "active_connection_count": 3,
                    "started_at_unix": now_ts - 1,
                    "last_seen_at_unix": now_ts
                }]
        });
        assert_eq!(
            store
                .record_agent_sessions(node_id, &session_payload)
                .await
                .unwrap(),
            1
        );
        let empty_session_payload = json!({
            "access_node_id": node_id,
            "sessions": []
        });
        assert_eq!(
            store
                .record_agent_sessions(node_id, &empty_session_payload)
                .await
                .unwrap(),
            0
        );
        let preserved_session_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM access_user_sessions WHERE access_node_id = $1",
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(preserved_session_count, 1);
        let first_report = TrafficReport {
            access_line_id: line_id,
            xray_user_key: user_key.to_string(),
            uplink_total: 100,
            downlink_total: 200,
            collected_at: Utc::now(),
        };
        let mut second_report = first_report.clone();
        second_report.uplink_total = 400;
        second_report.downlink_total = 800;
        second_report.collected_at += Duration::seconds(60);
        assert!(
            store
                .apply_report_for_node(Some(node_id), first_report)
                .await
                .unwrap()
                .baseline_only
        );
        let billed_result = store
            .apply_report_for_node(Some(node_id), second_report)
            .await
            .unwrap();
        assert_eq!(billed_result.billed_bytes, 900);
        let traffic_touch_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM access_nodes
            WHERE id = $1
              AND last_traffic_report_at IS NOT NULL
              AND last_traffic_success_at IS NOT NULL
            "#,
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(traffic_touch_count, 1);
        let ledger_endpoint_id = sqlx::query_scalar::<_, Option<Uuid>>(
            r#"
            SELECT exit_endpoint_id
            FROM usage_ledgers
            WHERE access_line_id = $1 AND user_id = $2
            "#,
        )
        .bind(line_id)
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(ledger_endpoint_id, Some(endpoint_id));
        let line_metrics = store.access_line_metrics_json(line_id).await.unwrap();
        assert_eq!(
            line_metrics["metrics"][0]["access_node_id"],
            node_id.to_string()
        );
        assert_eq!(
            line_metrics["metrics"][0]["exit_pool_id"],
            exit_pool_id.to_string()
        );
        assert_eq!(line_metrics["ledger_summary"]["delta_total"], 900);
        assert_eq!(line_metrics["ledger_summary"]["real_bytes"], 900);
        assert_eq!(line_metrics["ledger_summary"]["billed_bytes"], 900);
        assert_eq!(line_metrics["ledger_windows"][0]["delta_total"], 900);
        let line_sessions = store.access_line_sessions_json(line_id).await.unwrap();
        assert_eq!(line_sessions["sessions"][0]["active_connection_count"], 3);
        assert_eq!(line_sessions["sessions"][0]["status"], "online");

    }

    async fn assert_runtime_probe_failover_and_recovery(ctx: &RuntimeReportTestContext) {
        let store = &ctx.store;
        let node_id = ctx.node_id;
        let line_id = ctx.line_id;
        let user_id = ctx.user_id;
        let exit_pool_id = ctx.exit_pool_id;
        let endpoint_id = ctx.endpoint_id;
        let failover_endpoint_id = ctx.failover_endpoint_id;
        let probe_payload = json!({
            "access_node_id": node_id,
            "line_probes": [{
                "access_line_id": line_id,
                "status": "healthy",
                "latency_ms": 123,
                "probed_at_unix": 1_700_000_003
            }],
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "healthy",
                "latency_ms": 231,
                "probed_at_unix": 1_700_000_003
            }]
        });
        let probe_result = store
            .record_agent_probes(node_id, &probe_payload)
            .await
            .unwrap();
        assert_eq!(probe_result["line_probes"], 1);
        assert_eq!(probe_result["exit_probes"], 1);
        let (endpoint_last_status, resource_status, resource_last_status): (
            String,
            String,
            String,
        ) = sqlx::query_as(
            r#"
            SELECT e.last_probe_status, r.status, r.last_probe_status
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE e.id = $1
            "#,
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(endpoint_last_status, "healthy");
        assert_eq!(resource_status, "healthy");
        assert_eq!(resource_last_status, "healthy");
        let offline_probe_payload = json!({
            "access_node_id": node_id,
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "timeout",
                "latency_ms": null,
                "probed_at_unix": 1_700_000_004
            }]
        });
        let offline_result = store
            .record_agent_probes(node_id, &offline_probe_payload)
            .await
            .unwrap();
        assert_eq!(offline_result["exit_probes"], 1);
        let first_failure_state: String = sqlx::query_scalar(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(first_failure_state, "healthy");
        let second_offline_probe_payload = json!({
            "access_node_id": node_id,
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "timeout",
                "latency_ms": null,
                "probed_at_unix": 1_700_000_005
            }]
        });
        store
            .record_agent_probes(node_id, &second_offline_probe_payload)
            .await
            .unwrap();
        let second_failure_state: String = sqlx::query_scalar(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(second_failure_state, "healthy");
        let third_offline_probe_payload = json!({
            "access_node_id": node_id,
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "timeout",
                "error_summary": "runtime timeout threshold reached",
                "latency_ms": null,
                "probed_at_unix": 1_700_000_006
            }]
        });
        store
            .record_agent_probes(node_id, &third_offline_probe_payload)
            .await
            .unwrap();
        let offline_state: String = sqlx::query_scalar(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(offline_state, "offline");
        let (offline_resource_status, offline_resource_last_status, offline_endpoint_error): (
            String,
            String,
            String,
        ) = sqlx::query_as(
            r#"
            SELECT r.status, r.last_probe_status, r.last_endpoint_error
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE e.id = $1
            "#,
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(offline_resource_status, "offline");
        assert_eq!(offline_resource_last_status, "timeout");
        assert_eq!(offline_endpoint_error, "runtime timeout threshold reached");
        let member_status: String = sqlx::query_scalar(
            r#"
            SELECT status
            FROM exit_pool_members
            WHERE exit_endpoint_id = $1
            LIMIT 1
            "#,
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(member_status, "healthy");
        let failover_assignment =
            assigned_exit_assignment(store, user_id, line_id, exit_pool_id).await;
        assert_eq!(failover_assignment.exit_endpoint_id, failover_endpoint_id);
        assert_eq!(failover_assignment.failover_reason, "failover:unavailable");
        let first_recovery_payload = json!({
            "access_node_id": node_id,
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "healthy",
                "latency_ms": 120,
                "probed_at_unix": 1_700_000_007
            }]
        });
        store
            .record_agent_probes(node_id, &first_recovery_payload)
            .await
            .unwrap();
        let first_recovery_state: String = sqlx::query_scalar(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(first_recovery_state, "offline");
        let second_recovery_payload = json!({
            "access_node_id": node_id,
            "exit_probes": [{
                "exit_endpoint_id": endpoint_id,
                "status": "healthy",
                "latency_ms": 110,
                "probed_at_unix": 1_700_000_008
            }]
        });
        store
            .record_agent_probes(node_id, &second_recovery_payload)
            .await
            .unwrap();
        let recovered_state: String = sqlx::query_scalar(
            r#"
            SELECT effective_status
            FROM access_exit_probe_states
            WHERE access_node_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(node_id)
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(recovered_state, "healthy");
        let (recovered_resource_status, recovered_resource_last_status, recovered_endpoint_error): (
            String,
            String,
            String,
        ) = sqlx::query_as(
            r#"
            SELECT r.status, r.last_probe_status, r.last_endpoint_error
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE e.id = $1
            "#,
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(recovered_resource_status, "healthy");
        assert_eq!(recovered_resource_last_status, "healthy");
        assert_eq!(recovered_endpoint_error, "");

    }

    async fn assert_runtime_read_models_and_cleanup(ctx: &RuntimeReportTestContext) {
        let store = &ctx.store;
        let node_id = ctx.node_id;
        let line_id = ctx.line_id;
        let endpoint_id = ctx.endpoint_id;
        let failover_endpoint_id = ctx.failover_endpoint_id;
        let session_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM access_user_sessions WHERE access_node_id = $1",
        )
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(session_count, 1);

        let routing = store.access_routing_json().await.unwrap();
        let line = routing["access_lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|line| line["uuid"] == line_id.to_string())
            .unwrap();
        assert_eq!(line["online_users"], 2);
        assert_eq!(line["active_connections"], 3);
        assert_eq!(line["unique_client_ips"], 2);
        assert_eq!(line["uplink_rate_bps"], 1024);
        assert_eq!(line["downlink_rate_bps"], 2048);
        assert_eq!(line["latency_ms"], 123);
        assert_eq!(line["probe_status"], "healthy");

        let summary = store.operations_summary_json().await.unwrap();
        assert_eq!(summary["online_users"], 2);
        assert_eq!(summary["active_connections"], 3);
        assert_eq!(summary["unique_client_ips"], 2);
        assert!(summary["latest_metric_at"].is_string());
        assert_eq!(summary["runtime_metric_status"], "fresh");
        assert_eq!(summary["runtime_metric_line_count"], 1);
        assert_eq!(summary["traffic_health"]["windows"]["month"]["real_bytes"], 900);
        assert_eq!(summary["traffic_health"]["windows"]["total"]["real_bytes"], 900);
        let line_traffic = summary["traffic_health"]["line_items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["access_line_id"] == line_id.to_string())
            .unwrap();
        assert_traffic_health_entity(line_traffic);
        let node_traffic = summary["traffic_health"]["node_items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["access_node_id"] == node_id.to_string())
            .unwrap();
        assert_traffic_health_entity(node_traffic);
        let exit_traffic = summary["traffic_health"]["exit_items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["exit_endpoint_id"] == endpoint_id.to_string())
            .unwrap();
        assert_traffic_health_entity(exit_traffic);
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let group_traffic = summary["traffic_health"]["group_items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["line_group_id"] == group_id.to_string())
            .unwrap();
        assert_traffic_health_entity(group_traffic);
        let removed_legacy_group_id = uuid("00000000-0000-0000-0000-000000000600");
        assert!(summary["traffic_health"]["group_items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["line_group_id"] != removed_legacy_group_id.to_string()));
        store
            .clear_access_line_runtime(node_id, line_id)
            .await
            .unwrap();
        let no_metric_summary = store.operations_summary_json().await.unwrap();
        assert!(no_metric_summary["online_users"].is_null());
        assert!(no_metric_summary["active_connections"].is_null());
        assert!(no_metric_summary["unique_client_ips"].is_null());
        assert!(no_metric_summary["latest_metric_at"].is_null());
        assert_eq!(no_metric_summary["runtime_metric_status"], "no_data");
        sqlx::query(
            "DELETE FROM access_exit_probe_states WHERE access_node_id = $1 AND exit_endpoint_id = $2",
        )
        .bind(node_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM exit_endpoints WHERE id = $1")
            .bind(failover_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE exit_pool_members SET status = 'healthy' WHERE exit_endpoint_id = $1")
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        }
    fn assert_traffic_health_entity(item: &serde_json::Value) {
        for field in [
            "today_real_bytes",
            "week_real_bytes",
            "month_real_bytes",
            "total_real_bytes",
            "peak_hour_real_bytes",
            "low_hour_real_bytes",
        ] {
            assert_eq!(item[field], 900);
        }
        assert!(!item["chart"].as_array().unwrap().is_empty());
    }
