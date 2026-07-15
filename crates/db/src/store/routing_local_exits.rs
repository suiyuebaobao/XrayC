//! 本模块创建中转节点自建出口线路。
//! 自建出口线路只写入统一线路池，不自动创建用户入口。
//! 所属中转节点只作为线路来源标记，后续在线路池、中转绑定和线路分组页复用。
//! 批量创建时每行独立校验协议、网络模式、地址端口和连接配置。
//! 这里不触碰 access_lines、line_groups 或 plan_line_groups。
//! 删除节点时仍按 exit_resources.access_node_id 级联清理来源线路。
//! 所有敏感连接配置沿用 exit_endpoints 字段加密。
//! 返回值只包含 ID、协议和网络模式，不回显明文凭据。
//! 新增网络模式会先保存到 stream_config，Xray 编译支持需单独扩展。
//! 注释保持中文，方便运营口径和实现边界对齐。
use super::routing_local_exits_fill::fill_local_exit_outbound_config;
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

/// 本机出口 host 留空时的默认地址:回环地址,不过 CF、对所有节点类型通用。
/// 渲染侧本机出口 inbound 绑 0.0.0.0(ACCESS_INBOUND_LISTEN),故 127.0.0.1 可达。
/// pub(crate) 以便回归测试钉死「留空回退默认地址不命中 CF 护栏」(Bug ④)。
pub(crate) const LOCAL_EXIT_DEFAULT_HOST: &str = "127.0.0.1";

