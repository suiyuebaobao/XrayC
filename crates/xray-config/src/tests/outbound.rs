//! 本模块测试出口端点编译逻辑。
//! 覆盖代理协议、认证字段、端口校验和传输安全参数。
//! 入站线路相关行为由入站测试模块负责。

use serde_json::{json, Map};

use super::common::assert_missing_exit_field;
use crate::outbound::compile_outbound;
use crate::*;

#[test]
fn test_compile_direct_exit_uses_freedom_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-direct".to_owned(),
        tag: "exit-direct".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Direct,
    };

    let outbound = compile_outbound(&endpoint).expect("direct exit compiles");

    assert_eq!(outbound["tag"], "exit-direct");
    assert_eq!(outbound["protocol"], "freedom");
    assert_eq!(outbound["settings"], json!({}));
}

#[test]
fn test_compile_socks_outbound_with_auth() {
    let endpoint = ExitEndpoint {
        id: "exit-socks".to_owned(),
        tag: "exit-socks".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port: 1080,
            username: Some("agent".to_owned()),
            password: Some("secret".to_owned()),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "socks");
    assert_eq!(
        outbound["settings"]["servers"][0]["users"][0]["user"],
        "agent"
    );
}

#[test]
fn test_compile_socks_outbound_without_auth() {
    let endpoint = ExitEndpoint {
        id: "exit-socks-no-auth".to_owned(),
        tag: "exit-socks-no-auth".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port: 1080,
            username: None,
            password: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "socks");
    assert_eq!(outbound["settings"]["servers"][0]["address"], "127.0.0.1");
    assert_eq!(outbound["settings"]["servers"][0]["users"], json!([]));
}

#[test]
fn test_compile_http_outbound_without_auth() {
    let endpoint = ExitEndpoint {
        id: "exit-http".to_owned(),
        tag: "exit-http".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Http {
            address: "127.0.0.1".to_owned(),
            port: 8080,
            username: None,
            password: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "http");
    assert_eq!(outbound["settings"]["servers"][0]["address"], "127.0.0.1");
    assert_eq!(outbound["settings"]["servers"][0]["users"], json!([]));
}

#[test]
fn test_compile_http_outbound_with_auth() {
    let endpoint = ExitEndpoint {
        id: "exit-http-auth".to_owned(),
        tag: "exit-http-auth".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Http {
            address: "127.0.0.1".to_owned(),
            port: 8080,
            username: Some("agent".to_owned()),
            password: Some("secret".to_owned()),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "http");
    assert_eq!(outbound["settings"]["servers"][0]["address"], "127.0.0.1");
    assert_eq!(outbound["settings"]["servers"][0]["port"], 8080);
    assert_eq!(
        outbound["settings"]["servers"][0]["users"][0],
        json!({"user": "agent", "pass": "secret"})
    );
}

#[test]
fn test_compile_socks_and_http_outbounds_reject_auth_pair_mismatch() {
    let socks = ExitEndpoint {
        id: "exit-socks-missing-password".to_owned(),
        tag: "exit-socks-missing-password".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port: 1080,
            username: Some("agent".to_owned()),
            password: None,
        },
    };
    assert_missing_exit_field(
        compile_outbound(&socks),
        "exit-socks-missing-password",
        "password",
    );

    let http = ExitEndpoint {
        id: "exit-http-missing-username".to_owned(),
        tag: "exit-http-missing-username".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Http {
            address: "127.0.0.1".to_owned(),
            port: 8080,
            username: None,
            password: Some("secret".to_owned()),
        },
    };
    assert_missing_exit_field(
        compile_outbound(&http),
        "exit-http-missing-username",
        "username",
    );
}

#[test]
fn test_compile_non_direct_outbounds_reject_zero_port() {
    let cases = [
        ExitEndpoint {
            id: "exit-socks-zero-port".to_owned(),
            tag: "exit-socks-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Socks {
                address: "127.0.0.1".to_owned(),
                port: 0,
                username: None,
                password: None,
            },
        },
        ExitEndpoint {
            id: "exit-http-zero-port".to_owned(),
            tag: "exit-http-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Http {
                address: "127.0.0.1".to_owned(),
                port: 0,
                username: None,
                password: None,
            },
        },
        ExitEndpoint {
            id: "exit-vless-zero-port".to_owned(),
            tag: "exit-vless-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Vless {
                address: "203.0.113.20".to_owned(),
                port: 0,
                uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
                security: Some("tls".to_owned()),
                flow: None,
                server_name: Some("vless.example.invalid".to_owned()),
                public_key: None,
                short_id: None,
                fingerprint: None,
                network: None,
                xhttp_path: None,
                xhttp_host: None,
                xhttp_mode: None,
                udp_packet_encoding: None,
            },
        },
        ExitEndpoint {
            id: "exit-trojan-zero-port".to_owned(),
            tag: "exit-trojan-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Trojan {
                address: "203.0.113.30".to_owned(),
                port: 0,
                password: "trojan-secret".to_owned(),
                security: None,
                server_name: Some("trojan.example.invalid".to_owned()),
            },
        },
        ExitEndpoint {
            id: "exit-ss-zero-port".to_owned(),
            tag: "exit-ss-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Shadowsocks {
                address: "203.0.113.40".to_owned(),
                port: 0,
                method: "2022-blake3-aes-128-gcm".to_owned(),
                password: "ss-secret".to_owned(),
            },
        },
        ExitEndpoint {
            id: "exit-hy2-zero-port".to_owned(),
            tag: "exit-hy2-zero-port".to_owned(),
            sockopt_mark: None,
            protocol: ExitProtocol::Hysteria2 {
                address: "203.0.113.10".to_owned(),
                port: 0,
                password: "hy2-secret".to_owned(),
                server_name: Some("hy2.example.invalid".to_owned()),
                allow_insecure: None,
            },
        },
    ];

    for endpoint in cases {
        assert_missing_exit_field(compile_outbound(&endpoint), &endpoint.id, "port");
    }
}

