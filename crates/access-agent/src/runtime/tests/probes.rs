//! 本模块测试运行时探测结果生成。
//! 用例覆盖探测记录脱敏、本地入口探测地址选择、出口全集合探测和
//! 手动探测任务只访问当前配置端点的行为。

use tokio::net::{TcpListener, UdpSocket};
use xrayc_xray_config::{AccessProtocol, ExitEndpoint, ExitProtocol};

use crate::client::{ProbeTask, ProbeTaskTarget};

use super::super::probes::{
    access_line_probe_host, collect_probe_results, collect_probe_task_results, exit_probe_record,
    line_probe_record, probe_access_line, ProbeOutcome,
};
use super::support::sample_config;

#[test]
fn test_probe_results_use_config_ids_without_credentials() {
    let mut config = sample_config();
    config.exit_endpoints = vec![ExitEndpoint {
        id: "exit-secret".to_owned(),
        tag: "exit-secret".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "203.0.113.10".to_owned(),
            port: 1080,
            username: Some("proxy-user".to_owned()),
            password: Some("proxy-password".to_owned()),
        },
    }];
    config.access_lines[0].id = "line-secret".to_owned();
    config.access_lines[0].default_exit_tag = "exit-secret".to_owned();

    let line_probe = line_probe_record(
        &config.access_lines[0].id,
        1_700_000_000,
        ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some("TCP 拨测失败: 连接失败".to_owned()),
        },
    );
    let exit_probe = exit_probe_record(
        &config.exit_endpoints[0].id,
        1_700_000_000,
        ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some("TCP 拨测失败: 连接失败".to_owned()),
        },
    );
    let json =
        serde_json::to_string(&(line_probe.clone(), exit_probe.clone())).expect("probe serializes");

    assert_eq!(line_probe.access_line_id, "line-secret");
    assert_eq!(line_probe.status, "unhealthy");
    assert_eq!(exit_probe.exit_endpoint_id, "exit-secret");
    assert_eq!(exit_probe.status, "unhealthy");
    assert!(!json.contains("proxy-user"));
    assert!(!json.contains("proxy-password"));
    assert!(!json.contains("203.0.113.10"));
}

#[tokio::test]
async fn test_access_line_probe_uses_local_listen_endpoint() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener binds");
    let port = listener.local_addr().expect("listener addr").port();
    let accept_task = tokio::spawn(async move {
        let _ = listener.accept().await;
    });
    let mut config = sample_config();
    config.access_lines[0].listen_host = "0.0.0.0".to_owned();
    config.access_lines[0].listen_port = port;

    let outcome = probe_access_line(&config.access_lines[0]).await;

    assert_eq!(outcome.status, "healthy");
    assert!(outcome.latency_ms.is_some());
    accept_task.abort();
}

#[tokio::test]
async fn test_hy2_access_line_probe_uses_udp_listen_endpoint() {
    let socket = UdpSocket::bind(("127.0.0.1", 0))
        .await
        .expect("udp listener binds");
    let port = socket.local_addr().expect("udp listener addr").port();
    let receive_task = tokio::spawn(async move {
        let mut buf = [0_u8; 32];
        let _ = socket.recv_from(&mut buf).await;
    });
    let mut config = sample_config();
    config.access_lines[0].listen_host = "0.0.0.0".to_owned();
    config.access_lines[0].listen_port = port;
    config.access_lines[0].transport = "hysteria".to_owned();
    config.access_lines[0].protocol = AccessProtocol::Hysteria2;

    let outcome = probe_access_line(&config.access_lines[0]).await;

    assert_eq!(outcome.status, "healthy");
    assert!(outcome.latency_ms.is_some());
    receive_task.abort();
}

#[test]
fn test_access_line_probe_host_maps_wildcard_to_loopback() {
    assert_eq!(access_line_probe_host("0.0.0.0"), "127.0.0.1");
    assert_eq!(access_line_probe_host("::"), "127.0.0.1");
    assert_eq!(access_line_probe_host("203.0.113.10"), "203.0.113.10");
}

#[tokio::test]
async fn test_probe_results_include_all_configured_exit_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let port = listener.local_addr().expect("listener addr").port();
    let accept_task = tokio::spawn(async move {
        for _ in 0..2 {
            let _ = listener.accept().await;
        }
    });
    let mut config = sample_config();
    config.exit_endpoints.push(ExitEndpoint {
        id: "exit-http".to_owned(),
        tag: "exit-http".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Http {
            address: "127.0.0.1".to_owned(),
            port,
            username: None,
            password: None,
        },
    });
    config.exit_endpoints[0] = ExitEndpoint {
        id: "exit-socks".to_owned(),
        tag: "exit-socks".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port,
            username: None,
            password: None,
        },
    };

    let (line_probes, exit_probes) = collect_probe_results(&config, 1_700_000_000).await;

    assert_eq!(line_probes.len(), 1);
    assert_eq!(exit_probes.len(), 2);
    assert!(exit_probes
        .iter()
        .any(|probe| probe.exit_endpoint_id == "exit-socks"));
    assert!(exit_probes
        .iter()
        .any(|probe| probe.exit_endpoint_id == "exit-http"));
    accept_task.abort();
}