impl PgStore {
    pub async fn create_admin_local_exit_lines(
        &self,
        access_node_id: Uuid,
        input: AdminLocalExitLinesInput,
    ) -> Result<Value, DbError> {
        if input.lines.is_empty() {
            return Err(DbError::InvalidInput(
                "请至少添加一条本机出口线路".to_string(),
            ));
        }
        if input.lines.len() > 32 {
            return Err(DbError::InvalidInput(
                "单次最多添加 32 条本机出口线路".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        // 锁行同时读出 cert_domain(=域名直连地址):本机出口的「要证书」协议要靠它签/锚定证书,
        // 出口侧护栏据此放行或拦截,和入口侧 validate_tls_protocol_needs_domain 同思路。
        // public_host 仍随节点行读出但本机出口不再使用(host 留空默认回环地址),
        // 下划线前缀避免 unused 告警;保留列读出方便后续若需回显节点对外地址。
        let (node_name, _public_host, cert_domain) = sqlx::query_as::<
            _,
            (String, String, Option<String>),
        >(
            "SELECT name, public_host, cert_domain FROM access_nodes WHERE id = $1 FOR UPDATE",
        )
        .bind(access_node_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("中转节点不存在: {access_node_id}")))?;
        // 节点是否配了域名直连地址:cert_domain 去空白后非空才算有。
        let node_has_cert_domain = cert_domain
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        // 取节点全部 CF(橙云)域名,供 host 护栏比对:本机出口 host 命中任一 CF 域名要拦(Bug ④)。
        let node_cf_domains = node_cf_domains_in_tx(&mut tx, access_node_id).await?;

        let mut created_lines = Vec::with_capacity(input.lines.len());
        for (index, line) in input.lines.into_iter().enumerate() {
            let fallback_name = format!("{}本机出口{}", node_name, index + 1);
            let resource_name = if line.resource_name.trim().is_empty() {
                fallback_name
            } else {
                required_admin_text(&line.resource_name, "本机出口线路名称", 128)?
            };
            let endpoint_name = if line.endpoint_name.trim().is_empty() {
                resource_name.clone()
            } else {
                required_admin_text(&line.endpoint_name, "本机出口协议档名称", 128)?
            };
            let region_code = optional_admin_text(&line.region_code, 32);
            let outbound_type = validate_endpoint_type_name(&line.outbound_type)?;
            validate_hong_kong_hy2(outbound_type, &region_code)?;
            let network_mode = validate_local_exit_network_mode(outbound_type, &line.network_mode)?;
            // 本机出口被同节点中转就地消费,host 留空时默认回环地址 127.0.0.1:
            // 渲染侧本机出口 inbound 绑 0.0.0.0(ACCESS_INBOUND_LISTEN),127.0.0.1 可达;
            // 不再回落 public_host——橙云(CF)节点的 public_host 是 CF 代理域名,中转连它
            // 会走 CF 边缘、CF 不代理本机出口端口而连不到自己,故对所有节点类型统一用回环地址。
            let host = if line.host.trim().is_empty() {
                LOCAL_EXIT_DEFAULT_HOST.to_string()
            } else {
                required_admin_text(&line.host, "本机出口线路地址", 255)?
            };
            // host 护栏:命中节点任一 CF(橙云)域名直接拒(Bug ④),不让 CF 域名混进本机出口连接地址。
            validate_local_exit_host_not_cf(&host, &node_cf_domains)?;
            if line.port == 0 {
                return Err(DbError::InvalidInput(
                    "本机出口线路端口必须在 1-65535 之间".to_string(),
                ));
            }
            // 解析本行选中的 node_domain(多域名 Phase 2):校验该域名存在且属于本节点,取其 kind。
            // 不选(None)则 selected_kind=None,护栏回退节点级行为(向后兼容)。
            let selected_kind =
                selected_local_exit_domain_kind_in_tx(&mut tx, access_node_id, line.node_domain_id)
                    .await?;
            // 出口侧协议-地址护栏(提前判断):要证书的协议(Trojan/HY2/VLESS-TLS)必须有可签证书的直连域名。
            // 刻意放在协议字段校验之前——无证书直接给出「请先配/选直连域名,纯 IP 节点请用免证书协议」的
            // 清晰原因,而不是先让人补 password/SNI/证书路径再发现该节点/域名根本不能用 Trojan。
            // 选了 cf 域名时一律拒(CF 只放 ws/grpc/xhttp);选 direct 放行;没选回退节点级 cert_domain 判定。
            // VLESS 的 tls 判定读原始配置里的 security 字段即可,无需等 fill 补默认值。
            validate_local_exit_cert_domain(
                outbound_type,
                &line.outbound_config,
                node_has_cert_domain,
                selected_kind.as_deref(),
            )?;
            let outbound_config =
                fill_local_exit_outbound_config(outbound_type, &host, line.outbound_config)?;
            validate_exit_endpoint_protocol_config(outbound_type, &outbound_config)?;
            validate_local_exit_network_security(outbound_type, network_mode, &outbound_config)?;
            let stream_config = local_exit_stream_config(line.stream_config, network_mode)?;
            let probe_config = line.probe_config;

            let exit_resource_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO exit_resources (
                    name, region_code, provider_name, ownership,
                    access_node_id, enabled, status
                )
                VALUES ($1, $2, '自建', 'self_hosted', $3, $4, 'unknown')
                RETURNING id
                "#,
            )
            .bind(&resource_name)
            .bind(&region_code)
            .bind(access_node_id)
            .bind(line.enabled)
            .fetch_one(&mut *tx)
            .await?;

            let exit_endpoint_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO exit_endpoints (
                    exit_resource_id, name, outbound_type, host, port,
                    outbound_config, stream_config, probe_config, enabled, node_domain_id
                )
                VALUES ($1, $2, $3::endpoint_type, $4, $5, $6, $7, $8, $9, $10)
                RETURNING id
                "#,
            )
            .bind(exit_resource_id)
            .bind(&endpoint_name)
            .bind(outbound_type)
            .bind(&host)
            .bind(i32::from(line.port))
            .bind(outbound_config)
            .bind(stream_config)
            .bind(probe_config)
            .bind(line.enabled)
            .bind(line.node_domain_id)
            .fetch_one(&mut *tx)
            .await?;

            created_lines.push(json!({
                "exit_resource_id": exit_resource_id,
                "exit_endpoint_id": exit_endpoint_id,
                "resource_name": resource_name,
                "endpoint_name": endpoint_name,
                "outbound_type": outbound_type,
                "network_mode": network_mode,
                "host": host,
                "port": line.port,
                "enabled": line.enabled
            }));
        }

        sqlx::query(
            r#"
            UPDATE access_nodes
            SET config_dirty = TRUE,
                config_dirty_at = now(),
                desired_config_hash = NULL,
                config_dirty_reason = 'local_exit_lines_changed'
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(json!({
            "access_node_id": access_node_id,
            "access_node_name": node_name,
            "created_count": created_lines.len(),
            "created_lines": created_lines
        }))
    }
}

/// 在事务内解析本机出口某行选中的 node_domain,返回其 kind(direct/cf)。
///
/// 校验该域名存在且属于本节点(access_node_id 比对),避免选了别的节点的域名绕过护栏。
/// 不选(None)时返回 None,护栏据此回退节点级行为(向后兼容)。
pub(super) async fn selected_local_exit_domain_kind_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    node_domain_id: Option<Uuid>,
) -> Result<Option<String>, DbError> {
    let Some(domain_id) = node_domain_id else {
        return Ok(None);
    };
    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT access_node_id, kind FROM node_domains WHERE id = $1",
    )
    .bind(domain_id)
    .fetch_optional(&mut **tx)
    .await?;
    match row {
        Some((owner_id, _)) if owner_id != access_node_id => Err(DbError::InvalidInput(
            "选中的域名不属于该中转节点,请重新选择本节点的域名".to_string(),
        )),
        Some((_, kind)) => Ok(Some(kind)),
        None => Err(DbError::InvalidInput(
            "选中的节点域名不存在,请刷新后重新选择".to_string(),
        )),
    }
}

