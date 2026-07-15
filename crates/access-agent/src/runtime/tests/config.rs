//! 本模块测试配置应用和控制面刷新路径。
//! 用例覆盖成功写入、失败回滚、空配置下发、流量回报触发刷新和
//! 活动配置缓存恢复，确保拆分后的配置模块保持原行为。

use std::fs;

use tempfile::tempdir;

use crate::client::{AgentClient, ConfigStatus, TrafficSnapshot};
use crate::config::{AgentSettings, RuntimeCore};

use super::super::apply::{apply_config, apply_control_plane_config, apply_empty_config};
use super::super::cache::load_active_config_cache;
use super::super::reporting::report_traffic_snapshot;
use super::support::{sample_config, spawn_one_shot_server, spawn_sequence_server};

#[tokio::test]
async fn test_apply_config_writes_xray_json_and_reports_success() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: path.clone(),
        xray_binary: "xray".to_owned(),
        xray_test_command: "true".to_owned(),
        xray_reload_command: "true".to_owned(),
        xray_reclaim_command: "true".to_owned(),
        xray_container_name: String::new(),
        xray_start_command: "true".to_owned(),
        xray_stop_command: "true".to_owned(),
        xray_api_server: "127.0.0.1:10085".to_owned(),
        xray_api_listen_host: "127.0.0.1".to_owned(),
        xray_api_listen_port: 11085,
        rate_limiter_enabled: false,
        rate_limiter_dry_run: false,
        rate_limiter_interface: "eth0".to_owned(),
        rate_limiter_ifb_interface: "ifb-xrayc".to_owned(),
        rate_limiter_root_rate_bps: 10_000_000_000,
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v1",
        &sample_config(),
    )
    .await
    .expect("config applies");

    let rendered = fs::read_to_string(path).expect("config was written");
    assert!(rendered.contains("\"protocol\": \"vless\""));
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let api_inbound = json["inbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|inbound| inbound["tag"] == "xrayc-api")
        .expect("stats api inbound exists");
    assert_eq!(api_inbound["port"], 11085);
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/config-result HTTP/1.1"));
    assert!(request.contains(r#""success":true"#));
}

#[tokio::test]
async fn test_control_plane_config_only_retains_xray_access_lines() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let xray_path = dir.path().join("xray.json");
    let mut settings = super::support::test_settings(base_url, dir.path());
    settings.xray_config_path = xray_path.clone();
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    // 控制面仍可能在历史脏数据里带上非 xray 线路，写入前必须被拆分剔除，
    // 只有 xray 承载的线路才落进 Xray 配置。
    let outcome = apply_control_plane_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        Some("xray-v1".to_owned()),
        ConfigStatus {
            required: true,
            reason: Some("runtime_core_changed".to_owned()),
        },
        Some(super::support::dual_runtime_config()),
        true,
    )
    .await
    .expect("xray runtime config applies");

    assert_eq!(outcome.applied_config_version.as_deref(), Some("xray-v1"));
    let xray_rendered = fs::read_to_string(xray_path).expect("xray config was written");
    assert!(xray_rendered.contains("line-xray"));
    assert!(!xray_rendered.contains("line-legacy"));
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""config_version":"xray-v1""#));
    assert!(request.contains(r#""success":true"#));
}

#[tokio::test]
async fn test_apply_config_rolls_back_when_reload_fails() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(&path, "previous-good-config").expect("write previous config");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: path.clone(),
        xray_binary: "xray".to_owned(),
        xray_test_command: "true".to_owned(),
        xray_reload_command: "false".to_owned(),
        xray_reclaim_command: "true".to_owned(),
        xray_container_name: String::new(),
        xray_start_command: "true".to_owned(),
        xray_stop_command: "true".to_owned(),
        xray_api_server: "127.0.0.1:10085".to_owned(),
        xray_api_listen_host: "127.0.0.1".to_owned(),
        xray_api_listen_port: 11085,
        rate_limiter_enabled: false,
        rate_limiter_dry_run: false,
        rate_limiter_interface: "eth0".to_owned(),
        rate_limiter_ifb_interface: "ifb-xrayc".to_owned(),
        rate_limiter_root_rate_bps: 10_000_000_000,
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v1",
        &sample_config(),
    )
    .await
    .expect("failure is reported to control plane");

    assert!(!applied);
    assert_eq!(
        fs::read_to_string(path).expect("config restored"),
        "previous-good-config"
    );
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""success":false"#));
}

