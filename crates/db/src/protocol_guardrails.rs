//! 入口协议 × 地址类型(CF / 域名 / IP)的护栏校验。
//! 从 validation 拆出,避免该文件超过 550 行硬上限。
//! 决定哪些协议组合能过 CF、哪些直连 TLS 协议必须有域名。
//! CF 只代理 HTTP(S),故只放行 WS/gRPC/XHTTP + TLS 的 VLESS/Trojan。
//! 纯 IP 直连签不出证书,故 TLS 类协议(Trojan/HY2/VLESS-TLS)必须有域名直连地址。
//! Reality 借用第三方证书、Shadowsocks 无 TLS,二者免证书、不受此限。
//! 本模块纯函数,不读写数据库、不访问远端。
//! 由 lib 以 pub(crate) use 重导出,routing 层 `use crate::*` 调用不变。
//! 错误文案为中文,指明应改用的地址/协议。
//! 本头部满足前十行中文注释约束。

use crate::DbError;

/// CF 橙云只代理这组固定 HTTPS 端口;入口 listen_port 不在此集合,客户端到 CF 边缘会超时。
/// 来源:Cloudflare 官方支持的代理 HTTPS 端口(443/2053/2083/2087/2096/8443)。
pub(crate) const CF_SUPPORTED_HTTPS_PORTS: [u16; 6] = [443, 2053, 2083, 2087, 2096, 8443];

/// CF 入口端口护栏:橙云(cdn_enabled 或选中域名 kind=cf)入口的 listen_port 必须 ∈ CF 支持端口集合。
/// 非 CF 入口(direct/IP 直连)不受此限,任意端口放行(向后兼容)。
///
/// CF 只代理上述固定 HTTPS 端口,入口若绑别的端口,客户端连 CF 边缘必超时连不上,
/// 故在入口创建/更新护栏处提前拦下,给出明确的可用端口清单,而不是让用户线上排查超时。
pub(crate) fn validate_cf_entry_listen_port(
    listen_port: u16,
    cdn_enabled: bool,
    selected_kind: Option<&str>,
) -> Result<(), DbError> {
    let cf_guard = cdn_enabled || selected_kind == Some("cf");
    if !cf_guard || CF_SUPPORTED_HTTPS_PORTS.contains(&listen_port) {
        return Ok(());
    }
    Err(DbError::InvalidAgentPayload(
        "橙云(CF)入口端口必须是 CF 支持的 HTTPS 端口(443/2053/2083/2087/2096/8443)".to_string(),
    ))
}

/// CF 入口护栏:CF 只代理 HTTP(S),放行 VLESS/Trojan + WS/gRPC/XHTTP 传输 + TLS。
/// Reality(劫持 TLS 握手)、Shadowsocks(无 TLS)、HY2(UDP/QUIC)、裸 TCP 均不能过 CF。
///
/// 多域名 Phase 2:走 CF 护栏的条件改为「开了 CDN 开关」或「选中域名 kind=cf」二者之一。
/// 选了 CF 域名等价于声明该入口要经 Cloudflare,故即便没勾 cdn_enabled 也按 CF 限制协议组合。
pub(crate) fn validate_protocol_cdn_combination(
    protocol: &str,
    transport: &str,
    security: &str,
    cdn_enabled: bool,
    selected_kind: Option<&str>,
) -> Result<(), DbError> {
    // 选中 cf 域名 → 当作 CF 入口校验;否则按 cdn_enabled 开关决定是否约束。
    let cf_guard = cdn_enabled || selected_kind == Some("cf");
    // 既未开 CDN、也没选 cf 域名时不约束协议组合,直连按各自协议规则校验即可。
    if !cf_guard {
        return Ok(());
    }
    let protocol = protocol.trim().to_ascii_lowercase();
    let transport = transport.trim().to_ascii_lowercase();
    let security = security.trim().to_ascii_lowercase();
    let cf_transport = transport.contains("ws") || transport == "grpc" || transport == "xhttp";
    let cf_compatible =
        matches!(protocol.as_str(), "vless" | "trojan") && cf_transport && security == "tls";
    if cf_compatible {
        return Ok(());
    }
    Err(DbError::InvalidAgentPayload(
        "该协议不能过 CF;CF 只支持 VLESS/Trojan + WS/gRPC/XHTTP + TLS,其余请改用灰云域名或 IP 直连"
            .to_string(),
    ))
}

/// 直连 TLS 协议(Trojan/HY2/VLESS-TLS)必须有域名做 SNI/证书,纯 IP 直连签不出证书 → 拒绝。
/// CF 入口(cdn_enabled)证书走 cf_domain(本就是域名),不在此限;Reality 借证书、SS 无 TLS,均放行。
///
/// 多域名 Phase 2:选了 node_domain(direct/cf 任一,domain 本就是合法域名)时直接放行——
/// 证书锚定到该选中域名,SNI 即该域名,无需再按 server_name 猜 IP/域名(避免回填场景误判)。
/// 没选(selected_has_domain=false)则回退原行为:按 server_name 是否为域名判定。
pub(crate) fn validate_tls_protocol_needs_domain(
    security: &str,
    server_name: &str,
    cdn_enabled: bool,
    selected_has_domain: bool,
) -> Result<(), DbError> {
    if cdn_enabled || selected_has_domain {
        return Ok(());
    }
    // 只约束真 TLS(security=tls);Reality/SS 等非 tls 安全模式放行。
    if !security.trim().eq_ignore_ascii_case("tls") {
        return Ok(());
    }
    let sni = server_name.trim();
    let is_domain =
        !sni.is_empty() && sni.parse::<std::net::IpAddr>().is_err() && sni.contains('.');
    if is_domain {
        return Ok(());
    }
    Err(DbError::InvalidAgentPayload(
        "该 TLS 协议(Trojan/HY2/VLESS-TLS)需要域名直连地址签证书,纯 IP 不支持;请给节点配置域名直连地址,或改用 Reality/Shadowsocks"
            .to_string(),
    ))
}