/// 在事务内取某节点全部 CF(橙云)域名(node_domains.kind='cf' + 旧 access_nodes.cf_domain 兜底)。
///
/// 多域名模型下 CF 域名落 node_domains;旧单 cf_domain 列向后兼容也一并纳入,避免漏判老节点。
/// 返回去空白后非空的域名列表,供 validate_local_exit_host_not_cf 比对(Bug ④ 护栏)。
pub(super) async fn node_cf_domains_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
) -> Result<Vec<String>, DbError> {
    let mut domains = sqlx::query_scalar::<_, String>(
        "SELECT domain FROM node_domains WHERE access_node_id = $1 AND kind = 'cf'",
    )
    .bind(access_node_id)
    .fetch_all(&mut **tx)
    .await?;
    // 旧 access_nodes.cf_domain 单列向后兼容:存在且非空时也算 CF 域名。
    if let Some(cf_domain) =
        sqlx::query_scalar::<_, Option<String>>("SELECT cf_domain FROM access_nodes WHERE id = $1")
            .bind(access_node_id)
            .fetch_optional(&mut **tx)
            .await?
            .flatten()
    {
        if !cf_domain.trim().is_empty() {
            domains.push(cf_domain);
        }
    }
    Ok(domains
        .into_iter()
        .filter(|domain| !domain.trim().is_empty())
        .collect())
}

