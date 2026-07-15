//! 本模块提供运行时测试的共享支撑。
//! 这里集中放置一次性 HTTP 服务、测试配置构造和样例控制面配置，
//! 避免各测试模块重复搭建相同夹具。

use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xrayc_xray_config::{
    AccessConfig, AccessLine, AccessProtocol, AccessUser, ExitEndpoint, ExitProtocol, LogLevel,
};

use crate::config::{AgentSettings, RuntimeCore};

pub(super) async fn spawn_one_shot_server() -> (String, tokio::task::JoinHandle<String>) {
    let (base_url, requests) = spawn_sequence_server(vec![r#"{"accepted":true}"#]).await;
    let handle = tokio::spawn(async move {
        requests
            .await
            .expect("server task joins")
            .into_iter()
            .next()
            .unwrap_or_default()
    });
    (base_url, handle)
}

pub(super) async fn spawn_sequence_server(
    bodies: Vec<&'static str>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    let handle = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(bodies.len());
        for body in bodies {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buf = vec![0; 8192];
            let read = stream.read(&mut buf).await.expect("read request");
            requests.push(String::from_utf8_lossy(&buf[..read]).to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

pub(super) fn test_settings(base_url: String, dir: &Path) -> AgentSettings {
    AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        xray_config_path: dir.join("config.json"),
        agent_state_path: dir.join("agent-state.json"),
        traffic_backlog_path: dir.join("traffic-backlog.json"),
        xray_access_log_path: dir.join("access.log"),
        xray_binary: "xray".to_owned(),
        xray_test_command: "true".to_owned(),
        xray_reload_command: "true".to_owned(),
        xray_reclaim_command: "true".to_owned(),
        xray_container_name: String::new(),
        xray_start_command: "true".to_owned(),
        xray_stop_command: "true".to_owned(),
        xray_api_server: "127.0.0.1:10085".to_owned(),
        xray_api_listen_host: "127.0.0.1".to_owned(),
        xray_api_listen_port: 10085,
        rate_limiter_enabled: false,
        rate_limiter_dry_run: false,
        rate_limiter_interface: "eth0".to_owned(),
        rate_limiter_ifb_interface: "ifb-xrayc".to_owned(),
        rate_limiter_root_rate_bps: 10_000_000_000,
        rate_limiter_plan_path: dir.join("limiter-plan.sh"),
        tls_cert_domains: Vec::new(),
        certbot_binary: "certbot".to_owned(),
        openssl_binary: "openssl".to_owned(),
        cloudflare_api_token: String::new(),
        cf_cert_mode: "reuse_direct".to_owned(),
        cf_domain: String::new(),
        acme_email: String::new(),
        kernel_connmark_probe_command: String::new(),
        kernel_upgrade_command: String::new(),
        host_reboot_command: String::new(),
        reboot_check_grub_default_command: String::new(),
        reboot_check_container_restart_command: String::new(),
        reboot_check_docker_enabled_command: String::new(),
        // 监控中心磁盘采集挂载点:测试默认对真实 "/" 跑 statvfs,不依赖 /hostfs 挂载。
        host_fs_root: "/".to_owned(),
    }
}

pub(super) fn sample_config() -> AccessConfig {
    AccessConfig {
        node_id: "node-a".to_owned(),
        log_level: LogLevel::Warning,
        stats_enabled: true,
        access_lines: vec![AccessLine {
            id: "line-a".to_owned(),
            source_line_id: String::new(),
            runtime_core: "xray".to_owned(),
            listen_host: "0.0.0.0".to_owned(),
            listen_port: 443,
            transport: "tcp".to_owned(),
            xhttp_path: String::new(),
            xhttp_host: String::new(),
            xhttp_mode: "stream-one".to_owned(),
            inbound_security: None,
            server_name: None,
            reality_dest: None,
            reality_private_key: None,
            reality_short_ids: Vec::new(),
            tls_certificate_file: None,
            tls_key_file: None,
            protocol: AccessProtocol::Vless {
                flow: None,
                decryption: None,
            },
            users: vec![AccessUser {
                xray_user_key: "user-1".to_owned(),
                credential: "11111111-1111-1111-1111-111111111111".to_owned(),
                email: "user-1@xrayc".to_owned(),
                level: 0,
            }],
            default_exit_tag: "exit-socks".to_owned(),
        }],
        exit_endpoints: vec![ExitEndpoint {
            id: "exit-socks".to_owned(),
            tag: "exit-socks".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Socks {
                address: "127.0.0.1".to_owned(),
                port: 1080,
                username: None,
                password: None,
            },
        }],
        local_exit_services: Vec::new(),
        routing_rules: vec![],
        rate_limits: Vec::new(),
    }
}

// 构造含一条 xray 线路与一条历史非 xray 线路的配置，用于验证拆分时
// 只有 xray 承载的线路被保留、非 xray 脏数据被剔除。
pub(super) fn dual_runtime_config() -> AccessConfig {
    let mut config = sample_config();
    config.access_lines[0].id = "line-xray".to_owned();
    config.access_lines[0].source_line_id = "line-xray".to_owned();
    config.access_lines[0].runtime_core = "xray".to_owned();
    config.access_lines.push(AccessLine {
        id: "line-legacy".to_owned(),
        source_line_id: "line-legacy".to_owned(),
        // 历史脏数据：非 xray 运行内核，拆分时必须被剔除。
        runtime_core: "legacy_core".to_owned(),
        listen_host: "0.0.0.0".to_owned(),
        listen_port: 8443,
        transport: "tcp".to_owned(),
        xhttp_path: String::new(),
        xhttp_host: String::new(),
        xhttp_mode: "stream-one".to_owned(),
        inbound_security: None,
        server_name: None,
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: None,
        tls_key_file: None,
        protocol: AccessProtocol::Vless {
            flow: None,
            decryption: None,
        },
        users: vec![AccessUser {
            xray_user_key: "user-2".to_owned(),
            credential: "22222222-2222-2222-2222-222222222222".to_owned(),
            email: "user-2@xrayc".to_owned(),
            level: 0,
        }],
        default_exit_tag: "exit-socks".to_owned(),
    });
    config
}
