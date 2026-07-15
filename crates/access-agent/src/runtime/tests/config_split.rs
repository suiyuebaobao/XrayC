//! 本模块测试按内核拆分配置时的路由规则过滤。
//! 重点验证 inbound_tag 必须按完整 access-{line_id} 精确匹配，
//! 防止 line-1 子串命中 line-10 的标签把别的线路规则串进本内核配置。

use xrayc_xray_config::{
    AccessConfig, AccessLine, AccessProtocol, AccessUser, ExitEndpoint, ExitProtocol, LogLevel,
    RoutingRule,
};

use crate::config::RuntimeCore;

use super::super::split::split_config_for_runtime;

/// 构造一条最小可用的接入线路，仅保留拆分逻辑关心的字段。
fn line(id: &str, runtime_core: &str, user_key: &str) -> AccessLine {
    AccessLine {
        id: id.to_owned(),
        source_line_id: String::new(),
        runtime_core: runtime_core.to_owned(),
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
            xray_user_key: user_key.to_owned(),
            credential: "11111111-1111-1111-1111-111111111111".to_owned(),
            email: format!("{user_key}@xrayc"),
            level: 0,
        }],
        default_exit_tag: "exit-socks".to_owned(),
    }
}

/// inbound_tag 与编译器一致，固定形态为 access-{line_id}。
fn routing_rule(line_id: &str, outbound_tag: &str) -> RoutingRule {
    RoutingRule {
        inbound_tag: Some(format!("access-{line_id}")),
        user_email: None,
        outbound_tag: outbound_tag.to_owned(),
    }
}

fn exit_endpoint(id: &str) -> ExitEndpoint {
    ExitEndpoint {
        id: id.to_owned(),
        tag: id.to_owned(),
        sockopt_mark: None,
        protocol: ExitProtocol::Socks {
            address: "127.0.0.1".to_owned(),
            port: 1080,
            username: None,
            password: None,
        },
    }
}

/// line-1 与 line-10 同时存在时，xray 内核拆分必须只保留 access-line-1 的规则，
/// 绝不能因为 "line-1" 是 "line-10" 的子串而把 line-10 的规则也带进来。
#[test]
fn test_split_routing_rules_match_inbound_tag_exactly() {
    let config = AccessConfig {
        node_id: "node-a".to_owned(),
        log_level: LogLevel::Warning,
        stats_enabled: true,
        access_lines: vec![
            line("line-1", "xray", "user-1"),
            line("line-10", "xray", "user-10"),
        ],
        exit_endpoints: vec![exit_endpoint("exit-socks"), exit_endpoint("exit-line-10")],
        local_exit_services: Vec::new(),
        // 只给 line-1 配路由规则，line-10 不配；据此验证 access-line-1
        // 不会因为是 access-line-10 的子串而把 line-10 的规则误纳入。
        routing_rules: vec![routing_rule("line-1", "exit-socks")],
        rate_limits: Vec::new(),
    };

    let xray = split_config_for_runtime(&config, RuntimeCore::Xray);

    // 两条 xray 线路都保留。
    assert_eq!(xray.access_lines.len(), 2);
    // 路由规则只应保留 access-line-1，access-line-10 不得被子串误纳入。
    assert_eq!(xray.routing_rules.len(), 1);
    assert_eq!(
        xray.routing_rules[0].inbound_tag.as_deref(),
        Some("access-line-1")
    );
    assert_eq!(xray.routing_rules[0].outbound_tag, "exit-socks");
    // line-10 的出口端点不应被串进来（它没有任何规则引用）。
    let exit_ids = xray
        .exit_endpoints
        .iter()
        .map(|endpoint| endpoint.id.as_str())
        .collect::<Vec<_>>();
    assert!(exit_ids.contains(&"exit-socks"));
    assert!(!exit_ids.contains(&"exit-line-10"));
}

/// inbound_tag 为空（None）的全局规则继续保留，行为不变。
#[test]
fn test_split_keeps_routing_rules_without_inbound_tag() {
    let mut config = AccessConfig {
        node_id: "node-a".to_owned(),
        log_level: LogLevel::Warning,
        stats_enabled: true,
        access_lines: vec![line("line-1", "xray", "user-1")],
        exit_endpoints: vec![exit_endpoint("exit-socks")],
        local_exit_services: Vec::new(),
        routing_rules: Vec::new(),
        rate_limits: Vec::new(),
    };
    config.routing_rules.push(RoutingRule {
        inbound_tag: None,
        user_email: None,
        outbound_tag: "exit-socks".to_owned(),
    });

    let xray = split_config_for_runtime(&config, RuntimeCore::Xray);

    assert_eq!(xray.routing_rules.len(), 1);
    assert!(xray.routing_rules[0].inbound_tag.is_none());
}