pub(super) fn validate_local_exit_network_mode(
    outbound_type: &str,
    value: &str,
) -> Result<&'static str, DbError> {
    let normalized = value.trim().to_ascii_lowercase();
    let normalized = normalized.as_str();
    if outbound_type == "hysteria" {
        return match normalized {
            "" | "udp" | "hy2" | "hysteria" | "hysteria2" => Ok("udp"),
            other => Err(DbError::InvalidAgentPayload(format!(
                "HY2 本机出口线路只支持 UDP 网络模式，当前为 {other}"
            ))),
        };
    }

    // 合并承载:tcp,udp 这类逗号合并值,**仅 Shadowsocks/SOCKS 允许**(它们的 settings.network 原生支持
    // "tcp,udp" 一条同时收 TCP/UDP,不必拆两条)。逐段校验每段 ∈ {tcp,udp},规范化输出 "tcp,udp"。
    // 其余协议(VLESS/Trojan 等)的 UDP 走 packetEncoding/协议固有承载,不在网络模式合并里表达。
    if normalized.contains(',') {
        let parts: Vec<&str> = normalized
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let all_l4 = !parts.is_empty() && parts.iter().all(|p| matches!(*p, "tcp" | "udp"));
        if all_l4 && matches!(outbound_type, "shadowsocks" | "socks") {
            return Ok(match (parts.contains(&"tcp"), parts.contains(&"udp")) {
                (true, true) => "tcp,udp",
                (false, true) => "udp",
                _ => "tcp",
            });
        }
        return Err(DbError::InvalidAgentPayload(format!(
            "该协议不支持合并网络模式: {normalized}"
        )));
    }

    // HTTP 代理纯 TCP、无 UDP;只允许 tcp(防御纵深,前端已只给 TCP)。
    if outbound_type == "http" && !matches!(normalized, "" | "tcp") {
        return Err(DbError::InvalidAgentPayload(format!(
            "HTTP 本机出口只支持 TCP,当前为 {normalized}"
        )));
    }
    // 裸 udp 承载只有 SS/SOCKS(及上面单列的 HY2)支持;VLESS/Trojan 的 UDP 走协议固有承载/XUDP,不用裸 udp。
    if normalized == "udp" && !matches!(outbound_type, "shadowsocks" | "socks") {
        return Err(DbError::InvalidAgentPayload(format!(
            "{outbound_type} 本机出口不支持裸 UDP 承载(UDP 走协议自带 / XUDP)"
        )));
    }

    let selected = if normalized.is_empty() {
        "tcp"
    } else {
        normalized
    };
    match selected {
        "tcp" | "udp" | "xhttp" | "xudp" | "ws" | "grpc" => Ok(match selected {
            "tcp" => "tcp",
            "udp" => "udp",
            "xhttp" => "xhttp",
            "xudp" => "xudp",
            "ws" => "ws",
            "grpc" => "grpc",
            _ => unreachable!(),
        }),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的本机出口网络模式: {other}"
        ))),
    }
}

pub(super) fn validate_local_exit_network_security(
    outbound_type: &str,
    network_mode: &str,
    outbound_config: &Value,
) -> Result<(), DbError> {
    if outbound_type != "vless" || vless_endpoint_security(outbound_config) != "reality" {
        return Ok(());
    }
    if matches!(network_mode, "udp" | "xudp") {
        return Err(DbError::InvalidAgentPayload(
            "VLESS Reality 本机出口只支持 TCP 网络模式".to_string(),
        ));
    }
    if network_mode != "tcp" {
        return Err(DbError::InvalidAgentPayload(
            "VLESS Reality 本机出口只支持 TCP 网络模式".to_string(),
        ));
    }
    Ok(())
}

/// 出口侧协议-地址护栏(多域名 Phase 2:按选中域名判定)。
///
/// 要证书的协议 = Trojan、HY2(hysteria),或 VLESS 且 security=tls;否则签不出/锚不到证书。
/// 免证书协议(SOCKS5/HTTP/Shadowsocks/VLESS-Reality/VLESS-none)始终放行,纯 IP/无证书节点也能用。
///
/// 判定依据按是否选了 node_domain 分流:
/// - 选了域名(selected_kind=Some):direct 域名可签 LE 证书 → Trojan/HY2/VLESS-TLS 放行;
///   cf 域名只能过 CF(ws/grpc/xhttp+tls)→ 本机出口的要证书协议(Trojan/HY2/裸 TLS)一律拒;
/// - 没选域名(selected_kind=None):回退节点级行为——节点有 direct 域名(node_has_cert_domain)即放行。
pub(super) fn validate_local_exit_cert_domain(
    outbound_type: &str,
    outbound_config: &Value,
    node_has_cert_domain: bool,
    selected_kind: Option<&str>,
) -> Result<(), DbError> {
    let requires_cert = match outbound_type {
        "trojan" | "hysteria" => true,
        "vless" => vless_endpoint_security(outbound_config) == "tls",
        _ => false,
    };
    if !requires_cert {
        return Ok(());
    }
    // 选了 cf 域名:CF 只代理 ws/grpc/xhttp,本机出口的 Trojan/HY2/VLESS-TLS 不能过 → 直接拒。
    if selected_kind == Some("cf") {
        return Err(DbError::InvalidAgentPayload(
            "选中的是 CF 域名,CF 只支持 ws/grpc/xhttp;本机出口的 Trojan/HY2/VLESS-TLS \
             请改选直连域名,或改用 SOCKS5/HTTP/Shadowsocks/VLESS-Reality"
                .to_string(),
        ));
    }
    // 选了 direct 域名:可签 LE 证书,放行。没选域名:回退节点级(有 direct 域名即放行)。
    if selected_kind == Some("direct") || node_has_cert_domain {
        return Ok(());
    }
    Err(DbError::InvalidAgentPayload(
        "本机出口的 Trojan/HY2/VLESS-TLS 需要节点先配置域名直连地址(证书),\
         纯 IP/无证书节点请用 SOCKS5/HTTP/Shadowsocks/VLESS-Reality"
            .to_string(),
    ))
}

