//! 本模块测试访问入口编译逻辑。
//! 覆盖传输层、安全层、多用户统计身份和空用户校验。
//! 出口协议相关行为由出站测试模块负责。

use super::common::sample_access_config;
use crate::inbound::{compile_inbound, ACCESS_INBOUND_LISTEN};
use crate::*;
use base64::Engine;
use sha2::{Digest, Sha256};

#[test]
fn test_vless_xhttp_inbound_keeps_selected_transport() {
    let mut config = sample_access_config();
    config.access_lines[0].listen_host = "public-access.example.test".to_owned();
    config.access_lines[0].transport = "xhttp".to_owned();
    config.access_lines[0].xhttp_path = "/ray".to_owned();
    config.access_lines[0].xhttp_host = "edge.example.test".to_owned();
    config.access_lines[0].xhttp_mode = "stream-one".to_owned();

    let actual = compile_xray_config(&config).expect("config compiles");

    assert_eq!(actual["inbounds"][0]["listen"], ACCESS_INBOUND_LISTEN);
    assert_eq!(actual["inbounds"][0]["streamSettings"]["network"], "xhttp");
    assert_eq!(
        actual["inbounds"][0]["streamSettings"]["xhttpSettings"]["path"],
        "/ray"
    );
    assert_eq!(
        actual["inbounds"][0]["streamSettings"]["xhttpSettings"]["host"],
        "edge.example.test"
    );
    assert_eq!(
        actual["inbounds"][0]["streamSettings"]["xhttpSettings"]["mode"],
        "stream-one"
    );
}

#[test]
fn test_compile_vless_reality_inbound_stream_settings() {
    let mut config = sample_access_config();
    config.access_lines[0].inbound_security = Some("reality".to_owned());
    config.access_lines[0].server_name = Some("www.example.test".to_owned());
    config.access_lines[0].reality_dest = Some("www.example.test:443".to_owned());
    config.access_lines[0].reality_private_key = Some("reality-private-key".to_owned());
    config.access_lines[0].reality_short_ids = vec!["abcd1234".to_owned()];
    config.access_lines[0].protocol = AccessProtocol::Vless {
        flow: Some("xtls-rprx-vision".to_owned()),
        decryption: Some("none".to_owned()),
    };

    let actual = compile_xray_config(&config).expect("config compiles");

    let inbound = &actual["inbounds"][0];
    assert_eq!(inbound["streamSettings"]["network"], "tcp");
    assert_eq!(inbound["streamSettings"]["security"], "reality");
    assert_eq!(
        inbound["streamSettings"]["realitySettings"]["dest"],
        "www.example.test:443"
    );
    assert_eq!(
        inbound["streamSettings"]["realitySettings"]["serverNames"][0],
        "www.example.test"
    );
    assert_eq!(
        inbound["streamSettings"]["realitySettings"]["privateKey"],
        "reality-private-key"
    );
    assert_eq!(
        inbound["streamSettings"]["realitySettings"]["shortIds"][0],
        "abcd1234"
    );
    assert_eq!(
        inbound["settings"]["clients"][0]["flow"],
        "xtls-rprx-vision"
    );
}

#[test]
fn test_compile_vless_reality_grpc_inbound_keeps_grpc_transport() {
    // 官方口径允许 Reality+gRPC 入站：transport=grpc 不应被拒，且 network 落 grpc。
    let mut config = sample_access_config();
    config.access_lines[0].transport = "grpc".to_owned();
    config.access_lines[0].xhttp_path = "inbound-svc".to_owned();
    config.access_lines[0].inbound_security = Some("reality".to_owned());
    config.access_lines[0].server_name = Some("www.example.test".to_owned());
    config.access_lines[0].reality_dest = Some("www.example.test:443".to_owned());
    config.access_lines[0].reality_private_key = Some("reality-private-key".to_owned());
    config.access_lines[0].reality_short_ids = vec!["abcd1234".to_owned()];

    let actual = compile_xray_config(&config).expect("Reality+gRPC inbound compiles");

    let inbound = &actual["inbounds"][0];
    assert_eq!(inbound["streamSettings"]["network"], "grpc");
    assert_eq!(
        inbound["streamSettings"]["grpcSettings"]["serviceName"],
        "inbound-svc"
    );
    assert_eq!(inbound["streamSettings"]["security"], "reality");
}