#[tokio::test]
async fn test_probe_results_report_source_line_id_for_runtime_access_line() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let port = listener.local_addr().expect("listener addr").port();
    let accept_task = tokio::spawn(async move {
        let _ = listener.accept().await;
    });
    let mut config = sample_config();
    config.access_lines[0].id = "line-a-user-runtime".to_string();
    config.access_lines[0].source_line_id = "line-a".to_string();
    config.access_lines[0].listen_host = "0.0.0.0".to_string();
    config.access_lines[0].listen_port = port;

    let (line_probes, _) = collect_probe_results(&config, 1_700_000_000).await;

    assert_eq!(line_probes.len(), 1);
    assert_eq!(line_probes[0].access_line_id, "line-a");
    accept_task.abort();
}

#[tokio::test]
async fn test_marked_exit_probe_reports_source_endpoint_id() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let port = listener.local_addr().expect("listener addr").port();
    let accept_task = tokio::spawn(async move {
        for _ in 0..2 {
            let _ = listener.accept().await;
        }
    });
    let source_endpoint_id = "11111111-1111-1111-1111-111111111111";
    let synthetic_endpoint_id =
        "11111111-1111-1111-1111-111111111111:22222222-2222-2222-2222-222222222222";
    let mut config = sample_config();
    config.exit_endpoints[0] = ExitEndpoint {
        id: synthetic_endpoint_id.to_owned(),
        tag: "exit-marked".to_owned(),
        sockopt_mark: Some(65_537),
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port,
            username: None,
            password: None,
        },
    };

    let (_line_probes, exit_probes) = collect_probe_results(&config, 1_700_000_000).await;
    let tasks = vec![ProbeTask {
        exit_endpoint_id: source_endpoint_id.to_owned(),
        requested_at: Some("2026-06-04T00:00:00Z".to_owned()),
        target: None,
    }];
    let (_manual_lines, manual_exit_probes) =
        collect_probe_task_results(Some(&config), &tasks, 1_700_000_001).await;

    assert_eq!(exit_probes.len(), 1);
    assert_eq!(exit_probes[0].exit_endpoint_id, source_endpoint_id);
    assert_eq!(exit_probes[0].status, "healthy");
    assert_eq!(manual_exit_probes.len(), 1);
    assert_eq!(manual_exit_probes[0].exit_endpoint_id, source_endpoint_id);
    assert_eq!(manual_exit_probes[0].status, "healthy");
    accept_task.abort();
}

#[tokio::test]
async fn test_hy2_exit_probe_uses_udp_endpoint() {
    let socket = UdpSocket::bind(("127.0.0.1", 0))
        .await
        .expect("udp listener binds");
    let port = socket.local_addr().expect("udp listener addr").port();
    let receive_task = tokio::spawn(async move {
        let mut buf = [0_u8; 32];
        let _ = socket.recv_from(&mut buf).await;
    });
    let mut config = sample_config();
    config.exit_endpoints[0] = ExitEndpoint {
        id: "exit-hy2".to_owned(),
        tag: "exit-hy2".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Hysteria2 {
            address: "127.0.0.1".to_owned(),
            port,
            password: "hy2-secret".to_owned(),
            server_name: Some("hy2.example.test".to_owned()),
            allow_insecure: Some(true),
        },
    };

    let (_line_probes, exit_probes) = collect_probe_results(&config, 1_700_000_000).await;

    let probe = exit_probes
        .iter()
        .find(|probe| probe.exit_endpoint_id == "exit-hy2")
        .expect("hy2 probe exists");
    assert_eq!(probe.status, "healthy");
    assert!(probe.latency_ms.is_some());
    assert_eq!(probe.error_summary, "");
    receive_task.abort();
}

