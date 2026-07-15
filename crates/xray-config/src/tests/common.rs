//! 本模块提供单元测试共用的断言和样例配置。
//! 样例配置保持最小可编译形态，便于各测试按需覆盖字段。
//! 这里不参与生产构建，只服务测试模块。

use serde_json::Value;

use crate::*;

pub(super) fn assert_missing_exit_field(
    result: Result<Value, XrayConfigError>,
    expected_endpoint_id: &str,
    expected_field: &'static str,
) {
    assert!(matches!(
        result,
        Err(XrayConfigError::MissingExitField(endpoint_id, field))
            if endpoint_id == expected_endpoint_id && field == expected_field
    ));
}

pub(super) fn sample_access_config() -> AccessConfig {
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
    }
}
