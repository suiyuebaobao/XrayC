//! 入口 TLS 证书锚定与直连 SNI 物化辅助。
//! 本模块从 routing_access_entries 拆出,避免该超长文件继续膨胀。
//! 核心规则:直连 TLS 入口的服务端证书与 server_name(订阅 SNI)都锚定节点 cert_domain(灰云),
//! 而不是 listen_host/IP。CF 入口 server_name 保持 cf_domain(订阅公布橙云地址);其证书路径一律
//! 锚定 cf_domain 自己的 LE 路径(token-less 经 CF :80 HTTP-01 自动签、有 token 走 DNS-01),
//! 每域名各自一张、绝不复用直连灰云证书(用户定:不复用别的域名证书)。
//! Reality 借用 dest、Shadowsocks 无 TLS,二者都不吃证书,本模块不触碰。
//! 入参为已物化的协议/安全字段;本模块纯函数,不读写 SQL,不访问远端。
//! cert_domain/cf_cert_mode/cf_domain 由调用方在事务内从 access_nodes 读出并透传。
//! 错误处理留给上层校验,本模块只做字段改写。
//! 本头部满足前十行中文注释约束。

use crate::config_text;
use crate::validation::{normalize_access_inbound_config, TLS_CERTIFICATE_KEYS, TLS_KEY_KEYS};
use crate::DbError;
use serde_json::Value;

use super::local_access_defaults::local_access_line_inbound_config;
use super::routing_entry_reality::{
    fill_vless_quantum_entry_config, fill_vless_reality_entry_config, server_name_for_entry,
};
use super::routing_entry_selected_domain::SelectedEntryDomain;

/// 节点侧证书锚定相关字段集合,由调用方在事务内从 access_nodes 读出并整体透传。
/// 把两个 Option 打包成一个参数,既守住调用方超长文件的行数,又避免参数顺序错位。
#[derive(Clone, Copy, Default)]
pub(crate) struct NodeCertAnchor<'a> {
    /// 灰云证书域名(直连 TLS 入口锚定到此)。
    pub(crate) cert_domain: Option<&'a str>,
    /// CF 橙云域名(CF 入口证书一律锚定到此自己的 LE 路径,per-domain,不复用直连灰云)。
    pub(crate) cf_domain: Option<&'a str>,
}

/// 节点证书锚定字段的拥有式版本:在事务内一次读出 access_nodes 对应列后持有所有权。
/// 借出 NodeCertAnchor(借用视图)供 prepare_access_entry 透传,把 SELECT 噪声从超长的
/// 入口创建文件挪到本证书子模块,集中维护跨层锚定口径。
pub(super) struct NodeCertAnchorOwned {
    pub(super) public_host: String,
    /// IP 直连地址(节点公网 IP):IP 直连入口(非 CDN、未选域名)且 admin 未显式填 listen_host 时,
    /// 订阅 server 应落裸 IP(开发方案.md §196「IP直连线 server=ip_direct_address」),非空才覆盖兜底。
    pub(super) ip_direct_address: String,
    cert_domain: Option<String>,
    cf_domain: Option<String>,
}

impl NodeCertAnchorOwned {
    /// 在事务内按节点 id 锁行读出 public_host、ip_direct_address 与证书锚定字段(cert_domain/cf_domain)。
    pub(super) async fn read_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        access_node_id: uuid::Uuid,
    ) -> Result<Self, DbError> {
        let (public_host, ip_direct_address, cert_domain, cf_domain) =
            sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>)>(
                "SELECT public_host, ip_direct_address, cert_domain, cf_domain \
                 FROM access_nodes WHERE id = $1 FOR UPDATE",
            )
            .bind(access_node_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| DbError::InvalidAgentPayload("中转节点不存在".to_string()))?;
        Ok(Self {
            public_host,
            ip_direct_address: ip_direct_address.unwrap_or_default(),
            cert_domain,
            cf_domain,
        })
    }

    /// 借出借用视图供 prepare_access_entry 透传;CF 入口证书一律锚定 cf_domain 自己的证书(per-domain)。
    pub(super) fn anchor(&self) -> NodeCertAnchor<'_> {
        NodeCertAnchor {
            cert_domain: self.cert_domain.as_deref(),
            cf_domain: self.cf_domain.as_deref(),
        }
    }
}

