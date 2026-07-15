//! 入口管理与入口出口绑定节点写入逻辑。
//! 本模块承载新的 access_entries 主路径。
//! 绑定出口时会同步生成兼容 access_lines 行。
//! 订阅和 agent 运行期在过渡阶段仍读取 access_lines。
//! 分组成员使用 line_group_binding_nodes 保存绑定节点。
//! 本模块不处理前端展示，不访问远端服务器。
//! 所有真实出口敏感字段仍只来自 exit_endpoints。
//! 新增写操作会标记相关中转节点配置待同步。
//! routing_entries 仅保留运行表迁移和底层兼容生成逻辑。
//! 本头部满足前十行中文注释约束。

use super::dirty::*;
use super::existence::ensure_line_group_exists_in_tx;
use super::routing_entries::{
    access_entry_network_mode, selected_exit_endpoint_in_tx, sync_single_endpoint_exit_pool_in_tx,
};
use super::routing_entry_cert::{
    materialize_entry_tls_config, NodeCertAnchor, NodeCertAnchorOwned,
};
use super::routing_entry_reality::*;
use super::routing_entry_selected_domain::{
    resolve_selected_entry_domain_in_tx, SelectedEntryDomain, SelectedEntryDomainHolder,
};
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

// 两个纯数据行结构体已抽到同级 routing_access_entries_rows 模块(守 550 行硬上限)。
// 这里以 pub(crate) use 原样引回,使其它模块对 routing_access_entries::{PreparedAccessEntry,
// StoredAccessEntry} 的引用完全不变(口径不改、可见性不变)。
pub(crate) use super::routing_access_entries_rows::{PreparedAccessEntry, StoredAccessEntry};

