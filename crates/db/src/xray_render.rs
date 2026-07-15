//! 本模块组合 access-agent 所需的 Xray 配置。
//! 这里负责从 StoreData 选择线路、用户和出口端点。
//! 协议级字段转换委托给 xray_protocol 模块。
//! 函数只读取内存快照，不访问数据库或修改状态。
//! 探测状态只用于过滤离线出口，保持纯读模型语义。
//! 配置哈希保持稳定序列化，供心跳同步判断使用。
//! 入口绑定地址保留原有通配地址策略。
//! 新增渲染规则应优先放在本模块而非 crate root。
//! 本模块不得包含 SQL、PgStore 方法或事务流程。
//! 文件行数保持低于 500 行，便于后续维护。

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;
use xrayc_core::{
    binding_credential, effective_user_rate_limit_down_bps, effective_user_rate_limit_up_bps,
    rate_limit_mark_for_user, AccessLine, ExitEndpoint, StoreData, User,
};
use xrayc_xray_config::{
    AccessConfig as XrayAccessConfig, AccessLine as XrayAccessLine,
    AccessProtocol as XrayAccessProtocol, AccessUser as XrayAccessUser,
    ExitEndpoint as XrayExitEndpoint, ExitProtocol as XrayExitProtocol, LogLevel as XrayLogLevel,
    RoutingRule as XrayRoutingRule, UserRateLimit as XrayUserRateLimit,
};

use crate::xray_protocol::{
    access_inbound_tag_for_id, access_line_inbound_security, access_line_server_name,
    access_line_short_ids, access_line_tls_certificate_file, access_line_tls_key_file,
    access_protocol_for_line, config_text, exit_endpoint_tag, xray_exit_protocol_for_endpoint,
};
use crate::xray_render_cert::{resolve_tls_cert_paths, TlsCertResolution};
use crate::xray_render_local::local_exit_services_for_node;

pub(crate) fn selected_node_id(data: &StoreData, reported_node_id: Option<Uuid>) -> Option<Uuid> {
    reported_node_id.filter(|node_id| data.access_nodes.contains_key(node_id))
}