/// 物化入口的 server_name(订阅 SNI)与 inbound TLS 证书配置。
///
/// 一步完成:解析 server_name → 直连 TLS 锚定 cert_domain → 空配置补默认/归一 →
/// Reality 默认填充 → 证书路径锚定 cert_domain。把这条链路从超长的入口准备函数里抽出,
/// 既守住 routing_access_entries 的 550 行硬上限,又把证书/SNI 跨层规则集中在本模块。
///
/// 返回锚定后的 server_name 与已写好证书路径的 inbound_config,供上层继续做协议校验。
pub(super) fn materialize_entry_tls_config(
    protocol: &str,
    cdn_enabled: bool,
    listen_host: &str,
    anchor: NodeCertAnchor<'_>,
    selected: SelectedEntryDomain<'_>,
    mut inbound_config: Value,
) -> Result<(String, Value), DbError> {
    let base_server_name = server_name_for_entry(protocol, listen_host, &inbound_config);
    let server_name = anchor_direct_tls_server_name(
        protocol,
        cdn_enabled,
        &inbound_config,
        anchor.cert_domain,
        selected,
        base_server_name,
    );
    if inbound_config
        .as_object()
        .is_some_and(|object| object.is_empty())
    {
        inbound_config = local_access_line_inbound_config(protocol, &server_name)?;
    } else {
        inbound_config = normalize_access_inbound_config(protocol, inbound_config, &server_name)?;
    }
    inbound_config = fill_vless_reality_entry_config(protocol, &server_name, inbound_config)?;
    inbound_config = fill_vless_quantum_entry_config(protocol, inbound_config)?;
    inbound_config =
        anchor_tls_certificate_paths(protocol, cdn_enabled, anchor, selected, inbound_config);
    Ok((server_name, inbound_config))
}

/// 按选中域名 / 节点单字段选出 TLS 入口该锚定的证书域名(per-domain,每域名各自一张)。
///
/// 多域名:优先按入口选中的 node_domain 锚定——选 direct 域名锚该 direct 域名(HTTP-01 自签);
/// 选 cf 域名一律锚该 cf 域名自己的证书(token-less 经 CF :80 HTTP-01 自动签、有 token 走 DNS-01,
/// 实测穿 CF 能签出 cf 域名专属真证书),绝不复用直连灰云证书(用户定:不复用别的域名证书)。
///
/// 未选域名时回退节点单字段逻辑:CF 入口(cdn_enabled)锚 cf_domain 自己的证书,直连入口锚 cert_domain。
/// 返回 None 时上层不改写证书路径(既无选中域名也无节点证书域名,向后兼容)。
fn anchored_cert_domain<'a>(
    cdn_enabled: bool,
    anchor: NodeCertAnchor<'a>,
    selected: SelectedEntryDomain<'a>,
) -> Option<&'a str> {
    let non_empty = |value: Option<&'a str>| value.map(str::trim).filter(|s| !s.is_empty());
    // 选中域名优先:据其 kind 锚定(direct/cf 都锚选中域名自己的证书,per-domain 不复用)。
    if let Some(domain) = non_empty(selected.domain) {
        match selected.kind.map(str::trim) {
            Some("direct") => return Some(domain),
            // CF 入口一律锚定 cf 域名自己的证书(token-less 经 CF :80 HTTP-01 自动签、有 token 走 DNS-01;
            // 实测穿 CF 能签出 cf 域名专属真证书)——每域名各自一张,绝不复用直连灰云证书(用户定:不复用)。
            Some("cf") => return Some(domain),
            _ => return Some(domain),
        }
    }
    // 未选域名:回退节点单字段逻辑。CF 入口一律锚定 cf 域名自己的证书(per-domain,不复用直连灰云)。
    if cdn_enabled {
        if let Some(domain) = non_empty(anchor.cf_domain) {
            return Some(domain);
        }
    }
    non_empty(anchor.cert_domain)
}

