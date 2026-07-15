//! 本文件测试 access-agent 控制面 HTTP 客户端。
//! 用例覆盖心跳、流量、运行指标、会话、探测和配置结果上报。
//! 测试只启动本地一次性 HTTP 服务，不连接真实控制面。
//! 请求断言会检查鉴权头和关键 JSON 字段。
//! 探测上报测试确保请求体不携带密码或 secret 字样。
//! 这里拆出 client.rs 的测试以满足单文件行数限制。
//! 新增客户端 DTO 时应在本文件补兼容性解析用例。
//! 测试地址固定为 127.0.0.1 临时端口。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use reqwest::StatusCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::client::*;
use crate::config::RuntimeCore;

#[tokio::test]
async fn test_heartbeat_client_posts_bearer_json() {
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null,"probe_tasks":[{"exit_endpoint_id":"exit-a","requested_at":"2026-05-18T00:00:00Z"}]}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: None,
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    assert!(response.accepted);
    assert_eq!(response.desired_config_version.as_deref(), Some("v2"));
    assert_eq!(response.probe_tasks[0].exit_endpoint_id, "exit-a");
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/heartbeat HTTP/1.1"));
    assert!(request.contains("authorization: Bearer node-token"));
    assert!(request.contains(r#""node_id":"node-a""#));
    assert!(request.contains(r#""core_type":"xray""#));
}

#[tokio::test]
async fn test_heartbeat_client_accepts_probe_task_target() {
    let (base_url, _request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null,"probe_tasks":[{"exit_endpoint_id":"exit-hy2","requested_at":"2026-05-26T00:00:00Z","target":{"protocol":"hysteria","address":"127.0.0.1","port":8443}}]}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: None,
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    let target = response.probe_tasks[0]
        .target
        .as_ref()
        .expect("probe target exists");
    assert_eq!(target.protocol, "hysteria");
    assert_eq!(target.address, "127.0.0.1");
    assert_eq!(target.port, 8443);
}

#[tokio::test]
async fn test_heartbeat_client_posts_tls_reports_and_accepts_renew_task() {
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null,"tls_renew_task":{"request_id":"00000000-0000-0000-0000-000000000901","domains":["relay.example.test"]}}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: vec![TlsCertificateReport {
                domain: "relay.example.test".to_owned(),
                status: "valid".to_owned(),
                not_before: Some("2026-06-01T00:00:00Z".to_owned()),
                not_after: Some("2026-08-30T00:00:00Z".to_owned()),
                days_remaining: Some(80),
                error_summary: String::new(),
            }],
            tls_renew_result: None,
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    let task = response
        .tls_renew_task
        .expect("renew task is parsed from heartbeat response");
    assert_eq!(task.request_id, "00000000-0000-0000-0000-000000000901");
    assert_eq!(task.domains, vec!["relay.example.test".to_owned()]);
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""tls_certificates":[{"domain":"relay.example.test""#));
    assert!(!request.contains("/etc/letsencrypt"));
    assert!(!request.contains("privkey"));
}

#[tokio::test]
async fn test_heartbeat_client_posts_tls_renew_result() {
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: Some(TlsRenewResult {
                request_id: "00000000-0000-0000-0000-000000000901".to_owned(),
                status: "success".to_owned(),
                message: "renewed 1 certificate".to_owned(),
            }),
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    let request = request.await.expect("server task joins");
    assert!(request
        .contains(r#""tls_renew_result":{"request_id":"00000000-0000-0000-0000-000000000901""#));
    assert!(request.contains(r#""status":"success""#));
}

#[tokio::test]
async fn test_heartbeat_client_accepts_runtime_core_tasks() {
    let (base_url, _request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null,"runtime_core_tasks":[{"request_id":"00000000-0000-0000-0000-000000000902","core_type":"xray","action":"stop"}]}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: None,
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    let task = response
        .runtime_core_tasks
        .first()
        .expect("runtime core task is parsed");
    assert_eq!(task.request_id, "00000000-0000-0000-0000-000000000902");
    assert_eq!(task.core_type, RuntimeCore::Xray);
    assert_eq!(task.action, RuntimeCoreAction::Stop);
}

#[tokio::test]
async fn test_heartbeat_client_posts_runtime_core_results() {
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: None,
            runtime_core_results: vec![RuntimeCoreControlResult {
                request_id: "00000000-0000-0000-0000-000000000902".to_owned(),
                core_type: RuntimeCore::Xray,
                action: RuntimeCoreAction::Stop,
                success: true,
                message: "stopped xray runtime core".to_owned(),
            }],
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: None,
        })
        .await
        .expect("heartbeat succeeds");

    let request = request.await.expect("server task joins");
    assert!(request.contains(
        r#""runtime_core_results":[{"request_id":"00000000-0000-0000-0000-000000000902""#
    ));
    assert!(request.contains(r#""core_type":"xray""#));
    assert!(request.contains(r#""action":"stop""#));
    assert!(request.contains(r#""success":true"#));
}

#[tokio::test]
async fn test_heartbeat_omits_node_metrics_when_none_and_posts_when_present() {
    // 监控中心宿主指标(阶段B):node_metrics 缺省(None)时绝不出现在心跳 JSON(向后兼容),
    // 有采集结果(Some)时按 snake_case 字段如实上报。
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v2","config":null}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    // 先发一条不带 node_metrics 的心跳,确认序列化里完全没有该键。
    let absent = serde_json::to_string(&HeartbeatRequest {
        node_id: "node-a".to_owned(),
        agent_version: "0.1.0".to_owned(),
        hostname: "agent-host".to_owned(),
        core_type: RuntimeCore::Xray,
        xray_version: None,
        applied_config_version: Some("v1".to_owned()),
        uptime_seconds: 42,
        tls_certificates: Vec::new(),
        tls_renew_result: None,
        runtime_core_results: Vec::new(),
        kernel_connmark_available: None,
        kernel_upgrade_pending: None,
        reboot_result: None,
        node_metrics: None,
    })
    .expect("serialize without node_metrics");
    assert!(
        !absent.contains("node_metrics"),
        "node_metrics 为 None 时不应出现在心跳 JSON,实际: {absent}"
    );

    client
        .heartbeat(&HeartbeatRequest {
            node_id: "node-a".to_owned(),
            agent_version: "0.1.0".to_owned(),
            hostname: "agent-host".to_owned(),
            core_type: RuntimeCore::Xray,
            xray_version: None,
            applied_config_version: Some("v1".to_owned()),
            uptime_seconds: 42,
            tls_certificates: Vec::new(),
            tls_renew_result: None,
            runtime_core_results: Vec::new(),
            kernel_connmark_available: None,
            kernel_upgrade_pending: None,
            reboot_result: None,
            node_metrics: Some(NodeMetricsReport {
                cpu_pct_milli: 70_000,
                mem_used_bytes: 6_291_456_000,
                mem_total_bytes: 8_388_608_000,
                disk_used_bytes: 10_000_000_000,
                disk_total_bytes: 50_000_000_000,
                collected_at_unix: 1_700_000_000,
            }),
        })
        .await
        .expect("heartbeat succeeds");

    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""node_metrics":{"cpu_pct_milli":70000"#));
    assert!(request.contains(r#""mem_used_bytes":6291456000"#));
    assert!(request.contains(r#""disk_total_bytes":50000000000"#));
}

#[tokio::test]
async fn test_report_traffic_client_posts_snapshots() {
    let (base_url, request) = spawn_one_shot_server(
        200,
        r#"{"accepted":true,"desired_config_version":"v3","config_status":{"required":true,"reason":"quota_exhausted"},"config":null}"#,
    )
    .await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .report_traffic(&TrafficReportRequest {
            node_id: "node-a".to_owned(),
            applied_config_version: Some("applied-v2".to_owned()),
            snapshots: vec![TrafficSnapshot {
                access_line_id: Some("line-a".to_owned()),
                xray_user_key: "user-1".to_owned(),
                uplink_bytes: 100,
                downlink_bytes: 200,
                captured_at_unix: 1_700_000_000,
            }],
        })
        .await
        .expect("traffic report succeeds");

    assert!(response.accepted);
    assert_eq!(response.desired_config_version.as_deref(), Some("v3"));
    assert!(response.config_status.required);
    assert_eq!(
        response.config_status.reason.as_deref(),
        Some("quota_exhausted")
    );
    assert!(response.config.is_none());
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/traffic HTTP/1.1"));
    assert!(request.contains(r#""applied_config_version":"applied-v2""#));
    assert!(request.contains(r#""access_line_id":"line-a""#));
    assert!(request.contains(r#""xray_user_key":"user-1""#));
}

#[tokio::test]
async fn test_report_metrics_client_posts_metrics() {
    let (base_url, request) = spawn_one_shot_server(200, r#"{"accepted":true}"#).await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .report_metrics(&MetricsReportRequest {
            access_node_id: "node-a".to_owned(),
            metrics: vec![AccessLineMetric {
                access_node_id: "node-a".to_owned(),
                access_line_id: "line-a".to_owned(),
                online_users: 1,
                active_connections: 1,
                unique_client_ips: 1,
                uplink_rate_bps: 800,
                downlink_rate_bps: 1600,
                collected_at_unix: 1_700_000_000,
            }],
        })
        .await
        .expect("metrics report succeeds");

    assert!(response.accepted);
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/metrics HTTP/1.1"));
    assert!(request.contains("authorization: Bearer node-token"));
    assert!(request.contains(r#""access_line_id":"line-a""#));
    assert!(request.contains(r#""uplink_rate_bps":800"#));
}

#[tokio::test]
async fn test_report_sessions_client_posts_sessions() {
    let (base_url, request) = spawn_one_shot_server(200, r#"{"accepted":true}"#).await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .report_sessions(&SessionsReportRequest {
            access_node_id: "node-a".to_owned(),
            sessions: Vec::new(),
        })
        .await
        .expect("sessions report succeeds");

    assert!(response.accepted);
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/sessions HTTP/1.1"));
    assert!(request.contains(r#""access_node_id":"node-a""#));
    assert!(request.contains(r#""sessions":[]"#));
}

#[tokio::test]
async fn test_report_probes_client_posts_probes_without_credentials() {
    let (base_url, request) = spawn_one_shot_server(200, r#"{"accepted":true}"#).await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let response = client
        .report_probes(&ProbesReportRequest {
            access_node_id: "node-a".to_owned(),
            line_probes: vec![AccessLineProbe {
                access_line_id: "line-a".to_owned(),
                status: "healthy".to_owned(),
                latency_ms: Some(1),
                error_summary: String::new(),
                probed_at_unix: 1_700_000_000,
            }],
            exit_probes: vec![AccessExitProbe {
                exit_endpoint_id: "exit-a".to_owned(),
                status: "healthy".to_owned(),
                latency_ms: Some(1),
                error_summary: String::new(),
                probed_at_unix: 1_700_000_000,
            }],
        })
        .await
        .expect("probes report succeeds");

    assert!(response.accepted);
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/probes HTTP/1.1"));
    assert!(request.contains(r#""exit_endpoint_id":"exit-a""#));
    assert!(!request.contains("password"));
    assert!(!request.contains("secret"));
}

#[tokio::test]
async fn test_config_result_surfaces_http_error() {
    let (base_url, request) = spawn_one_shot_server(500, "boom").await;
    let client = AgentClient::new(base_url, "node-token").expect("client builds");

    let error = client
        .report_config_result(&ConfigResultRequest {
            node_id: "node-a".to_owned(),
            config_version: "v2".to_owned(),
            success: false,
            message: Some("compile failed".to_owned()),
        })
        .await
        .expect_err("server error should be returned");

    match error {
        AgentClientError::Status { status, body } => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert!(body.contains("redacted response body"));
            assert!(!body.contains("boom"));
        }
        other => panic!("unexpected error: {other}"),
    }
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/config-result HTTP/1.1"));
}

async fn spawn_one_shot_server(
    status: u16,
    body: &'static str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut buf = vec![0; 8192];
        let read = stream.read(&mut buf).await.expect("read request");
        let request = String::from_utf8_lossy(&buf[..read]).to_string();
        let reason = if status == 200 { "OK" } else { "ERR" };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        request
    });
    (format!("http://{addr}"), handle)
}