/// 本机出口 host 的 CF 域名护栏(Bug ④ 防回归,纯函数便于单测)。
///
/// 本机出口是中转节点同机自家出口、中转直接连它(本地/内网/回环),**绝不走 CF(橙云)边缘**:
/// CF 只代理入口的 HTTPS 端口、不代理本机出口的自定义端口,中转去连 CF 域名会经 CF 边缘连不到
/// 自己的出口端口 → 探测 offline → 订阅"无可用线路"。故 host 命中节点任一 CF 域名时一律拒,
/// 让管理员改用 IP 直连地址 / 直连域名 / 留空(回退回环默认地址)。
///
/// 比对前两边都去空白并转小写(域名大小写不敏感);节点无 CF 域名时直接放行(纯 IP/纯直连节点不受影响)。
pub(crate) fn validate_local_exit_host_not_cf(
    host: &str,
    cf_domains: &[String],
) -> Result<(), DbError> {
    let normalized_host = host.trim().to_ascii_lowercase();
    if normalized_host.is_empty() {
        return Ok(());
    }
    let hit_cf = cf_domains.iter().any(|domain| {
        let normalized = domain.trim().to_ascii_lowercase();
        !normalized.is_empty() && normalized == normalized_host
    });
    if hit_cf {
        return Err(DbError::InvalidAgentPayload(
            "本机出口连接地址不能用 CF(橙云)域名:本机出口是中转同机自家出口、中转直接连它,\
             走 CF 会经边缘连不到出口端口。请改用 IP 直连地址 / 直连域名,或留空走本机回环地址"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn vless_endpoint_security(outbound_config: &Value) -> String {
    config_text(outbound_config, &["security"])
        .map(|security| security.trim().to_ascii_lowercase())
        .filter(|security| !security.is_empty())
        .unwrap_or_else(|| {
            if config_text(
                outbound_config,
                &["public_key", "publicKey", "reality_public_key"],
            )
            .is_some()
                || config_text(
                    outbound_config,
                    &[
                        "private_key",
                        "privateKey",
                        "reality_private_key",
                        "realityPrivateKey",
                    ],
                )
                .is_some()
                || config_text(
                    outbound_config,
                    &["short_id", "shortId", "reality_short_id", "realityShortId"],
                )
                .is_some()
            {
                "reality".to_string()
            } else {
                "none".to_string()
            }
        })
}

pub(super) fn local_exit_stream_config(value: Value, network_mode: &str) -> Result<Value, DbError> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("Stream 配置必须是 JSON 对象".to_string()))?;
    object.insert("network_mode".to_string(), json!(network_mode));
    object.insert(
        "network".to_string(),
        json!(match network_mode {
            "xudp" => "tcp",
            other => other,
        }),
    );
    if network_mode == "xudp" {
        object.insert("udp_packet_encoding".to_string(), json!("xudp"));
    }
    Ok(Value::Object(object))
}