/// 判断该入口是否为吃证书的 TLS 入口(需要 certificate_file/key_file)。
/// Trojan/HY2 强制 TLS;VLESS 仅当 security=tls 时吃证书;Reality/SS 不吃。
fn entry_uses_tls(protocol: &str, inbound_config: &Value) -> bool {
    let security = config_text(inbound_config, &["security"])
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(protocol, "trojan" | "hysteria") || security == "tls"
}

/// TLS 入口的 server_name(订阅 SNI)锚定:优先用入口选中域名,否则用节点 cert_domain。
///
/// 多域名 Phase 3:入口选了 node_domain 时,SNI 必须是选中域名(direct 直连域名或 cf 橙云域名),
/// 客户端按证书域名校验 TLS。这对 CF 入口(cdn_enabled)同样适用——选中的 cf 域名即对外 SNI。
/// 未选域名时回退旧逻辑:直连 TLS 入口锚定节点 cert_domain;CF 入口(cdn_enabled)SNI 保持 cf_domain
/// (由 server_name_for_entry 已物化),不在此覆盖。Reality/SS 不吃证书,也不走这里。
///
/// 在 normalize 之前调用,使后续证书路径直接从正确的 server_name 推导。
fn anchor_direct_tls_server_name(
    protocol: &str,
    cdn_enabled: bool,
    inbound_config: &Value,
    cert_domain: Option<&str>,
    selected: SelectedEntryDomain<'_>,
    server_name: String,
) -> String {
    if !entry_uses_tls(protocol, inbound_config) {
        return server_name;
    }
    // 选中域名优先:无论 direct/cf,SNI 都用选中域名(客户端按证书域名校验)。
    if let Some(domain) = selected.domain.map(str::trim).filter(|d| !d.is_empty()) {
        return domain.to_string();
    }
    // 未选域名:CF 入口 SNI 保持已物化的 cf_domain,不覆盖;直连入口锚定节点 cert_domain。
    if cdn_enabled {
        return server_name;
    }
    match cert_domain
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
    {
        Some(domain) => domain.to_string(),
        None => server_name,
    }
}

/// 把 TLS 入口的证书/私钥路径按节点 CF 证书模式锚定到对应域名的 LE 路径。
///
/// normalize_access_inbound_config 会按 server_name 推证书路径;对 CF 入口而言
/// server_name=cf_domain,直连入口可能是 IP,都需在 normalize 之后纠正。
/// 本函数据 anchored_cert_domain 选域名(优先入口选中 node_domain,否则节点单字段旧逻辑),
/// 覆盖证书/私钥路径(清掉所有大小写别名键,保留单一规范键)。Reality/SS 不吃证书,返回不动。
///
/// 选不出域名(既无选中域名也无节点 cert_domain/cf_domain)时不改写,保留 normalize 既有路径。
fn anchor_tls_certificate_paths(
    protocol: &str,
    cdn_enabled: bool,
    anchor: NodeCertAnchor<'_>,
    selected: SelectedEntryDomain<'_>,
    mut inbound_config: Value,
) -> Value {
    if !entry_uses_tls(protocol, &inbound_config) {
        return inbound_config;
    }
    let Some(domain) = anchored_cert_domain(cdn_enabled, anchor, selected) else {
        return inbound_config;
    };
    let Some(object) = inbound_config.as_object_mut() else {
        return inbound_config;
    };
    // 清掉所有别名键,避免遗留 live/{cf_domain}/ 之类的旧路径,再写回规范键。
    for key in TLS_CERTIFICATE_KEYS.iter().chain(TLS_KEY_KEYS.iter()) {
        object.remove(*key);
    }
    object.insert(
        "certificate_file".to_string(),
        Value::String(format!("/etc/letsencrypt/live/{domain}/fullchain.pem")),
    );
    object.insert(
        "key_file".to_string(),
        Value::String(format!("/etc/letsencrypt/live/{domain}/privkey.pem")),
    );
    inbound_config
}