#[test]
fn test_compile_vless_tls_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-tls".to_owned(),
        tag: "exit-vless-tls".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("tls".to_owned()),
            flow: None,
            server_name: Some("vless.example.invalid".to_owned()),
            public_key: None,
            short_id: None,
            fingerprint: Some("chrome".to_owned()),
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");
    let user = outbound["settings"]["vnext"][0]["users"][0]
        .as_object()
        .expect("vless user is an object");

    assert_eq!(outbound["protocol"], "vless");
    assert_eq!(outbound["streamSettings"]["security"], "tls");
    assert_eq!(
        outbound["streamSettings"]["tlsSettings"]["serverName"],
        "vless.example.invalid"
    );
    assert_eq!(
        outbound["streamSettings"]["tlsSettings"]["fingerprint"],
        "chrome"
    );
    assert!(!user.contains_key("flow"));
}

#[test]
fn test_compile_vless_none_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-none".to_owned(),
        tag: "exit-vless-none".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 80,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("none".to_owned()),
            flow: None,
            server_name: None,
            public_key: None,
            short_id: None,
            fingerprint: None,
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "vless");
    assert!(outbound["streamSettings"]
        .as_object()
        .is_some_and(Map::is_empty));
}

#[test]
fn test_compile_vless_reality_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-reality".to_owned(),
        tag: "exit-vless-reality".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.20".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: Some("xtls-rprx-vision".to_owned()),
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "vless");
    assert_eq!(
        outbound["settings"]["vnext"][0]["users"][0]["flow"],
        "xtls-rprx-vision"
    );
    assert_eq!(outbound["streamSettings"]["security"], "reality");
    assert_eq!(
        outbound["streamSettings"]["realitySettings"]["publicKey"],
        "reality-public-key"
    );
    assert_eq!(
        outbound["streamSettings"]["realitySettings"]["shortId"],
        "abcd1234"
    );
}