pub(crate) fn prepare_access_entry(
    input: &AdminAccessEntryInput,
    default_host: String,
    anchor: NodeCertAnchor<'_>,
    selected: SelectedEntryDomain<'_>,
) -> Result<PreparedAccessEntry, DbError> {
    let protocol = validate_access_protocol(&input.protocol)?;
    let requested_transport = if protocol == "hysteria" && input.transport.trim().is_empty() {
        "hysteria".to_string()
    } else {
        optional_admin_text(&input.transport, 32)
    };
    let mode = access_entry_network_mode(protocol, &requested_transport)?;
    validate_access_protocol_transport(protocol, mode.transport)?;
    let listen_host = optional_admin_text(&input.listen_host, 255);
    let listen_host = if listen_host.is_empty() {
        default_host
    } else {
        listen_host
    };
    // CDN 模式下订阅应公布 CDN 域名（客户端连 Cloudflare 等 CDN 边缘代理），
    // 而不是节点真实 listen_host；xray 入站仍绑 0.0.0.0，listen_host 只作对外公布地址。
    let listen_host = if input.cdn_enabled && !input.cdn_hostname.trim().is_empty() {
        optional_admin_text(&input.cdn_hostname, 255)
    } else if let Some(direct_domain) = selected_direct_listen_host(selected) {
        // 域名直连线（选中 kind=direct 的 node_domain）：订阅连接地址 = 证书域名（开发方案.md §2.2.1 口径
        // 「域名直连线 server=cert_domain、sni=cert_domain」）；不能回退成节点 IP/public_host 泄露真实地址。
        // 与 CF 公布 cdn_hostname 同源：listen_host 只作对外公布地址，xray 入站仍绑 0.0.0.0。
        direct_domain
    } else {
        listen_host
    };
    let security = optional_admin_text(&input.security, 32);
    let configured_server_name = optional_admin_text(&input.server_name, 255);
    let mut config_object = serde_json::Map::new();
    if !security.is_empty() {
        config_object.insert("security".to_string(), json!(security));
    }
    if !configured_server_name.is_empty() {
        config_object.insert("server_name".to_string(), json!(configured_server_name));
    }
    // 量子加密标记**仅 VLESS** 才透传 inbound_config(供 materialize 内 fill_vless_quantum_entry_config
    // 判定生成/清空;仅非 Reality VLESS 生效)。写给非 VLESS 会让其 inbound_config 凭空非空,致 SS 等
    // 依赖"空配置才注入默认凭据"的协议在 routing_entry_cert.rs 的 is_empty() 注入门被跳过、PSK 不注入
    // → shadowsocks_subscription_supported 判 false 过滤掉线路(1ebb430 回归根因,db part70 测试守护)。
    if protocol == "vless" {
        config_object.insert(
            "vless_quantum_encryption".to_string(),
            json!(input.vless_quantum_encryption),
        );
    }
    // 物化 server_name(订阅 SNI)与 inbound TLS 证书配置:直连 TLS 锚定 cert_domain,
    // CF 入口 SNI 仍 = cf_domain,其证书路径一律锚 cf_domain 自己的 LE 路径(per-domain,不复用直连灰云)。
    // 链路细节集中在 cert 子模块。
    let (server_name, inbound_config) = materialize_entry_tls_config(
        protocol,
        input.cdn_enabled,
        &listen_host,
        anchor,
        selected,
        Value::Object(config_object),
    )?;
    validate_vless_reality_entry_config(protocol, &inbound_config)?;
    validate_vless_reality_entry_network_mode(protocol, &inbound_config, &mode)?;
    validate_access_inbound_config(protocol, &inbound_config, &server_name)?;
    let effective_security = config_text(&inbound_config, &["security"]).unwrap_or(security);
    // 域名直连护栏(防御纵深):选中 kind=direct 直连域名时不允许 Reality。
    // Reality 用 decoy SNI、不碰你的证书,属 IP 直连技术;域名直连用证书做 TLS,二者矛盾。
    // 开发方案 §2.2.x:域名直连只放 TLS 全族,Reality 仅 IP 直连。即使前端漏拦,这里也兜底拒。
    if effective_security == "reality" && selected.kind.map(str::trim) == Some("direct") {
        return Err(DbError::InvalidAgentPayload(
            "域名直连不支持 Reality,请改用 TLS,或选 IP 直连(免证书)再用 Reality".to_string(),
        ));
    }
    // CF 护栏:启用 CDN 时只放行 VLESS-WS-TLS,其余协议组合(Trojan/HY2/SS/Reality 等)拒绝。
    // 具体判定在 validation.rs,此处只做薄调用,避免本超长文件继续膨胀。
    validate_protocol_cdn_combination(
        protocol,
        mode.transport,
        &effective_security,
        input.cdn_enabled,
        selected.kind,
    )?;
    // CF 入口端口护栏:橙云只代理固定 HTTPS 端口,listen_port 不在集合内客户端连 CF 边缘会超时,
    // 故 cdn_enabled 或选中 cf 域名时强制端口 ∈ CF 支持集合;非 CF 入口任意端口不受限。
    validate_cf_entry_listen_port(input.listen_port, input.cdn_enabled, selected.kind)?;
    // 直连 TLS 协议(Trojan/HY2/VLESS-TLS)纯 IP 签不出证书 → 要求域名直连地址(SNI 为域名)。
    // 选了 node_domain(direct/cf)时 domain 本就合法,直接放行(由 selected.has_domain 表达)。
    validate_tls_protocol_needs_domain(
        &effective_security,
        &server_name,
        input.cdn_enabled,
        selected.has_domain,
    )?;
    let (public_key, short_id) = vless_reality_entry_public_fields(protocol, &inbound_config)?;
    let flow = config_text(&inbound_config, &["flow"]).unwrap_or_default();
    let ws_path = optional_admin_text(&input.ws_path, 255);
    let ws_host = optional_admin_text(&input.ws_host, 255);
    let xhttp_path = if matches!(mode.transport, "ws" | "xhttp" | "grpc") {
        ws_path.clone()
    } else {
        String::new()
    };
    let xhttp_host = if matches!(mode.transport, "ws" | "xhttp" | "grpc") {
        ws_host.clone()
    } else {
        String::new()
    };
    Ok(PreparedAccessEntry {
        listen_host,
        protocol,
        transport: mode.transport,
        security: effective_security,
        server_name,
        public_key,
        short_id,
        flow,
        udp_enabled: mode.udp_enabled,
        udp_packet_encoding: mode.udp_packet_encoding.to_string(),
        ws_path,
        ws_host,
        xhttp_path,
        xhttp_host,
        xhttp_mode: "auto",
        inbound_config,
    })
}

