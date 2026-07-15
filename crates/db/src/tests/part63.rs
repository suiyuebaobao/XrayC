/// 数据库测试分片 63。
// 本文件覆盖中转节点内核能力软状态上报与整机重启请求/命令下发(§7.7.1)。
// 内核状态只保存软信号(connmark 可用性 + 是否待重启),不保存宿主明文细节。
// 整机重启请求只记 request_id + queued 状态,经心跳下发 reboot_task,agent 自检后执行。
// 重启结果据 request_id 幂等清理待执行请求,迟到/重复结果不误清新请求。
// 缺少 DATABASE_URL 时跳过,避免无 PostgreSQL 环境误报。
// 测试只用 example.test 域名和随机 UUID,不写真实主机/凭据。
// SQL 与断言必须围绕真实 PostgreSQL 行为。
// 后续节点控制策略调整时优先维护本分片。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_access_node_kernel_status_and_reboot_request_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL access node kernel/reboot test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "kernel-reboot-node".to_string(),
            public_host: "kernel-reboot.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-kernel-reboot-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // 默认(未上报)按可用、无待重启:读模型必含键且为兜底值。
    let node = find_routing_node(&store, node_id).await;
    assert_eq!(node["kernel_connmark_available"], true);
    assert_eq!(node["kernel_upgrade_pending"], false);
    assert_eq!(node["reboot_status"], "");

    // agent 上报内核降级 + 已装新内核待重启:落库刷新读模型。
    store
        .record_agent_kernel_and_reboot_status(node_id, Some(false), Some(true), None)
        .await
        .unwrap();
    let node = find_routing_node(&store, node_id).await;
    assert_eq!(node["kernel_connmark_available"], false);
    assert_eq!(node["kernel_upgrade_pending"], true);

    // 缺省上报(都为 None)不应误清已落库的降级/待重启软状态(向后兼容旧 agent)。
    store
        .record_agent_kernel_and_reboot_status(node_id, None, None, None)
        .await
        .unwrap();
    let node = find_routing_node(&store, node_id).await;
    assert_eq!(node["kernel_connmark_available"], false);
    assert_eq!(node["kernel_upgrade_pending"], true);

    // 管理员触发整机重启:记 queued 请求,心跳下发 reboot_task。
    let reboot = store.request_access_node_reboot(node_id).await.unwrap();
    assert_eq!(reboot.status, "queued");
    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    assert_eq!(
        heartbeat["reboot_task"]["request_id"],
        reboot.request_id.to_string()
    );

    // 重复触发幂等:不堆叠新请求,返回同一 request_id。
    let reboot_again = store.request_access_node_reboot(node_id).await.unwrap();
    assert_eq!(reboot_again.request_id, reboot.request_id);

    // agent 回报重启结果(失败:自检未过):据 request_id 落库并清理待执行请求。
    store
        .record_agent_kernel_and_reboot_status(
            node_id,
            Some(true),
            Some(false),
            Some((
                reboot.request_id,
                "failed".to_string(),
                "GRUB 默认未引导最高版本内核,重启后仍会跑旧内核".to_string(),
            )),
        )
        .await
        .unwrap();
    let node = find_routing_node(&store, node_id).await;
    assert_eq!(node["reboot_status"], "failed");
    assert!(node["reboot_message"]
        .as_str()
        .unwrap()
        .contains("GRUB"));
    // 内核恢复可用、待重启复位(管理员重启换新内核后 agent 自检恢复)。
    assert_eq!(node["kernel_connmark_available"], true);
    assert_eq!(node["kernel_upgrade_pending"], false);

    // 结果落库后待执行请求已清理:心跳不再下发 reboot_task。
    let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
    assert!(heartbeat["reboot_task"].is_null());
}

/// 非法重启状态必须被拒绝入库(白名单校验),不污染节点 reboot_status。
#[tokio::test]
async fn test_pg_record_reboot_status_rejects_invalid_status_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL reboot status validation test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "reboot-invalid-node".to_string(),
            public_host: "reboot-invalid.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-reboot-invalid-token".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let reboot = store.request_access_node_reboot(node_id).await.unwrap();

    let err = store
        .record_agent_kernel_and_reboot_status(
            node_id,
            None,
            None,
            Some((reboot.request_id, "exploded".to_string(), "x".to_string())),
        )
        .await
        .expect_err("invalid reboot status must be rejected");
    assert!(matches!(err, DbError::InvalidAgentPayload(_)));
}

/// 共享 helper:从 access_routing_json 取指定节点对象。
async fn find_routing_node(store: &PgStore, node_id: Uuid) -> serde_json::Value {
    let routing = store.access_routing_json().await.unwrap();
    routing["access_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == node_id.to_string())
        .unwrap()
        .clone()
}
