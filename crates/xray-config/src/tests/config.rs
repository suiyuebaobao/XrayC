//! 本模块测试完整配置编译和统计用户标识。
//! 这些用例覆盖主入口对入站、出站、路由和统计开关的组装行为。
//! 具体协议细节在入站和出站测试模块中验证。

use serde_json::Value;

use super::common::sample_access_config;
use crate::*;

#[test]
fn test_compile_vless_socks_xray_config() {
    let config = AccessConfig {
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
                decryption: Some("none".to_owned()),
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
            id: "exit-a".to_owned(),
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
        routing_rules: vec![RoutingRule {
            inbound_tag: Some("access-line-a".to_owned()),
            user_email: Some("user-1@xrayc".to_owned()),
            outbound_tag: "exit-socks".to_owned(),
        }],
        rate_limits: Vec::new(),
    };

    let actual = compile_xray_config(&config).expect("config compiles");

    assert_eq!(actual["inbounds"][0]["protocol"], "vless");
    assert_eq!(actual["inbounds"][0]["tag"], "access-line-a");
    assert_eq!(actual["inbounds"][1]["protocol"], "dokodemo-door");
    assert_eq!(actual["inbounds"][1]["tag"], XRAY_STATS_API_TAG);
    assert_eq!(actual["api"]["services"][0], "StatsService");
    assert_eq!(
        actual["inbounds"][0]["settings"]["clients"][0]["id"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(actual["outbounds"][0]["protocol"], "socks");
    assert_eq!(
        actual["routing"]["rules"][0]["outboundTag"],
        XRAY_STATS_API_TAG
    );
    assert_eq!(actual["routing"]["rules"][1]["user"][0], "user-1@xrayc");
    assert_eq!(
        actual["policy"]["levels"]["0"]["statsUserUplink"],
        Value::Bool(true)
    );
}

#[test]
fn test_compile_outbound_applies_sockopt_mark() {
    let mut config = sample_access_config();
    config.exit_endpoints[0].sockopt_mark = Some(65_537);

    let actual = compile_xray_config(&config).expect("config compiles");

    assert_eq!(
        actual["outbounds"][0]["streamSettings"]["sockopt"]["mark"],
        65_537
    );
}

#[test]
fn test_stats_user_email_roundtrip() {
    let email = stats_user_email("line-a", "u-1@xrayc.local");
    assert!(email.starts_with("xrayc-line-"));

    let parsed = parse_stats_user_email(&email).expect("stats email parses");

    assert_eq!(parsed.0, "line-a");
    assert_eq!(parsed.1, "u-1@xrayc.local");

    let legacy = parse_stats_user_email("rp-line-line-a--u-1@xrayc.local").expect("legacy parses");
    assert_eq!(legacy.0, "line-a");
    assert_eq!(legacy.1, "u-1@xrayc.local");
}

#[test]
fn test_compile_omits_stats_api_config_when_stats_disabled() {
    let mut config = sample_access_config();
    config.stats_enabled = false;

    let actual = compile_xray_config(&config).expect("config compiles");

    assert!(actual.get("api").is_none());
    assert!(actual.get("stats").is_none());
    assert!(actual["inbounds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|inbound| inbound["tag"] != XRAY_STATS_API_TAG));
    assert!(actual["routing"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .all(|rule| rule["outboundTag"] != XRAY_STATS_API_TAG));
}

#[test]
fn test_compile_local_exit_service_without_access_lines() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![LocalExitService {
        id: "local-socks".to_owned(),
        tag: "local-exit-service-local-socks".to_owned(),
        listen_port: 38081,
        protocol: LocalExitProtocol::Socks,
        network: "tcp".to_owned(),
        username: "local-user".to_owned(),
        password: "local-pass".to_owned(),
        uuid: String::new(),
        method: String::new(),
        security: None,
        server_name: None,
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: None,
        tls_key_file: None,
    }];

    let actual = compile_xray_config(&config).expect("local service config compiles");

    assert_eq!(actual["inbounds"][0]["protocol"], "socks");
    assert_eq!(actual["inbounds"][0]["port"], 38081);
    assert_eq!(
        actual["inbounds"][0]["settings"]["accounts"][0]["user"],
        "local-user"
    );
    assert_eq!(actual["outbounds"][0]["protocol"], "freedom");
    assert_eq!(
        actual["routing"]["rules"][1]["inboundTag"][0],
        "local-exit-service-local-socks"
    );
}

#[test]
fn test_compile_hysteria2_local_exit_service_uses_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![hysteria2_local_exit_service(
        Some("/etc/letsencrypt/live/hy2.example.test/fullchain.pem"),
        Some("/etc/letsencrypt/live/hy2.example.test/privkey.pem"),
    )];

    let actual = compile_xray_config(&config).expect("hy2 local service config compiles");
    let inbound = &actual["inbounds"][0];

    assert_eq!(inbound["protocol"], "hysteria");
    assert_eq!(inbound["streamSettings"]["network"], "hysteria");
    assert_eq!(inbound["streamSettings"]["security"], "tls");
    assert_eq!(inbound["streamSettings"]["hysteriaSettings"]["version"], 2);
    assert_eq!(inbound["streamSettings"]["tlsSettings"]["alpn"][0], "h3");
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["certificateFile"],
        "/etc/letsencrypt/live/hy2.example.test/fullchain.pem"
    );
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["keyFile"],
        "/etc/letsencrypt/live/hy2.example.test/privkey.pem"
    );
}

#[test]
fn test_compile_trojan_local_exit_service_uses_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![trojan_local_exit_service(
        Some("/etc/letsencrypt/live/trojan.example.test/fullchain.pem"),
        Some("/etc/letsencrypt/live/trojan.example.test/privkey.pem"),
    )];

    let actual = compile_xray_config(&config).expect("trojan local service config compiles");
    let inbound = &actual["inbounds"][0];

    assert_eq!(inbound["protocol"], "trojan");
    assert_eq!(inbound["streamSettings"]["network"], "tcp");
    assert_eq!(inbound["streamSettings"]["security"], "tls");
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["serverName"],
        "trojan.example.test"
    );
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["certificateFile"],
        "/etc/letsencrypt/live/trojan.example.test/fullchain.pem"
    );
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["keyFile"],
        "/etc/letsencrypt/live/trojan.example.test/privkey.pem"
    );
}