/// 计算 prepare_access_entry 的默认兜底 listen_host(admin 未显式填 listen_host 时才生效)。
///
/// IP 直连入口(未选 node_domain 且非 CDN)且节点配了 ip_direct_address(非空)时,兜底用裸 IP
/// `ip_direct_address`,使订阅 server 落裸 IP(开发方案.md §196「IP直连线 server=ip_direct_address」、
/// §146「ip_direct_address=节点公网IP,供 VLESS(Reality/none)/SS 直连」);否则回落 `public_host`。
///
/// 这与域名直连分支(selected_direct_listen_host 强制物化 cert_domain)对称,但仅改默认兜底:
/// 这里不强制覆盖 admin 显式填的 listen_host(显式输入由 prepare_access_entry 优先尊重),
/// 域名直连为防真实 IP 泄露才强制覆盖。本函数只产出"没填时用哪个地址",纯函数、不读 SQL。
///
/// 只影响 listen_host(→订阅 server);Reality 的借用域名是 server_name(→订阅 sni),两者独立,不受此影响。
pub(crate) fn default_listen_host_for_entry(
    input: &AdminAccessEntryInput,
    public_host: &str,
    ip_direct_address: &str,
) -> String {
    if input.node_domain_id.is_none() && !input.cdn_enabled && !ip_direct_address.trim().is_empty()
    {
        ip_direct_address.trim().to_string()
    } else {
        public_host.to_string()
    }
}

/// 选中 kind=direct 的 node_domain 时,返回该直连域名作为订阅对外公布的 listen_host(连接地址)。
///
/// 仅 direct 档命中:CF(kind=cf)走上面的 cdn_hostname 分支、不选域名(IP 直连/Reality)返回 None
/// 保持节点 IP/public_host 现有行为。Reality 不通过选 direct 域名表达,故不会落进这里(server 仍 = IP/借用域名)。
fn selected_direct_listen_host(selected: SelectedEntryDomain<'_>) -> Option<String> {
    if selected.kind.map(str::trim) != Some("direct") {
        return None;
    }
    selected
        .domain
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn reseal_access_entry_inbound_config_for_line(value: Value) -> Result<Value, DbError> {
    // 字段加密已移除，入口配置以明文存取，入口态到线路态无需再做密文转换，直接透传。
    Ok(value)
}

/// 入口端口冲突校验:同一中转节点下,enabled 入口的 listen_port 必须唯一。
///
/// Xray 同一端口无法承载多协议入站,同节点同端口入口会让 reload bind 失败回滚,
/// 因此冲突判定与协议无关——只看 access_node_id + listen_port + enabled。
/// 写入前先做这层应用校验,返回带节点/端口信息的友好错误,
/// 不依赖数据库唯一索引的原始报错(那条报错文案不可控、也不脱敏)。
///
/// 仅在本次入口 enabled=true 时校验:停用入口不占用端口,允许复用。
/// `exclude_entry_id` 用于更新场景排除自身,避免"原地不动也报冲突"。
/// 范围限定 access_entries 之间;local_exit_services 端口冲突不在本单元(遗留项)。
pub(crate) async fn ensure_listen_port_available_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    listen_port: i32,
    enabled: bool,
    exclude_entry_id: Option<Uuid>,
) -> Result<(), DbError> {
    // 停用入口不抢占端口,直接放行。
    if !enabled {
        return Ok(());
    }
    // 命中即冲突:同节点、同端口、已启用、且不是自身。
    // SELECT 1 仅判存在性,不取占用方名称,避免把入口名等信息泄露到错误文案里。
    let conflict = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT 1
        FROM access_entries
        WHERE access_node_id = $1
          AND listen_port = $2
          AND enabled = true
          AND ($3::uuid IS NULL OR id <> $3)
        LIMIT 1
        "#,
    )
    .bind(access_node_id)
    .bind(listen_port)
    .bind(exclude_entry_id)
    .fetch_optional(&mut **tx)
    .await?;
    if conflict.is_some() {
        // 错误文案只带节点 id(UUID,本就是非敏感标识)与端口,不带任何主机/域名/凭据。
        return Err(DbError::InvalidInput(format!(
            "端口已被占用:中转节点 {access_node_id} 上已存在启用的 {listen_port} 端口入口,请改用其它端口"
        )));
    }
    Ok(())
}