pub(crate) fn build_access_config_with_probe_policy(
    data: &StoreData,
    node_id: Option<Uuid>,
    block_unhealthy_lines: bool,
) -> Option<XrayAccessConfig> {
    let node_id = node_id?;
    let mut access_line_ids = data
        .access_lines
        .values()
        .filter(|line| line.access_node_id == node_id && line.enabled)
        .map(|line| line.id)
        .collect::<Vec<_>>();
    access_line_ids.sort();

    let now = Utc::now();
    let mut used_endpoint_ids = HashSet::new();
    let mut marked_endpoint_keys = HashSet::new();
    let mut access_lines = Vec::new();
    let mut shared_inbounds = BTreeMap::<SharedInboundKey, SharedInbound>::new();
    let mut routing_rules = Vec::new();
    let mut rate_limit_by_user = HashMap::new();
    let mut rate_limits = Vec::new();
    let local_exit_services = local_exit_services_for_node(data, node_id);
    // 节点域名清单(含 cert_status):用于 TLS 入口证书就绪兜底(BUG-D)。
    let node_domains = data
        .access_nodes
        .get(&node_id)
        .map(|node| node.domains.as_slice())
        .unwrap_or(&[]);

    for access_line_id in access_line_ids {
        let Some(line) = data.access_lines.get(&access_line_id) else {
            continue;
        };
        let Some(protocol) = access_protocol_for_line(line) else {
            continue;
        };
        // TLS 入口证书就绪兜底(per-domain):入口锚定的证书域名(CF→cf_domain)若 cert_status≠valid,
        // 跳过该单条 inbound、等 agent 给该域名签好自己的证书后自然渲染(绝不复用其它域名证书,用户定:每域名各自一张);
        // 绝不让整份 apply 因单条证书未就绪 bail(§4)。
        let tls_cert_paths = match resolve_tls_cert_paths(line, node_domains) {
            TlsCertResolution::Keep => None,
            TlsCertResolution::Skip => continue,
        };
        let mut assignments = data
            .user_exit_assignments
            .iter()
            .filter(|assignment| assignment.access_line_id == access_line_id)
            .collect::<Vec<_>>();
        assignments.sort_by_key(|assignment| (assignment.user_id, assignment.exit_endpoint_id));

        for assignment in assignments {
            let Some(user) = data.users.get(&assignment.user_id) else {
                continue;
            };
            let Some(subscription) = data.subscriptions.get(&assignment.user_id) else {
                continue;
            };
            if user.disabled
                || !subscription.active
                || subscription.expires_at <= now
                || subscription.remaining_bytes() == 0
                || !user_has_active_subscription_token(data, user.id)
            {
                continue;
            }
            let Some(endpoint) = healthy_supported_endpoint(
                data,
                node_id,
                assignment.exit_pool_id,
                assignment.exit_endpoint_id,
                block_unhealthy_lines,
            ) else {
                continue;
            };

            let rate_limit = {
                // 方向限速:分别解析上行/下行有效值(均回退对称值),门控取二者较大值。
                let up = effective_user_rate_limit_up_bps(data, user.id).unwrap_or(0);
                let down = effective_user_rate_limit_down_bps(data, user.id).unwrap_or(0);
                rate_limit_entry_for_user(
                    data,
                    line,
                    &mut rate_limit_by_user,
                    &mut rate_limits,
                    user,
                    up,
                    down,
                )
            };
            // 限速与不限速用户都进同线路共享入站(单入站多用户、守真实端口);
            // 限速差异只体现在出站 sockopt.mark + user 路由规则,入口端口不再按用户拆分。
            let outbound_tag = if let Some(rate_limit) = rate_limit {
                marked_endpoint_keys.insert((endpoint.id, user.id, rate_limit.mark));
                marked_exit_endpoint_tag(endpoint.id, user.id)
            } else {
                used_endpoint_ids.insert(endpoint.id);
                exit_endpoint_tag(endpoint.id)
            };
            let access_user = xray_access_user(line, user);
            let shared_key = shared_inbound_key(line);
            let shared_inbound =
                shared_inbounds
                    .entry(shared_key)
                    .or_insert_with(|| SharedInbound {
                        line: line.clone(),
                        protocol: protocol.clone(),
                        runtime_id: line.id.to_string(),
                        users: Vec::new(),
                        default_exit_tag: None,
                        tls_cert_override: tls_cert_paths.clone(),
                    });
            // 默认出站只作未命中 user 规则时的兜底;每个用户都有显式 user 路由,故首个出站即可。
            shared_inbound
                .default_exit_tag
                .get_or_insert_with(|| outbound_tag.clone());
            routing_rules.push(XrayRoutingRule {
                inbound_tag: Some(access_inbound_tag_for_id(&shared_inbound.runtime_id)),
                user_email: Some(access_user.email.clone()),
                outbound_tag,
            });
            shared_inbound.users.push(access_user);
        }
    }

    for mut shared_inbound in shared_inbounds.into_values() {
        shared_inbound
            .users
            .sort_by(|left, right| left.email.cmp(&right.email));
        access_lines.push(xray_access_line(
            &shared_inbound.line,
            shared_inbound.protocol,
            shared_inbound.runtime_id,
            shared_inbound.line.listen_port,
            shared_inbound.users,
            shared_inbound
                .default_exit_tag
                .unwrap_or_else(|| "direct".to_string()),
            shared_inbound.tls_cert_override,
        ));
    }

    if access_lines.is_empty() && local_exit_services.is_empty() {
        return None;
    }

    let mut exit_endpoint_ids = used_endpoint_ids.into_iter().collect::<Vec<_>>();
    exit_endpoint_ids.sort();
    let mut exit_endpoints = exit_endpoint_ids
        .into_iter()
        .filter_map(|endpoint_id| {
            find_endpoint(data, endpoint_id).and_then(|endpoint| {
                let protocol = xray_exit_protocol_for_endpoint(endpoint)?;
                Some(XrayExitEndpoint {
                    id: endpoint.id.to_string(),
                    tag: exit_endpoint_tag(endpoint.id),
                    sockopt_mark: None,
                    protocol,
                })
            })
        })
        .collect::<Vec<_>>();
    let mut marked_endpoint_keys = marked_endpoint_keys.into_iter().collect::<Vec<_>>();
    marked_endpoint_keys.sort_by_key(|(endpoint_id, user_id, _)| (*endpoint_id, *user_id));
    for (endpoint_id, user_id, mark) in marked_endpoint_keys {
        let Some(endpoint) = find_endpoint(data, endpoint_id) else {
            continue;
        };
        // 本机出口回环限速修复:限速用户路由到「本节点自有的本机出口(self_hosted)」时,本机出口本质是
        // 本节点 freedom 直连出网(xray-config 给每个本机出口服务硬编码 freedom 出站)。若仍按协议把带
        // mark 出站渲染成「连本机出口环回入站(127.0.0.1:<port>)」的对应协议,这一跳走 lo 不经 eth0,
        // agent 的 eth0/ifb 限速器抓不到 mark → 该用户完全不被限速(真机铁证)。这里折叠成带该用户 mark 的
        // freedom 直连出站,让 用户→公网 的包带 mark 直接走 eth0 被限速器抓到;egress 等价(同为本节点公网
        // IP 直连出网),且每用户计费按入站 user email 归属、与出站协议无关,折叠不影响计费/统计归属。
        // 只折叠 owner==当前节点的 self_hosted(走环回);跨节点 self_hosted 是经 eth0 连对端的真实网络跳,
        // 折叠会把出网 IP 从对端节点错改成本节点,故保持其原协议出站。
        let protocol = if is_self_hosted_local_exit(endpoint, node_id) {
            XrayExitProtocol::Direct
        } else {
            let Some(protocol) = xray_exit_protocol_for_endpoint(endpoint) else {
                continue;
            };
            protocol
        };
        exit_endpoints.push(XrayExitEndpoint {
            id: format!("{endpoint_id}:{user_id}"),
            tag: marked_exit_endpoint_tag(endpoint_id, user_id),
            sockopt_mark: Some(mark),
            protocol,
        });
    }
    if !access_lines.is_empty() && exit_endpoints.is_empty() {
        return None;
    }

    routing_rules.sort_by(|left, right| {
        (
            left.inbound_tag.as_deref(),
            left.user_email.as_deref(),
            left.outbound_tag.as_str(),
        )
            .cmp(&(
                right.inbound_tag.as_deref(),
                right.user_email.as_deref(),
                right.outbound_tag.as_str(),
            ))
    });
    access_lines.sort_by(|left, right| {
        (
            left.source_line_id.as_str(),
            left.listen_port,
            left.id.as_str(),
        )
            .cmp(&(
                right.source_line_id.as_str(),
                right.listen_port,
                right.id.as_str(),
            ))
    });

    Some(XrayAccessConfig {
        node_id: node_id.to_string(),
        log_level: XrayLogLevel::Warning,
        stats_enabled: true,
        access_lines,
        local_exit_services,
        exit_endpoints,
        routing_rules,
        rate_limits,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SharedInboundKey {
    bind_host: String,
    listen_port: u16,
    protocol: String,
    transport: String,
    flow: String,
    udp_enabled: bool,
    udp_packet_encoding: String,
    xhttp_path: String,
    xhttp_host: String,
    xhttp_mode: String,
    inbound_security: Option<String>,
    server_name: Option<String>,
    reality_dest: Option<String>,
    reality_private_key: Option<String>,
    reality_short_ids: Vec<String>,
    tls_certificate_file: Option<String>,
    tls_key_file: Option<String>,
}

#[derive(Debug, Clone)]
struct SharedInbound {
    line: AccessLine,
    protocol: XrayAccessProtocol,
    runtime_id: String,
    users: Vec<XrayAccessUser>,
    default_exit_tag: Option<String>,
    // TLS 证书就绪兜底:cert 域名 cert_status≠valid 时改锚到的 (cert_file, key_file)。
    tls_cert_override: Option<(String, String)>,
}

fn shared_inbound_key(line: &AccessLine) -> SharedInboundKey {
    SharedInboundKey {
        bind_host: xray_bind_host(&line.listen_host),
        listen_port: line.listen_port,
        protocol: line.protocol.trim().to_ascii_lowercase(),
        transport: line.transport.trim().to_ascii_lowercase(),
        flow: line.flow.clone(),
        udp_enabled: line.udp_enabled,
        udp_packet_encoding: line.udp_packet_encoding.clone(),
        xhttp_path: line.xhttp_path.clone(),
        xhttp_host: line.xhttp_host.clone(),
        xhttp_mode: line.xhttp_mode.clone(),
        inbound_security: access_line_inbound_security(line),
        server_name: access_line_server_name(line),
        reality_dest: config_text(
            &line.inbound_config,
            &["dest", "reality_dest", "realityDest"],
        )
        .or_else(|| access_line_server_name(line).map(|server_name| format!("{server_name}:443"))),
        reality_private_key: config_text(
            &line.inbound_config,
            &[
                "private_key",
                "privateKey",
                "reality_private_key",
                "realityPrivateKey",
            ],
        ),
        reality_short_ids: access_line_short_ids(line),
        tls_certificate_file: access_line_tls_certificate_file(line),
        tls_key_file: access_line_tls_key_file(line),
    }
}

#[derive(Debug, Clone, Copy)]
struct RateLimitEntry {
    mark: u32,
}

fn xray_access_user(line: &AccessLine, user: &User) -> XrayAccessUser {
    XrayAccessUser {
        xray_user_key: user.xray_user_key.clone(),
        credential: binding_credential(&line.protocol, &user.access_credential, line.id),
        email: xrayc_xray_config::stats_user_email(&line.id.to_string(), &user.xray_user_key),
        level: 0,
    }
}

fn xray_access_line(
    line: &AccessLine,
    protocol: XrayAccessProtocol,
    runtime_id: String,
    listen_port: u16,
    users: Vec<XrayAccessUser>,
    default_exit_tag: String,
    // 证书就绪兜底改锚后的 (cert_file, key_file);None 时沿用 inbound_config 原路径。
    tls_cert_override: Option<(String, String)>,
) -> XrayAccessLine {
    let (tls_certificate_file, tls_key_file) = match tls_cert_override {
        Some((cert_file, key_file)) => (Some(cert_file), Some(key_file)),
        None => (
            access_line_tls_certificate_file(line),
            access_line_tls_key_file(line),
        ),
    };
    XrayAccessLine {
        id: runtime_id,
        source_line_id: line.id.to_string(),
        // xray-config 的 AccessLine 仍保留 runtime_core 字段，DB 侧单内核统一写 xray。
        runtime_core: "xray".to_string(),
        listen_host: xray_bind_host(&line.listen_host),
        listen_port,
        transport: line.transport.clone(),
        xhttp_path: line.xhttp_path.clone(),
        xhttp_host: line.xhttp_host.clone(),
        xhttp_mode: line.xhttp_mode.clone(),
        inbound_security: access_line_inbound_security(line),
        server_name: access_line_server_name(line),
        reality_dest: config_text(
            &line.inbound_config,
            &["dest", "reality_dest", "realityDest"],
        )
        .or_else(|| access_line_server_name(line).map(|server_name| format!("{server_name}:443"))),
        reality_private_key: config_text(
            &line.inbound_config,
            &[
                "private_key",
                "privateKey",
                "reality_private_key",
                "realityPrivateKey",
            ],
        ),
        reality_short_ids: access_line_short_ids(line),
        tls_certificate_file,
        tls_key_file,
        protocol,
        users,
        default_exit_tag,
    }
}

fn rate_limit_entry_for_user(
    data: &StoreData,
    line: &AccessLine,
    rate_limit_by_user: &mut HashMap<Uuid, RateLimitEntry>,
    rate_limits: &mut Vec<XrayUserRateLimit>,
    user: &User,
    rate_limit_up_bps: u64,
    rate_limit_down_bps: u64,
) -> Option<RateLimitEntry> {
    // 门控:上下行任一方向有限速即视为该用户需要限速类与 fwmark;均为 0 则不限速。
    let gate = rate_limit_up_bps.max(rate_limit_down_bps);
    if gate == 0 {
        return None;
    }
    if let Some(entry) = rate_limit_by_user.get(&user.id).copied() {
        return Some(entry);
    }
    // mark 由 core 按节点确定性派生(hash(user_id)+去冲突),落区间 [0x10000,0xFFFFF];不再用 0x10000+index。
    let mark = rate_limit_mark_for_user(data, line, user.id)?;
    let index = rate_limits.len() + 1;
    let class_id = 100_u16.saturating_add(index as u16);
    let entry = RateLimitEntry { mark };
    rate_limit_by_user.insert(user.id, entry);
    rate_limits.push(XrayUserRateLimit {
        user_id: user.id.to_string(),
        xray_user_key: user.xray_user_key.clone(),
        rate_limit_bps: gate,
        rate_limit_up_bps,
        rate_limit_down_bps,
        mark,
        class_id,
    });
    Some(entry)
}

fn marked_exit_endpoint_tag(endpoint_id: Uuid, user_id: Uuid) -> String {
    format!("exit-{}-user-{}", endpoint_id.simple(), user_id.simple())
}

pub(crate) fn user_has_active_subscription_token(data: &StoreData, user_id: Uuid) -> bool {
    data.tokens.values().any(|token| token.user_id == user_id)
}

pub(crate) fn healthy_supported_endpoint(
    data: &StoreData,
    access_node_id: Uuid,
    exit_pool_id: Uuid,
    exit_endpoint_id: Uuid,
    block_unhealthy_lines: bool,
) -> Option<&ExitEndpoint> {
    let pool = data.exit_pools.get(&exit_pool_id)?;
    if !pool.enabled {
        return None;
    }
    pool.members.iter().find(|endpoint| {
        // 本机出口(self_hosted)是节点同机自家出口、中转直接连它(本地/内网),按 CLAUDE.md §4
        // 天然可指派、不需上游探测就绪;否则会陷入"要监听才能探测健康、要健康才下发监听"的死锁。
        // 第三方上游仍按探测派生的 healthy / probe 态门控,不给真实离线上游下发。
        let self_hosted = endpoint
            .ownership
            .trim()
            .eq_ignore_ascii_case("self_hosted");
        endpoint.id == exit_endpoint_id
            && (endpoint.healthy || self_hosted)
            && (!block_unhealthy_lines
                || self_hosted
                || !probe_state_blocks_endpoint(data, access_node_id, endpoint.id))
            && xray_exit_protocol_for_endpoint(endpoint).is_some()
    })
}

fn xray_bind_host(public_host: &str) -> String {
    let host = public_host.trim();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if ip.is_loopback() || ip.is_unspecified() {
            return ip.to_string();
        }
    }
    // 中转入口的公开地址用于订阅下发；Xray 入站应绑定本机通配地址，
    // 避免公网地址未配置在网卡上时 Agent 安装后启动失败。
    "0.0.0.0".to_string()
}

pub(crate) fn probe_state_blocks_endpoint(
    data: &StoreData,
    access_node_id: Uuid,
    exit_endpoint_id: Uuid,
) -> bool {
    data.access_exit_probe_states
        .get(&(access_node_id, exit_endpoint_id))
        .map(|state| state.effective_status == "offline")
        .unwrap_or(false)
}

/// 判定出口是否为「当前节点自有的本机出口(self_hosted)」:owner==当前节点 → 中转直接连它走环回 lo。
/// 限速带 mark 出站对这类出口须折叠成 freedom 直连,否则 mark 落不到 eth0、限速器抓不到(详见调用处注释)。
fn is_self_hosted_local_exit(endpoint: &ExitEndpoint, node_id: Uuid) -> bool {
    endpoint
        .ownership
        .trim()
        .eq_ignore_ascii_case("self_hosted")
        && endpoint.owner_access_node_id == Some(node_id)
}

fn find_endpoint(data: &StoreData, exit_endpoint_id: Uuid) -> Option<&ExitEndpoint> {
    data.exit_pools
        .values()
        .flat_map(|pool| pool.members.iter())
        .find(|endpoint| endpoint.id == exit_endpoint_id)
}

pub(crate) fn access_config_hash(config: &XrayAccessConfig) -> String {
    let mut digest = Sha256::new();
    digest.update(b"xrayc-access-config-v3:");
    let bytes = serde_json::to_vec(config).expect("access config should serialize");
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

pub(crate) fn empty_access_config_hash(node_id: Uuid) -> String {
    let mut digest = Sha256::new();
    digest.update(b"xrayc-empty-access-config-v3:");
    digest.update(node_id.as_bytes());
    format!("{:x}", digest.finalize())
}