#[test]
fn test_compile_hysteria2_local_exit_service_requires_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![hysteria2_local_exit_service(None, None)];

    let error = compile_xray_config(&config).expect_err("missing hy2 certificate should fail");

    assert!(matches!(
        error,
        XrayConfigError::UnsupportedAccessProtocol(message)
            if message == "local-hy2 missing tls_certificate_file"
    ));
}

#[test]
fn test_compile_trojan_local_exit_service_requires_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![trojan_local_exit_service(None, None)];

    let error = compile_xray_config(&config).expect_err("missing trojan certificate should fail");

    assert!(matches!(
        error,
        XrayConfigError::UnsupportedAccessProtocol(message)
            if message == "local-trojan missing tls_certificate_file"
    ));
}

#[test]
fn test_compile_vless_tls_local_exit_service_uses_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![vless_tls_local_exit_service(
        Some("/etc/letsencrypt/live/vless.example.test/fullchain.pem"),
        Some("/etc/letsencrypt/live/vless.example.test/privkey.pem"),
    )];

    let actual = compile_xray_config(&config).expect("vless-tls local service config compiles");
    let inbound = &actual["inbounds"][0];

    assert_eq!(inbound["protocol"], "vless");
    assert_eq!(inbound["streamSettings"]["security"], "tls");
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["serverName"],
        "vless-tls.example.test"
    );
    // 回归:VLESS 本机出口 TLS 必须带服务器证书,否则 Xray 握手失败、该出口 100% 不可用
    // (此前漏写 certificates——真机压测发现的产品 bug)。
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["certificateFile"],
        "/etc/letsencrypt/live/vless.example.test/fullchain.pem"
    );
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["certificates"][0]["keyFile"],
        "/etc/letsencrypt/live/vless.example.test/privkey.pem"
    );
}

#[test]
fn test_compile_vless_tls_local_exit_service_requires_tls_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines.clear();
    config.exit_endpoints.clear();
    config.routing_rules.clear();
    config.local_exit_services = vec![vless_tls_local_exit_service(None, None)];

    let error =
        compile_xray_config(&config).expect_err("missing vless-tls certificate should fail");

    assert!(matches!(
        error,
        XrayConfigError::UnsupportedAccessProtocol(message)
            if message == "local-vless-tls missing tls_certificate_file"
    ));
}

fn vless_tls_local_exit_service(
    tls_certificate_file: Option<&str>,
    tls_key_file: Option<&str>,
) -> LocalExitService {
    LocalExitService {
        id: "local-vless-tls".to_owned(),
        tag: "local-exit-service-local-vless-tls".to_owned(),
        listen_port: 1447,
        protocol: LocalExitProtocol::Vless,
        network: "tcp".to_owned(),
        username: String::new(),
        password: String::new(),
        uuid: "11111111-2222-3333-4444-555555555555".to_owned(),
        method: String::new(),
        security: Some("tls".to_owned()),
        server_name: Some("vless-tls.example.test".to_owned()),
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: tls_certificate_file.map(ToOwned::to_owned),
        tls_key_file: tls_key_file.map(ToOwned::to_owned),
    }
}

fn hysteria2_local_exit_service(
    tls_certificate_file: Option<&str>,
    tls_key_file: Option<&str>,
) -> LocalExitService {
    LocalExitService {
        id: "local-hy2".to_owned(),
        tag: "local-exit-service-local-hy2".to_owned(),
        listen_port: 1444,
        protocol: LocalExitProtocol::Hysteria2,
        network: String::new(),
        username: String::new(),
        password: ["hy2", "local", "pass"].join("-"),
        uuid: String::new(),
        method: String::new(),
        security: None,
        server_name: None,
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: tls_certificate_file.map(ToOwned::to_owned),
        tls_key_file: tls_key_file.map(ToOwned::to_owned),
    }
}

fn trojan_local_exit_service(
    tls_certificate_file: Option<&str>,
    tls_key_file: Option<&str>,
) -> LocalExitService {
    LocalExitService {
        id: "local-trojan".to_owned(),
        tag: "local-exit-service-local-trojan".to_owned(),
        listen_port: 1446,
        protocol: LocalExitProtocol::Trojan,
        network: "tcp".to_owned(),
        username: String::new(),
        password: ["trojan", "local", "pass"].join("-"),
        uuid: String::new(),
        method: String::new(),
        security: Some("tls".to_owned()),
        server_name: Some("trojan.example.test".to_owned()),
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: tls_certificate_file.map(ToOwned::to_owned),
        tls_key_file: tls_key_file.map(ToOwned::to_owned),
    }
}
