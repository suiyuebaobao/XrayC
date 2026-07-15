//! 本模块测试两类补充协议的出站编译逻辑。
//! 这些用例从主要出站测试拆出，避免单个测试文件继续膨胀。
//! 基础代理和传输安全用例保留在出站测试模块。

use super::common::assert_missing_exit_field;
use crate::outbound::compile_outbound;
use crate::*;

#[test]
fn test_compile_vless_xhttp_outbound_stream_settings() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-xhttp".to_owned(),
        tag: "exit-vless-xhttp".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: Some("xhttp".to_owned()),
            xhttp_path: Some("/xrayc-upstream".to_owned()),
            xhttp_host: Some("edge.example.test".to_owned()),
            xhttp_mode: Some("packet-up".to_owned()),
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["streamSettings"]["network"], "xhttp");
    assert_eq!(
        outbound["streamSettings"]["xhttpSettings"]["path"],
        "/xrayc-upstream"
    );
    assert_eq!(
        outbound["streamSettings"]["xhttpSettings"]["host"],
        "edge.example.test"
    );
    assert_eq!(
        outbound["streamSettings"]["xhttpSettings"]["mode"],
        "packet-up"
    );
    assert_eq!(outbound["streamSettings"]["security"], "reality");
}

#[test]
fn test_compile_vless_xudp_outbound_enables_mux() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-xudp".to_owned(),
        tag: "exit-vless-xudp".to_owned(),
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
            udp_packet_encoding: Some("xudp".to_owned()),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["mux"]["enabled"], true);
    assert_eq!(outbound["mux"]["xudpConcurrency"], 16);
}