#[test]
fn test_compile_vless_outbound_rejects_invalid_security() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-invalid-security".to_owned(),
        tag: "exit-vless-invalid-security".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("invalid-security".to_owned()),
            flow: None,
            server_name: None,
            public_key: None,
            short_id: None,
            fingerprint: None,
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    assert!(matches!(
        compile_outbound(&endpoint),
        Err(XrayConfigError::UnsupportedExitProtocol(protocol))
            if protocol == "vless security invalid-security"
    ));
}

#[test]
fn test_compile_vless_reality_outbound_requires_reality_fields() {
    let missing_server_name = ExitEndpoint {
        id: "exit-vless-missing-server-name".to_owned(),
        tag: "exit-vless-missing-server-name".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.22".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: None,
            public_key: Some("reality-public-key".to_owned()),
            short_id: None,
            fingerprint: None,
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };
    assert_missing_exit_field(
        compile_outbound(&missing_server_name),
        "exit-vless-missing-server-name",
        "server_name",
    );

    let missing_public_key = ExitEndpoint {
        id: "exit-vless-missing-reality".to_owned(),
        tag: "exit-vless-missing-reality".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.22".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: None,
            short_id: None,
            fingerprint: None,
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    assert_missing_exit_field(
        compile_outbound(&missing_public_key),
        "exit-vless-missing-reality",
        "public_key",
    );
}

#[test]
fn test_compile_trojan_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-trojan".to_owned(),
        tag: "exit-trojan".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Trojan {
            address: "203.0.113.30".to_owned(),
            port: 443,
            password: "trojan-secret".to_owned(),
            security: None,
            server_name: Some("trojan.example.invalid".to_owned()),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "trojan");
    assert_eq!(
        outbound["settings"]["servers"][0]["address"],
        "203.0.113.30"
    );
    assert_eq!(
        outbound["settings"]["servers"][0]["password"],
        "trojan-secret"
    );
    assert_eq!(outbound["streamSettings"]["security"], "tls");
    assert_eq!(
        outbound["streamSettings"]["tlsSettings"]["serverName"],
        "trojan.example.invalid"
    );
}

#[test]
fn test_compile_trojan_none_outbound_is_rejected() {
    let endpoint = ExitEndpoint {
        id: "exit-trojan-none".to_owned(),
        tag: "exit-trojan-none".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Trojan {
            address: "203.0.113.30".to_owned(),
            port: 443,
            password: "trojan-secret".to_owned(),
            security: Some("none".to_owned()),
            server_name: None,
        },
    };

    assert!(compile_outbound(&endpoint).is_err());
}

#[test]
fn test_compile_trojan_tls_outbound_requires_server_name() {
    let endpoint = ExitEndpoint {
        id: "exit-trojan-missing-sni".to_owned(),
        tag: "exit-trojan-missing-sni".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Trojan {
            address: "203.0.113.30".to_owned(),
            port: 443,
            password: "trojan-secret".to_owned(),
            security: Some("tls".to_owned()),
            server_name: None,
        },
    };

    assert_missing_exit_field(
        compile_outbound(&endpoint),
        "exit-trojan-missing-sni",
        "server_name",
    );
}

#[test]
fn test_compile_trojan_outbound_requires_password() {
    let endpoint = ExitEndpoint {
        id: "exit-trojan-missing-password".to_owned(),
        tag: "exit-trojan-missing-password".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Trojan {
            address: "203.0.113.30".to_owned(),
            port: 443,
            password: " ".to_owned(),
            security: None,
            server_name: Some("trojan.example.invalid".to_owned()),
        },
    };

    assert_missing_exit_field(
        compile_outbound(&endpoint),
        "exit-trojan-missing-password",
        "password",
    );
}