#[test]
fn test_compile_vless_reality_ws_inbound_is_rejected() {
    // 官方口径拒绝 Reality+WS：入站必须与出站对称报错，不生成非法 streamSettings。
    let mut config = sample_access_config();
    config.access_lines[0].transport = "ws".to_owned();
    config.access_lines[0].inbound_security = Some("reality".to_owned());
    config.access_lines[0].server_name = Some("www.example.test".to_owned());
    config.access_lines[0].reality_private_key = Some("reality-private-key".to_owned());

    let error = compile_xray_config(&config).expect_err("Reality+WS inbound must be rejected");

    assert!(matches!(
        error,
        XrayConfigError::UnsupportedAccessProtocol(message)
            if message.contains("Reality") && message.contains("line-a")
    ));
}

#[test]
fn test_compile_trojan_tls_inbound_uses_certificate_files() {
    let mut config = sample_access_config();
    config.access_lines[0].inbound_security = Some("tls".to_owned());
    config.access_lines[0].server_name = Some("trojan.example.test".to_owned());
    config.access_lines[0].tls_certificate_file =
        Some("/etc/letsencrypt/live/trojan.example.test/fullchain.pem".to_owned());
    config.access_lines[0].tls_key_file =
        Some("/etc/letsencrypt/live/trojan.example.test/privkey.pem".to_owned());
    config.access_lines[0].protocol = AccessProtocol::Trojan;

    let actual = compile_xray_config(&config).expect("config compiles");

    let inbound = &actual["inbounds"][0];
    assert_eq!(inbound["protocol"], "trojan");
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
fn test_compile_vless_tls_websocket_inbound_uses_cloudflare_stream_settings() {
    let mut config = sample_access_config();
    config.access_lines[0].listen_port = 443;
    config.access_lines[0].transport = "ws".to_owned();
    config.access_lines[0].server_name = Some("cdn-entry.example.test".to_owned());
    config.access_lines[0].xhttp_path = "/vless-ws".to_owned();
    config.access_lines[0].xhttp_host = "cdn-entry.example.test".to_owned();
    config.access_lines[0].inbound_security = Some("tls".to_owned());
    config.access_lines[0].tls_certificate_file =
        Some("/etc/letsencrypt/live/cdn-entry.example.test/fullchain.pem".to_owned());
    config.access_lines[0].tls_key_file =
        Some("/etc/letsencrypt/live/cdn-entry.example.test/privkey.pem".to_owned());
    config.access_lines[0].protocol = AccessProtocol::Vless {
        flow: None,
        decryption: Some("none".to_owned()),
    };

    let actual = compile_xray_config(&config).expect("config compiles");

    let stream_settings = &actual["inbounds"][0]["streamSettings"];
    assert_eq!(stream_settings["network"], "ws");
    assert_eq!(stream_settings["security"], "tls");
    assert_eq!(
        stream_settings["tlsSettings"]["certificates"][0]["certificateFile"],
        "/etc/letsencrypt/live/cdn-entry.example.test/fullchain.pem"
    );
    assert_eq!(
        stream_settings["tlsSettings"]["certificates"][0]["keyFile"],
        "/etc/letsencrypt/live/cdn-entry.example.test/privkey.pem"
    );
    assert_eq!(stream_settings["wsSettings"]["path"], "/vless-ws");
    assert_eq!(
        stream_settings["wsSettings"]["headers"]["Host"],
        "cdn-entry.example.test"
    );
}

#[test]
fn test_compile_hysteria2_tls_inbound_uses_user_clients() {
    let mut config = sample_access_config();
    config.access_lines[0].transport = "hysteria".to_owned();
    config.access_lines[0].inbound_security = Some("tls".to_owned());
    config.access_lines[0].server_name = Some("hy2.example.test".to_owned());
    config.access_lines[0].tls_certificate_file =
        Some("/etc/letsencrypt/live/hy2.example.test/fullchain.pem".to_owned());
    config.access_lines[0].tls_key_file =
        Some("/etc/letsencrypt/live/hy2.example.test/privkey.pem".to_owned());
    config.access_lines[0].protocol = AccessProtocol::Hysteria2;

    let actual = compile_xray_config(&config).expect("config compiles");

    let inbound = &actual["inbounds"][0];
    assert_eq!(inbound["protocol"], "hysteria");
    assert_eq!(inbound["streamSettings"]["network"], "hysteria");
    assert_eq!(inbound["streamSettings"]["security"], "tls");
    assert_eq!(
        inbound["streamSettings"]["tlsSettings"]["serverName"],
        "hy2.example.test"
    );
    assert_eq!(inbound["streamSettings"]["tlsSettings"]["alpn"][0], "h3");
    assert_eq!(
        inbound["settings"]["clients"][0]["auth"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert!(inbound["settings"]["clients"][0]["password"].is_null());
    assert_eq!(inbound["settings"]["clients"][0]["email"], "user-1@xrayc");
}

#[test]
fn test_compile_shadowsocks_inbound_uses_multi_user_stats_identity() {
    let mut config = sample_access_config();
    config.access_lines[0].protocol = AccessProtocol::Shadowsocks {
        method: "aes-256-gcm".to_owned(),
        server_password: "line-root-placeholder".to_owned(),
        network: "tcp,udp".to_owned(),
    };
    config.access_lines[0].users = vec![
        AccessUser {
            xray_user_key: "user-1".to_owned(),
            credential: "user-secret-1".to_owned(),
            email: "stats-user-1@xrayc".to_owned(),
            level: 0,
        },
        AccessUser {
            xray_user_key: "user-2".to_owned(),
            credential: "user-secret-2".to_owned(),
            email: "stats-user-2@xrayc".to_owned(),
            level: 0,
        },
    ];

    let actual = compile_xray_config(&config).expect("config compiles");
    let inbound = &actual["inbounds"][0];

    assert_eq!(inbound["protocol"], "shadowsocks");
    assert_eq!(inbound["settings"]["method"], "aes-256-gcm");
    assert_eq!(inbound["settings"]["password"], "line-root-placeholder");
    assert_eq!(inbound["settings"]["network"], "tcp,udp");
    assert_eq!(
        inbound["settings"]["clients"][0]["password"],
        "user-secret-1"
    );
    assert_eq!(
        inbound["settings"]["clients"][0]["email"],
        "stats-user-1@xrayc"
    );
    assert_eq!(inbound["settings"]["clients"][0]["method"], "aes-256-gcm");
    assert_eq!(
        inbound["settings"]["clients"][1]["password"],
        "user-secret-2"
    );
    assert_eq!(
        inbound["settings"]["clients"][1]["email"],
        "stats-user-2@xrayc"
    );
}

#[test]
fn test_compile_shadowsocks_2022_derives_base64_user_key() {
    let mut config = sample_access_config();
    config.access_lines[0].protocol = AccessProtocol::Shadowsocks {
        method: "2022-blake3-aes-128-gcm".to_owned(),
        server_password: "line-root-placeholder".to_owned(),
        network: "tcp,udp".to_owned(),
    };
    config.access_lines[0].users[0].credential = "user-secret-1".to_owned();

    let actual = compile_xray_config(&config).expect("config compiles");
    let inbound = &actual["inbounds"][0];
    let expected_key =
        base64::engine::general_purpose::STANDARD.encode(&Sha256::digest(b"user-secret-1")[..16]);

    assert_eq!(inbound["settings"]["method"], "2022-blake3-aes-128-gcm");
    assert_eq!(inbound["settings"]["clients"][0]["password"], expected_key);
    assert!(inbound["settings"]["clients"][0]["method"].is_null());
}

#[test]
fn test_reject_empty_access_users() {
    let line = AccessLine {
        id: "line-empty".to_owned(),
        source_line_id: String::new(),
        runtime_core: "xray".to_owned(),
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
        protocol: AccessProtocol::Trojan,
        users: vec![],
        default_exit_tag: "exit-socks".to_owned(),
    };

    let error = compile_inbound(&line).expect_err("empty users should fail");

    assert!(matches!(error, XrayConfigError::EmptyUsers(id) if id == "line-empty"));
}