#[test]
fn test_compile_vless_reality_grpc_outbound_stream_settings() {
    // 官方口径允许 Reality+gRPC：既要正确落 network=grpc，也不应被传输层校验拒绝。
    let endpoint = ExitEndpoint {
        id: "exit-vless-reality-grpc".to_owned(),
        tag: "exit-vless-reality-grpc".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: Some("grpc".to_owned()),
            xhttp_path: Some("upstream-svc".to_owned()),
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("Reality+gRPC outbound compiles");

    assert_eq!(outbound["streamSettings"]["network"], "grpc");
    assert_eq!(
        outbound["streamSettings"]["grpcSettings"]["serviceName"],
        "upstream-svc"
    );
    assert_eq!(outbound["streamSettings"]["security"], "reality");
}

#[test]
fn test_compile_vless_reality_grpc_outbound_defaults_service_name() {
    // serviceName 缺省时回落到默认 "xrayc"，与入站 gRPC 口径一致。
    let endpoint = ExitEndpoint {
        id: "exit-vless-reality-grpc-default".to_owned(),
        tag: "exit-vless-reality-grpc-default".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: Some("grpc".to_owned()),
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("Reality+gRPC outbound compiles");

    assert_eq!(outbound["streamSettings"]["network"], "grpc");
    assert_eq!(
        outbound["streamSettings"]["grpcSettings"]["serviceName"],
        "xrayc"
    );
}

#[test]
fn test_compile_vless_ws_outbound_stream_settings() {
    // 非 Reality 的 VLESS+WS 出口：落 network=ws，path/Host 复用 xhttp 字段。
    let endpoint = ExitEndpoint {
        id: "exit-vless-ws".to_owned(),
        tag: "exit-vless-ws".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("tls".to_owned()),
            flow: None,
            server_name: Some("cdn.example.test".to_owned()),
            public_key: None,
            short_id: None,
            fingerprint: Some("chrome".to_owned()),
            network: Some("ws".to_owned()),
            xhttp_path: Some("/vless-ws".to_owned()),
            xhttp_host: Some("cdn.example.test".to_owned()),
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("VLESS+WS outbound compiles");

    assert_eq!(outbound["streamSettings"]["network"], "ws");
    assert_eq!(
        outbound["streamSettings"]["wsSettings"]["path"],
        "/vless-ws"
    );
    assert_eq!(
        outbound["streamSettings"]["wsSettings"]["headers"]["Host"],
        "cdn.example.test"
    );
    assert_eq!(outbound["streamSettings"]["security"], "tls");
}

#[test]
fn test_compile_vless_reality_outbound_rejects_ws() {
    // 官方口径拒绝 Reality+WS：传输层校验必须报错，不能静默生成非法配置。
    let endpoint = ExitEndpoint {
        id: "exit-vless-reality-ws".to_owned(),
        tag: "exit-vless-reality-ws".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: Some("ws".to_owned()),
            xhttp_path: Some("/vless-ws".to_owned()),
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: None,
        },
    };

    let err = compile_outbound(&endpoint).expect_err("Reality must reject WS");

    assert!(matches!(
        err,
        XrayConfigError::UnsupportedExitProtocol(protocol)
            if protocol.contains("Reality") && protocol.contains("exit-vless-reality-ws")
    ));
}

#[test]
fn test_compile_vless_reality_outbound_rejects_xudp() {
    let endpoint = ExitEndpoint {
        id: "exit-vless-reality-xudp".to_owned(),
        tag: "exit-vless-reality-xudp".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Vless {
            address: "203.0.113.21".to_owned(),
            port: 443,
            uuid: "5dd7c58d-2ae0-478e-bba9-80354e17e107".to_owned(),
            security: Some("reality".to_owned()),
            flow: None,
            server_name: Some("www.example.com".to_owned()),
            public_key: Some("reality-public-key".to_owned()),
            short_id: Some("abcd1234".to_owned()),
            fingerprint: Some("chrome".to_owned()),
            network: None,
            xhttp_path: None,
            xhttp_host: None,
            xhttp_mode: None,
            udp_packet_encoding: Some("xudp".to_owned()),
        },
    };

    let err = compile_outbound(&endpoint).expect_err("Reality must reject XUDP");

    assert!(format!("{err:?}").contains("Reality"));
}

#[test]
fn test_compile_shadowsocks_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-ss".to_owned(),
        tag: "exit-ss".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Shadowsocks {
            address: "203.0.113.40".to_owned(),
            port: 8388,
            method: "2022-blake3-aes-128-gcm".to_owned(),
            password: "ss-secret".to_owned(),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "shadowsocks");
    assert_eq!(outbound["settings"]["address"], "203.0.113.40");
    assert_eq!(outbound["settings"]["port"], 8388);
    assert_eq!(outbound["settings"]["method"], "2022-blake3-aes-128-gcm");
    assert_eq!(outbound["settings"]["password"], "ss-secret");
    assert!(outbound["settings"]["uot"].is_null());
    assert!(outbound["settings"]["UoTVersion"].is_null());
}

#[test]
fn test_compile_shadowsocks_outbound_requires_method_and_password() {
    let missing_method = ExitEndpoint {
        id: "exit-ss-missing-method".to_owned(),
        tag: "exit-ss-missing-method".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Shadowsocks {
            address: "203.0.113.40".to_owned(),
            port: 8388,
            method: String::new(),
            password: "ss-secret".to_owned(),
        },
    };
    assert_missing_exit_field(
        compile_outbound(&missing_method),
        "exit-ss-missing-method",
        "method",
    );

    let missing_password = ExitEndpoint {
        id: "exit-ss-missing-password".to_owned(),
        tag: "exit-ss-missing-password".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Shadowsocks {
            address: "203.0.113.40".to_owned(),
            port: 8388,
            method: "2022-blake3-aes-128-gcm".to_owned(),
            password: String::new(),
        },
    };
    assert_missing_exit_field(
        compile_outbound(&missing_password),
        "exit-ss-missing-password",
        "password",
    );
}

#[test]
fn test_compile_hysteria2_outbound() {
    let endpoint = ExitEndpoint {
        id: "exit-hy2".to_owned(),
        tag: "exit-hy2".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Hysteria2 {
            address: "203.0.113.10".to_owned(),
            port: 443,
            password: "secret".to_owned(),
            server_name: Some("edge.example.invalid".to_owned()),
            allow_insecure: None,
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "hysteria");
    assert_eq!(outbound["settings"]["version"], 2);
    assert_eq!(outbound["settings"]["address"], "203.0.113.10");
    assert_eq!(outbound["settings"]["port"], 443);
    assert_eq!(outbound["streamSettings"]["network"], "hysteria");
    assert_eq!(outbound["streamSettings"]["security"], "tls");
    assert_eq!(outbound["streamSettings"]["hysteriaSettings"]["version"], 2);
    assert_eq!(
        outbound["streamSettings"]["hysteriaSettings"]["auth"],
        "secret"
    );
    assert_eq!(
        outbound["streamSettings"]["tlsSettings"]["serverName"],
        "edge.example.invalid"
    );
}

#[test]
fn test_compile_hysteria2_ignores_removed_allow_insecure_field() {
    let endpoint = ExitEndpoint {
        id: "exit-hy2-insecure".to_owned(),
        tag: "exit-hy2-insecure".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Hysteria2 {
            address: "203.0.113.10".to_owned(),
            port: 443,
            password: "secret".to_owned(),
            server_name: Some("edge.example.invalid".to_owned()),
            allow_insecure: Some(true),
        },
    };

    let outbound = compile_outbound(&endpoint).expect("outbound compiles");

    assert_eq!(outbound["protocol"], "hysteria");
    assert!(!outbound["streamSettings"]["tlsSettings"]
        .as_object()
        .expect("tls settings object")
        .contains_key("allowInsecure"));
}

#[test]
fn test_compile_hysteria2_outbound_requires_password() {
    let endpoint = ExitEndpoint {
        id: "exit-hy2-missing-password".to_owned(),
        tag: "exit-hy2-missing-password".to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Hysteria2 {
            address: "203.0.113.10".to_owned(),
            port: 443,
            password: String::new(),
            server_name: Some("edge.example.invalid".to_owned()),
            allow_insecure: None,
        },
    };

    assert_missing_exit_field(
        compile_outbound(&endpoint),
        "exit-hy2-missing-password",
        "password",
    );
}