#[tokio::test]
async fn test_apply_empty_config_removes_old_inbounds() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: path.clone(),
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
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    apply_empty_config(&client, &settings, RuntimeCore::Xray, "empty-v1")
        .await
        .expect("empty config applies");

    let rendered = fs::read_to_string(path).expect("config was written");
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(json["inbounds"][0]["tag"], "xrayc-api");
    assert_eq!(json["inbounds"][0]["protocol"], "dokodemo-door");
    assert_eq!(json["outbounds"][0]["protocol"], "blackhole");
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""config_version":"empty-v1""#));
    assert!(request.contains(r#""success":true"#));
}

#[tokio::test]
async fn test_traffic_refresh_applies_empty_config_immediately() {
    let (base_url, requests) = spawn_sequence_server(vec![
        r#"{"accepted":true,"desired_config_version":"quota-v1","config_status":{"required":true,"reason":"quota_exhausted"},"config":null}"#,
        r#"{"accepted":true}"#,
    ])
    .await;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: path.clone(),
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
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let outcome = report_traffic_snapshot(
        &client,
        &settings,
        vec![TrafficSnapshot {
            access_line_id: Some("line-a".to_owned()),
            xray_user_key: "user-1".to_owned(),
            uplink_bytes: 100,
            downlink_bytes: 200,
            captured_at_unix: 1_700_000_000,
        }],
        Some("applied-v0"),
        true,
    )
    .await
    .expect("traffic refresh applies");

    assert_eq!(outcome.applied_config_version.as_deref(), Some("quota-v1"));
    assert!(outcome.replace_active_config);
    assert!(outcome.active_config.is_none());
    let rendered = fs::read_to_string(path).expect("empty config was written");
    assert!(rendered.contains(r#""protocol": "blackhole""#));
    let requests = requests.await.expect("server task joins");
    assert!(requests[0].contains("POST /api/agent/access/traffic HTTP/1.1"));
    assert!(requests[0].contains(r#""applied_config_version":"applied-v0""#));
    assert!(requests[1].contains("POST /api/agent/access/config-result HTTP/1.1"));
    assert!(requests[1].contains(r#""config_version":"quota-v1""#));
    assert!(requests[1].contains(r#""success":true"#));
}

#[tokio::test]
async fn test_report_traffic_filters_snapshots_without_line_id() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: dir.path().join("config.json"),
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
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    report_traffic_snapshot(
        &client,
        &settings,
        vec![
            TrafficSnapshot {
                access_line_id: None,
                xray_user_key: "legacy-user".to_owned(),
                uplink_bytes: 10,
                downlink_bytes: 20,
                captured_at_unix: 1_700_000_000,
            },
            TrafficSnapshot {
                access_line_id: Some("line-a".to_owned()),
                xray_user_key: "user-1".to_owned(),
                uplink_bytes: 100,
                downlink_bytes: 200,
                captured_at_unix: 1_700_000_000,
            },
        ],
        None,
        true,
    )
    .await
    .expect("traffic report succeeds");

    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/traffic HTTP/1.1"));
    assert!(request.contains(r#""access_line_id":"line-a""#));
    assert!(request.contains(r#""xray_user_key":"user-1""#));
    assert!(!request.contains("legacy-user"));
}

#[tokio::test]
async fn test_control_plane_config_cache_restores_active_config_after_restart() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let settings = AgentSettings {
        node_id: "node-a".to_owned(),
        control_plane_url: base_url,
        node_token: "token".to_owned(),
        runtime_core: RuntimeCore::Xray,
        heartbeat_interval_seconds: 30,
        traffic_interval_seconds: 60,
        session_idle_seconds: 180,
        agent_state_path: dir.path().join("agent-state.json"),
        traffic_backlog_path: dir.path().join("traffic-backlog.json"),
        xray_access_log_path: dir.path().join("access.log"),
        xray_config_path: path,
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
        rate_limiter_plan_path: dir.path().join("limiter-plan.sh"),
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
        host_fs_root: "/".to_owned(),
    };
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let outcome = apply_control_plane_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        Some("v1".to_owned()),
        ConfigStatus {
            required: true,
            reason: Some("config_dirty".to_owned()),
        },
        Some(sample_config()),
        true,
    )
    .await
    .expect("control plane config applies");

    assert_eq!(outcome.applied_config_version.as_deref(), Some("v1"));
    assert!(outcome.replace_active_config);
    let cached = load_active_config_cache(&settings)
        .expect("cache loads")
        .expect("cache exists");
    assert_eq!(cached.config_version.as_deref(), Some("v1"));
    let active_config = cached.config.expect("active config cached");
    assert_eq!(active_config.access_lines[0].id, "line-a");
    assert_eq!(active_config.exit_endpoints[0].id, "exit-socks");
    let request = request.await.expect("server task joins");
    assert!(request.contains("POST /api/agent/access/config-result HTTP/1.1"));
}
