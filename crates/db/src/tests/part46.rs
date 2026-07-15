/// 数据库测试分片 46。
// 本文件覆盖本机出口服务的 runtime 渲染回归。
// 测试只使用示例域名、RFC 文档地址和测试 UUID。
// 本机出口 XUDP 在存储层会保留 network_mode，但 runtime network 必须是 TCP。
// 这里防止 XUDP 被误写进 Xray inbound streamSettings.network。
// PostgreSQL 用例缺少 DATABASE_URL 时只输出脱敏跳过原因。
// 父级 tests 模块提供 PgStore、输入类型、json 和 pg_test_guard。
// 新增本机出口渲染回归优先放在这里，避免旧分片超长。
// 不在这里写真实服务器、token、订阅或代理凭据。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_local_exit_vless_xudp_renders_tcp_service_network_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local VLESS XUDP service network test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-vless-xudp-node".to_string(),
            public_host: "local-vless-xudp.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-local-vless-xudp-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "local-vless-xudp-resource".to_string(),
                    endpoint_name: "local-vless-xudp-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "vless".to_string(),
                    network_mode: "xudp".to_string(),
                    host: "local-vless-xudp.example.test".to_string(),
                    port: 39_044,
                    outbound_config: json!({
                        "uuid": "00000000-0000-4000-8000-000000000044",
                        "security": "none"
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

    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    let service = &heartbeat["config"]["local_exit_services"][0];

    assert_eq!(service["protocol"], "vless");
    assert_eq!(service["network"], "tcp");
}

#[tokio::test]
async fn test_pg_operations_summary_reports_production_alerts_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL operations alert summary test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "alert-offline-node".to_string(),
            public_host: "alert-offline.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-alert-offline-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE access_nodes
        SET last_heartbeat_at = now() - interval '10 minutes',
            tls_certificates = $2::jsonb,
            tls_cert_last_report_at = now()
        WHERE id = $1
        "#,
    )
    .bind(node_id)
    .bind(json!([{
        "domain": "alert-offline.example.test",
        "status": "expired",
        "days_remaining": -1,
        "error_summary": "certificate expired"
    }]))
    .execute(store.pool())
    .await
    .unwrap();
    store
        .update_database_backup_state_json(json!({
            "last_status": "failed",
            "last_error": "pg_dump failed",
            "last_finished_at": Utc::now()
        }))
        .await
        .unwrap();

    let summary = store.operations_summary_json().await.unwrap();
    let alerts = summary["alerts"].as_array().unwrap();

    assert!(alerts.iter().any(|alert| alert["kind"] == "access_node_offline"
        && alert["resource_id"] == node_id.to_string()
        && alert["severity"] == "danger"));
    assert!(alerts.iter().any(|alert| alert["kind"] == "ssl_certificate_expired"
        && alert["resource_id"] == node_id.to_string()
        && alert["severity"] == "danger"));
    assert!(alerts.iter().any(|alert| alert["kind"] == "database_backup_failed"
        && alert["severity"] == "danger"));
    assert_eq!(summary["alert_summary"]["danger_count"], 3);
}

#[test]
fn test_runtime_metric_status_without_configured_lines_is_no_data() {
    assert_eq!(runtime_metric_status(0, 0, None), "no_data");
}

#[tokio::test]
async fn test_pg_access_routing_reports_heartbeat_health_in_node_status_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL access node status read model test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "heartbeat-status-node".to_string(),
            public_host: "heartbeat-status.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-heartbeat-status-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    sqlx::query("UPDATE access_nodes SET status = 'unknown', config_dirty = FALSE WHERE id = $1")
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();

    store.heartbeat_json(Some(node_id), None).await.unwrap();
    let desired_config_hash = sqlx::query_scalar::<_, Option<String>>(
        "SELECT desired_config_hash FROM access_nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .unwrap();
    store
        .heartbeat_json(Some(node_id), Some(&desired_config_hash))
        .await
        .unwrap();
    let routing = store.access_routing_json().await.unwrap();
    let node = routing["access_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == node_id.to_string())
        .unwrap();

    assert_eq!(node["health_status"], "healthy");
    assert_eq!(node["status"], "healthy");
}
