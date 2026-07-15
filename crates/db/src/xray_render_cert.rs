//! TLS 入口证书路径就绪兜底判定(per-domain)。
//! 从 xray_render 拆出,守住主渲染文件 550 行硬上限。
//! 核心:入口锚定的证书域名(CF 入口=cf_domain)若证书尚未就绪,控制面写侧锚的
//! live/{domain}/ 在节点上还不存在 → agent xray-test「no such file」→ 整份 apply bail。
//! 用 node_domains.cert_status(agent 心跳回填、控制面已有、不依赖 token)对齐两层:
//! cert 域名未就绪时跳过该单条 inbound(绝不复用其它域名证书,用户定:每域名各自一张),
//! 等 agent 给本域名签好自己的证书后自然渲染;Skip 不拖垮整份 apply。
//! 本模块纯函数,只读 AccessLine 与 NodeDomainView 快照,不访问数据库或远端。
//! Reality/SS 等非 TLS 入口与已就绪证书一律 Keep,不影响既有渲染。
//! 证书路径只认 /etc/letsencrypt/live/{domain}/ 形态,自定义路径保持不动。
//! 本头部满足前十行中文注释约束。

use xrayc_core::{AccessLine, NodeDomainView};

use crate::xray_protocol::access_line_tls_certificate_file;

/// TLS 入口证书路径就绪兜底的判定结果。
pub(crate) enum TlsCertResolution {
    /// 沿用 inbound_config 里的原证书路径(非 TLS、证书已就绪、或无从判定时)。
    Keep,
    /// 跳过该单条 inbound(原锚定域名证书未就绪;不复用其它域名证书,等本域名证书签好后自然渲染)。
    Skip,
}

/// 渲染 TLS 入口前判定其证书路径是否就绪,不就绪则跳过该单条 inbound(per-domain,不复用其它域名证书)。
///
/// 背景:CF 入口证书锚定 cf 域名自己的 LE 路径(token-less 经 CF :80 HTTP-01 自签、有 token 走 DNS-01),
/// 但证书签好前控制面写侧锚的 live/{cf_domain}/ 在节点上还不存在 → xray-test「no such file」→ 整份 apply bail。
/// 用 node_domains.cert_status 对齐两层:cert 域名 cert_status≠valid 时跳过该单条 inbound、等 agent 给该
/// 域名签好自己的证书后自然渲染(用户定:每域名各自一张,绝不复用别的域名证书),Skip 绝不拖垮整份 apply(§4)。
/// 非 TLS 入口、原证书域名已 valid、或证书路径解析不出域名时一律 Keep。
pub(crate) fn resolve_tls_cert_paths(
    line: &AccessLine,
    node_domains: &[NodeDomainView],
) -> TlsCertResolution {
    let Some(cert_path) = access_line_tls_certificate_file(line) else {
        return TlsCertResolution::Keep;
    };
    let Some(cert_domain) = cert_domain_from_letsencrypt_path(&cert_path) else {
        return TlsCertResolution::Keep;
    };
    // 节点域名清单里查不到该证书域名时无从判定,保持原路径不动(向后兼容)。
    let Some(status) = node_domains
        .iter()
        .find(|domain| domain.domain == cert_domain)
        .map(|domain| domain.cert_status.trim())
    else {
        return TlsCertResolution::Keep;
    };
    if status.eq_ignore_ascii_case("valid") {
        return TlsCertResolution::Keep;
    }
    // 原锚定证书未就绪:不复用其它域名证书,跳过等本域名证书签好后自然渲染
    // (per-domain,每域名各自一张;Skip 不拖垮整份 apply)。
    TlsCertResolution::Skip
}

/// 从 `/etc/letsencrypt/live/{domain}/fullchain.pem` 形态的证书路径里取出域名段。
/// 非该形态(自定义路径等)返回 None,调用方据此保持原路径不动。
fn cert_domain_from_letsencrypt_path(cert_path: &str) -> Option<String> {
    let rest = cert_path.trim().strip_prefix("/etc/letsencrypt/live/")?;
    let domain = rest.split('/').next()?.trim();
    (!domain.is_empty()).then(|| domain.to_string())
}