#[tokio::test]
async fn test_manual_probe_tasks_probe_configured_endpoint_only() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let port = listener.local_addr().expect("listener addr").port();
    let accept_task = tokio::spawn(async move {
        let _ = listener.accept().await;
    });
    let mut config = sample_config();
    config.exit_endpoints[0] = ExitEndpoint {
        id: "exit-socks".to_owned(),
        tag: "exit-socks".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port,
            username: None,
            password: None,
        },
    };
    let tasks = vec![
        ProbeTask {
            exit_endpoint_id: "exit-socks".to_owned(),
            requested_at: Some("2026-05-18T00:00:00Z".to_owned()),
            target: None,
        },
        ProbeTask {
            exit_endpoint_id: "missing-exit".to_owned(),
            requested_at: None,
            target: None,
        },
    ];

    let (line_probes, exit_probes) =
        collect_probe_task_results(Some(&config), &tasks, 1_700_000_000).await;

    assert!(line_probes.is_empty());
    assert_eq!(exit_probes.len(), 2);
    assert_eq!(exit_probes[0].exit_endpoint_id, "exit-socks");
    assert_eq!(exit_probes[0].status, "healthy");
    assert_eq!(exit_probes[1].exit_endpoint_id, "missing-exit");
    assert_eq!(exit_probes[1].status, "unknown");
    accept_task.abort();
}

#[tokio::test]
async fn test_manual_hy2_probe_task_uses_explicit_udp_target_without_active_config() {
    let socket = UdpSocket::bind(("127.0.0.1", 0))
        .await
        .expect("udp listener binds");
    let port = socket.local_addr().expect("udp listener addr").port();
    let receive_task = tokio::spawn(async move {
        let mut buf = [0_u8; 32];
        let _ = socket.recv_from(&mut buf).await;
    });
    let tasks = vec![ProbeTask {
        exit_endpoint_id: "exit-hy2-manual".to_owned(),
        requested_at: Some("2026-05-26T00:00:00Z".to_owned()),
        target: Some(ProbeTaskTarget {
            protocol: "hysteria".to_owned(),
            address: "127.0.0.1".to_owned(),
            port,
        }),
    }];

    let (_line_probes, exit_probes) = collect_probe_task_results(None, &tasks, 1_700_000_000).await;

    assert_eq!(exit_probes.len(), 1);
    assert_eq!(exit_probes[0].exit_endpoint_id, "exit-hy2-manual");
    assert_eq!(exit_probes[0].status, "healthy");
    assert_eq!(exit_probes[0].error_summary, "");
    receive_task.abort();
}

#[tokio::test]
async fn test_manual_hy2_probe_reports_unhealthy_when_udp_port_is_closed() {
    let port = closed_udp_probe_test_port().await;
    let tasks = vec![ProbeTask {
        exit_endpoint_id: "exit-hy2-closed".to_owned(),
        requested_at: Some("2026-05-26T00:00:00Z".to_owned()),
        target: Some(ProbeTaskTarget {
            protocol: "hysteria2".to_owned(),
            address: "127.0.0.1".to_owned(),
            port,
        }),
    }];

    let (_line_probes, exit_probes) = collect_probe_task_results(None, &tasks, 1_700_000_000).await;

    assert_eq!(exit_probes.len(), 1);
    assert_eq!(exit_probes[0].exit_endpoint_id, "exit-hy2-closed");
    assert_eq!(exit_probes[0].status, "unhealthy");
    assert!(exit_probes[0].error_summary.contains("UDP"));
}

async fn closed_udp_probe_test_port() -> u16 {
    for port in 24_000..24_100 {
        if let Ok(socket) = UdpSocket::bind(("127.0.0.1", port)).await {
            drop(socket);
            return port;
        }
    }
    panic!("no closed UDP probe test port available");
}

#[tokio::test]
async fn test_manual_probe_task_target_overrides_stale_active_config() {
    let socket = UdpSocket::bind(("127.0.0.1", 0))
        .await
        .expect("udp listener binds");
    let port = socket.local_addr().expect("udp listener addr").port();
    let receive_task = tokio::spawn(async move {
        let mut buf = [0_u8; 32];
        let _ = socket.recv_from(&mut buf).await;
    });
    let mut config = sample_config();
    config.exit_endpoints[0] = ExitEndpoint {
        id: "exit-stale".to_owned(),
        tag: "exit-stale".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port: 9,
            username: None,
            password: None,
        },
    };
    let tasks = vec![ProbeTask {
        exit_endpoint_id: "exit-stale".to_owned(),
        requested_at: Some("2026-05-26T00:00:00Z".to_owned()),
        target: Some(ProbeTaskTarget {
            protocol: "hysteria".to_owned(),
            address: "127.0.0.1".to_owned(),
            port,
        }),
    }];

    let (_line_probes, exit_probes) =
        collect_probe_task_results(Some(&config), &tasks, 1_700_000_000).await;

    assert_eq!(exit_probes.len(), 1);
    assert_eq!(exit_probes[0].exit_endpoint_id, "exit-stale");
    assert_eq!(exit_probes[0].status, "healthy");
    receive_task.abort();
}
