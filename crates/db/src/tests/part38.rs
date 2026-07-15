/// 数据库测试分片 38。
// 本文件覆盖中转节点 SSL 证书状态上报和单节点续期任务。
// 证书状态只保存脱敏摘要，不保存证书正文、私钥或 certbot 原始输出。
// 续期任务按 access_node_id 下发，不能跨节点批量执行。
// 心跳响应携带待续期任务，agent 下一次心跳回报执行结果。
// 测试只使用 example.test 域名和随机 UUID。
// 缺少 DATABASE_URL 时跳过，避免无 PostgreSQL 环境误报。
// SQL 与断言必须围绕真实 PostgreSQL 行为。
// 后续续期协议调整时优先维护本分片。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_access_node_tls_status_and_renewal_task_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL access node TLS status test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = uuid("00000000-0000-0000-0000-000000000201");
    let certificate = AgentTlsCertificateReport {
        domain: "tls-node.example.test".to_string(),
        status: "valid".to_string(),
        not_before: Some("2026-06-01T00:00:00Z".to_string()),
        not_after: Some("2026-08-30T00:00:00Z".to_string()),
        days_remaining: Some(80),
        error_summary: String::new(),
    };

    store
        .record_agent_tls_status(node_id, vec![certificate], None)
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let node = routing["access_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == node_id.to_string())
        .unwrap();
    assert_eq!(node["tls_certificates"][0]["domain"], "tls-node.example.test");
    assert_eq!(node["tls_certificates"][0]["status"], "valid");
    assert_eq!(node["tls_certificates"][0]["days_remaining"], 80);
    assert!(node["tls_cert_last_report_at"].as_str().is_some());

    let renewal = store.request_access_node_tls_renewal(node_id).await.unwrap();
    assert_eq!(renewal.status, "queued");
    assert_eq!(renewal.domains, vec!["tls-node.example.test".to_string()]);

    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    assert_eq!(
        heartbeat["tls_renew_task"]["request_id"],
        renewal.request_id.to_string()
    );
    assert_eq!(heartbeat["tls_renew_task"]["domains"][0], "tls-node.example.test");

    store
        .record_agent_tls_status(
            node_id,
            Vec::new(),
            Some(AgentTlsRenewResult {
                request_id: renewal.request_id,
                status: "success".to_string(),
                message: "renewed 1 certificate".to_string(),
            }),
        )
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let node = routing["access_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == node_id.to_string())
        .unwrap();
    assert_eq!(node["tls_renew_status"], "success");
    assert_eq!(node["tls_renew_message"], "renewed 1 certificate");
    assert!(node["tls_renew_completed_at"].as_str().is_some());

    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    assert!(heartbeat["tls_renew_task"].is_null());
}
