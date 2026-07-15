/// 数据库测试分片 52。
// 本文件覆盖 P4 协议护栏:CF 放开 VLESS/Trojan + WS/gRPC/XHTTP;直连 TLS 需域名。
// 纯函数校验,不依赖 PostgreSQL,不需要 DATABASE_URL。
// 只使用 example.test 占位与 RFC 5737 示例 IP,不写真实服务器/凭据。
// CF 护栏:Reality/SS/HY2/裸 TCP + CF 拒绝。
// 直连 TLS:纯 IP SNI 拒绝、域名 SNI 放行、Reality/SS 放行。
// 断言以校验函数返回为准,符合跨层对齐红线。
// 父级 tests 模块通过 include! 引入本文件。
// 本头部满足前十行中文注释约束。
// 不在此处写真实 token/订阅/代理凭据。

#[test]
fn test_cf_allows_vless_and_trojan_ws_grpc_xhttp() {
    use crate::protocol_guardrails::validate_protocol_cdn_combination as cf;
    assert!(cf("trojan", "ws", "tls", true, None).is_ok(), "Trojan-WS 应能过 CF");
    assert!(cf("vless", "grpc", "tls", true, None).is_ok(), "VLESS-gRPC 应能过 CF");
    assert!(
        cf("trojan", "xhttp", "tls", true, None).is_ok(),
        "Trojan-XHTTP 应能过 CF"
    );
    assert!(cf("vless", "ws", "tls", true, None).is_ok());
    assert!(cf("vless", "tcp", "tls", true, None).is_err(), "裸 TCP 不能过 CF");
    assert!(
        cf("vless", "ws", "reality", true, None).is_err(),
        "Reality 不能过 CF"
    );
    assert!(
        cf("shadowsocks", "ws", "tls", true, None).is_err(),
        "SS 不能过 CF"
    );
    assert!(cf("hysteria", "ws", "tls", true, None).is_err(), "HY2 不能过 CF");
    // 未开 CDN 不约束协议组合。
    assert!(cf("trojan", "tcp", "tls", false, None).is_ok());
    // 多域名 Phase 2:选中 cf 域名(即便没勾 cdn_enabled)按 CF 护栏——VLESS/ws/tls 放行,裸 TCP 拒。
    assert!(
        cf("vless", "ws", "tls", false, Some("cf")).is_ok(),
        "选 cf 域名的 VLESS-WS-TLS 应过 CF"
    );
    assert!(
        cf("trojan", "tcp", "tls", false, Some("cf")).is_err(),
        "选 cf 域名的裸 TCP 不能过 CF"
    );
    // 选 direct 域名不触发 CF 护栏(direct 走直连规则,不在此函数约束)。
    assert!(cf("trojan", "tcp", "tls", false, Some("direct")).is_ok());
}

#[test]
fn test_direct_tls_requires_domain_not_ip() {
    use crate::protocol_guardrails::validate_tls_protocol_needs_domain as needs_domain;
    assert!(
        needs_domain("tls", "192.0.2.10", false, false).is_err(),
        "纯 IP TLS 应拒"
    );
    assert!(
        needs_domain("tls", "direct.example.test", false, false).is_ok(),
        "域名 TLS 应放行"
    );
    assert!(
        needs_domain("reality", "192.0.2.10", false, false).is_ok(),
        "Reality 放行"
    );
    assert!(
        needs_domain("", "192.0.2.10", false, false).is_ok(),
        "SS 无 TLS 放行"
    );
    assert!(
        needs_domain("tls", "cf.example.test", true, false).is_ok(),
        "CF 不在此限"
    );
    assert!(needs_domain("tls", "", false, false).is_err(), "空 SNI 拒");
    // 多域名 Phase 2:选了 node_domain(has_domain=true)时即便 server_name 像 IP 也放行,
    // 证书锚定到选中域名,无需按 server_name 猜测。
    assert!(
        needs_domain("tls", "192.0.2.10", false, true).is_ok(),
        "选了直连域名时 TLS 应放行(证书锚定选中域名)"
    );
}