impl PgStore {
    pub(crate) async fn repair_access_entry_binding_line_inbound_configs(
        &self,
    ) -> Result<u64, DbError> {
        let rows = sqlx::query_as::<_, (Uuid, Value)>(
            r#"
            SELECT al.id, e.inbound_config
            FROM access_lines al
            JOIN access_entry_exit_bindings b ON b.id = al.id
            JOIN access_entries e ON e.id = b.access_entry_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut repaired = 0_u64;
        let mut tx = self.pool.begin().await?;
        for (access_line_id, entry_inbound_config) in rows {
            let access_line_inbound_config =
                reseal_access_entry_inbound_config_for_line(entry_inbound_config)?;
            let result = sqlx::query("UPDATE access_lines SET inbound_config = $2 WHERE id = $1")
                .bind(access_line_id)
                .bind(access_line_inbound_config)
                .execute(&mut *tx)
                .await?;
            repaired += result.rows_affected();
        }
        tx.commit().await?;
        Ok(repaired)
    }

    pub async fn create_admin_access_entry(
        &self,
        input: AdminAccessEntryInput,
    ) -> Result<Uuid, DbError> {
        let mut tx = self.pool.begin().await?;
        // 锁行读出 public_host 与证书锚定三件套;CF 入口证书按 cf_cert_mode 选 cf_domain 或 cert_domain。
        let node = NodeCertAnchorOwned::read_in_tx(&mut tx, input.access_node_id).await?;
        // 解析选中域名(多域名 Phase 2/3):护栏据 kind 判定,证书锚定据 domain/cf_cert_mode;不选则回退。
        let mut selected_holder = SelectedEntryDomainHolder::default();
        let selected = resolve_selected_entry_domain_in_tx(
            &mut tx,
            input.access_node_id,
            input.node_domain_id,
            &mut selected_holder,
        )
        .await?;
        // IP 直连入口(未选域名、非 CDN)且 admin 未显式填 listen_host 时,兜底用裸 IP ip_direct_address,
        // 与域名直连物化 cert_domain 对称(开发方案.md §196);其余情形仍回落 public_host。
        let default_host =
            default_listen_host_for_entry(&input, &node.public_host, &node.ip_direct_address);
        let prepared = prepare_access_entry(&input, default_host, node.anchor(), selected)?;
        // 写入前做端口冲突校验:同节点已有 enabled 入口占用该端口则拒绝(协议无关)。
        ensure_listen_port_available_in_tx(
            &mut tx,
            input.access_node_id,
            i32::from(input.listen_port),
            input.enabled,
            None,
        )
        .await?;
        let inbound_config = prepared.inbound_config;
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO access_entries (
                access_node_id, name, listen_host, listen_port, protocol, transport,
                security, user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                udp_packet_encoding, ws_path, ws_host, xhttp_path, xhttp_host, xhttp_mode,
                cdn_enabled, cdn_provider, cdn_hostname, cdn_server, inbound_config,
                enabled, sort_weight, node_domain_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, gen_random_uuid()::text, $8, $9, $10,
                    $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)
            RETURNING id
            "#,
        )
        .bind(input.access_node_id)
        .bind(required_admin_text(&input.name, "入口名称", 128)?)
        .bind(prepared.listen_host)
        .bind(i32::from(input.listen_port))
        .bind(prepared.protocol)
        .bind(prepared.transport)
        .bind(prepared.security)
        .bind(prepared.server_name)
        .bind(prepared.public_key)
        .bind(prepared.short_id)
        .bind(prepared.flow)
        .bind(prepared.udp_enabled)
        .bind(prepared.udp_packet_encoding)
        .bind(prepared.ws_path)
        .bind(prepared.ws_host)
        .bind(prepared.xhttp_path)
        .bind(prepared.xhttp_host)
        .bind(prepared.xhttp_mode)
        .bind(input.cdn_enabled)
        .bind(optional_admin_text(&input.cdn_provider, 32))
        .bind(optional_admin_text(&input.cdn_hostname, 255))
        .bind(optional_admin_text(&input.cdn_server, 255))
        .bind(inbound_config)
        .bind(input.enabled)
        .bind(input.sort_weight.clamp(0, 1_000_000))
        .bind(input.node_domain_id)
        .fetch_one(&mut *tx)
        .await?;
        mark_access_node_dirty_in_tx(&mut tx, input.access_node_id, "admin_created_access_entry")
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn create_admin_access_entry_exit_binding(
        &self,
        access_entry_id: Uuid,
        input: AdminAccessEntryExitBindingInput,
    ) -> Result<Uuid, DbError> {
        let mut tx = self.pool.begin().await?;
        let entry = sqlx::query_as::<_, StoredAccessEntry>(
            r#"
            SELECT access_node_id, name, listen_host, listen_port, protocol, transport,
                   user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                   udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode, inbound_config,
                   enabled, sort_weight
            FROM access_entries
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(access_entry_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidInput("入口不存在".to_string()))?;
        let exit_endpoint =
            selected_exit_endpoint_in_tx(&mut tx, entry.access_node_id, input.exit_endpoint_id)
                .await?;
        validate_hong_kong_hy2(&entry.protocol, &exit_endpoint.region_code)?;
        let exit_pool_id = sync_single_endpoint_exit_pool_in_tx(
            &mut tx,
            exit_endpoint.id,
            &exit_endpoint.label,
            &exit_endpoint.region_code,
        )
        .await?;
        let access_line_inbound_config =
            reseal_access_entry_inbound_config_for_line(entry.inbound_config)?;
        let name = if input.name.trim().is_empty() {
            format!("{} / {}", entry.name, exit_endpoint.label)
        } else {
            required_admin_text(&input.name, "绑定节点名称", 128)?
        };
        let binding_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO access_entry_exit_bindings (
                access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight, remark
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id
            "#,
        )
        .bind(access_entry_id)
        .bind(exit_endpoint.id)
        .bind(exit_pool_id)
        .bind(&name)
        .bind(input.enabled)
        .bind(input.sort_weight.clamp(0, 1_000_000))
        .bind(optional_admin_text(&input.remark, 512))
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO access_lines (
                id, name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id,
                listen_host, listen_port, protocol, transport, user_uuid, server_name,
                public_key, short_id, enabled, region_code, region_name, region_flag,
                flow, udp_enabled, udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode,
                identity_mode, user_key_source, inbound_config, visibility_weight
            )
            VALUES (
                $1, $2, $3, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                $14, $15, $16, '', $17, $18, $19, $20, $21, $22,
                'credential', 'xray_email', $23, $24
            )
            "#,
        )
        .bind(binding_id)
        .bind(&name)
        .bind(entry.access_node_id)
        .bind(exit_endpoint.id)
        .bind(exit_pool_id)
        .bind(entry.listen_host)
        .bind(entry.listen_port)
        .bind(entry.protocol)
        .bind(entry.transport)
        .bind(entry.user_uuid)
        .bind(entry.server_name)
        .bind(entry.public_key)
        .bind(entry.short_id)
        .bind(entry.enabled && input.enabled)
        .bind(&exit_endpoint.region_code)
        .bind(&exit_endpoint.label)
        .bind(entry.flow)
        .bind(entry.udp_enabled)
        .bind(entry.udp_packet_encoding)
        .bind(entry.xhttp_path)
        .bind(entry.xhttp_host)
        .bind(entry.xhttp_mode)
        .bind(access_line_inbound_config)
        .bind(entry.sort_weight)
        .execute(&mut *tx)
        .await?;
        mark_access_node_dirty_in_tx(
            &mut tx,
            entry.access_node_id,
            "admin_bound_access_entry_exit",
        )
        .await?;
        tx.commit().await?;
        Ok(binding_id)
    }

    pub async fn replace_admin_line_group_binding_nodes(
        &self,
        line_group_id: Uuid,
        binding_node_ids: Vec<Uuid>,
    ) -> Result<usize, DbError> {
        ensure_unique_uuids(&binding_node_ids, "绑定节点重复")?;
        let mut tx = self.pool.begin().await?;
        ensure_line_group_exists_in_tx(&mut tx, line_group_id).await?;
        if !binding_node_ids.is_empty() {
            let count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM access_entry_exit_bindings WHERE id = ANY($1)",
            )
            .bind(&binding_node_ids)
            .fetch_one(&mut *tx)
            .await?;
            if count != binding_node_ids.len() as i64 {
                return Err(DbError::InvalidInput("绑定节点不存在".to_string()));
            }
        }
        sqlx::query("DELETE FROM line_group_binding_nodes WHERE line_group_id = $1")
            .bind(line_group_id)
            .execute(&mut *tx)
            .await?;
        for (index, binding_id) in binding_node_ids.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO line_group_binding_nodes (
                    line_group_id, entry_exit_binding_id, position
                )
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(line_group_id)
            .bind(binding_id)
            .bind(((index + 1) * 100) as i32)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM user_access_line_assignments WHERE line_group_id = $1")
            .bind(line_group_id)
            .execute(&mut *tx)
            .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_line_group_binding_nodes").await?;
        tx.commit().await?;
        Ok(binding_node_ids.len())
    }
}
