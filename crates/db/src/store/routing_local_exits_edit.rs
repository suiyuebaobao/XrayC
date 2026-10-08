//! 本机出口就地编辑/删除/列出(多域名 Phase 4)。
//! 从 routing_local_exits 拆出,避免创建文件继续膨胀超过 550 行硬上限。
//! 只操作 ownership=self_hosted 的出口端点:其他来源出口由出口管理页统一维护,本模块拒绝。
//! 更新/删除后置所属节点 config_dirty(desired_config_hash 清空),让节点重新拉取配置。
//! list_node_local_exit_lines 列该节点 self_hosted 出口,供 API/前端就地编辑;不回显出口真实凭据。
//! 所有写操作走单事务:先按 endpoint 反查 self_hosted 归属与所属节点,再改端点、置节点 dirty。
//! 域名选择改写 exit_endpoints.node_domain_id;node_domain_id 内层 None 表示显式清空。
//! 不在此触碰 access_entries / line_groups / 计费,纯出口端点维护。
//! 注释保持中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use super::dirty::mark_access_node_dirty_in_tx;
use super::routing_local_exits::{
    local_exit_stream_config, node_cf_domains_in_tx, selected_local_exit_domain_kind_in_tx,
    validate_local_exit_cert_domain, validate_local_exit_host_not_cf,
    validate_local_exit_network_mode, validate_local_exit_network_security,
};
use super::routing_local_exits_fill::fill_local_exit_outbound_config;
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    /// 就地更新一条本机出口线路(限 ownership=self_hosted)。
    /// 各字段 None 时不改;改完置所属节点 config_dirty。
    /// 端点不属于 self_hosted 出口或不存在时返回 Err。
    pub async fn update_local_exit_line(
        &self,
        endpoint_id: Uuid,
        update: AdminLocalExitLineUpdate,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        // 反查端点所属:必须是 self_hosted 且挂在某节点下;顺带锁行避免并发改写。
        let access_node_id = self_hosted_exit_node_id_in_tx(&mut tx, endpoint_id).await?;

        // node_domain_id 若要改(外层 Some),校验内层 Some 的域名属于本节点;内层 None 显式清空。
        if let Some(node_domain_id) = update.node_domain_id {
            ensure_node_domain_belongs_in_tx(&mut tx, access_node_id, node_domain_id).await?;
        }

        let endpoint_name = match update.endpoint_name.as_deref() {
            Some(raw) => Some(required_admin_text(raw, "本机出口协议档名称", 128)?),
            None => None,
        };
        let host = match update.host.as_deref() {
            Some(raw) => Some(required_admin_text(raw, "本机出口线路地址", 255)?),
            None => None,
        };
        // 改 host 时同样过 CF 域名护栏(编辑不绕护栏,Bug ④):命中本节点任一 CF 域名直接拒。
        if let Some(new_host) = host.as_deref() {
            let cf_domains = node_cf_domains_in_tx(&mut tx, access_node_id).await?;
            validate_local_exit_host_not_cf(new_host, &cf_domains)?;
        }
        if let Some(port) = update.port {
            if port == 0 {
                return Err(DbError::InvalidInput(
                    "本机出口线路端口必须在 1-65535 之间".to_string(),
                ));
            }
        }

        // 改协议配置必须重新校验(编辑不绕护栏):outbound_config/stream_config 任一为 Some 时,
        // 反查该端点已存库的 outbound_type、当前 host/node_domain_id/stream_config,
        // 用最终生效的 host + 最终生效的 node_domain_id(本次改的或原值)重跑出口护栏后才落库。
        let (filled_outbound_config, next_stream_config) =
            prepare_local_exit_config_edit(&mut tx, access_node_id, endpoint_id, &update, &host)
                .await?;

        // COALESCE 写法:传 NULL 的列保留原值;node_domain_id 用 $6::uuid + $7 标志区分「不改」与「清空」。
        // outbound_config/stream_config 用 $8/$9::jsonb,None 时为 NULL 保留原值。
        sqlx::query(
            r#"
            UPDATE exit_endpoints
            SET name = COALESCE($2, name),
                host = COALESCE($3, host),
                port = COALESCE($4, port),
                enabled = COALESCE($5, enabled),
                node_domain_id = CASE WHEN $7 THEN $6 ELSE node_domain_id END,
                outbound_config = COALESCE($8::jsonb, outbound_config),
                stream_config = COALESCE($9::jsonb, stream_config)
            WHERE id = $1
            "#,
        )
        .bind(endpoint_id)
        .bind(endpoint_name)
        .bind(host)
        .bind(update.port.map(i32::from))
        .bind(update.enabled)
        .bind(update.node_domain_id.flatten())
        .bind(update.node_domain_id.is_some())
        .bind(filled_outbound_config)
        .bind(next_stream_config)
        .execute(&mut *tx)
        .await?;

        let pool_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_pool_id FROM exit_pool_members WHERE exit_endpoint_id = $1",
        )
        .bind(endpoint_id)
        .fetch_all(&mut *tx)
        .await?;
        super::line_binding::prune_unusable_exit_pool_lines_in_tx(&mut tx, &pool_ids).await?;
        super::dirty::mark_nodes_dirty_for_endpoint_in_tx(
            &mut tx,
            endpoint_id,
            "local_exit_lines_changed",
        )
        .await?;
        mark_access_node_dirty_in_tx(&mut tx, access_node_id, "local_exit_lines_changed").await?;
        tx.commit().await?;
        Ok(())
    }

    /// 就地删除一条本机出口线路(限 ownership=self_hosted)。
    /// 同时删除对应的 exit_resource(自建出口一资源一端点);改完置所属节点 config_dirty。
    pub async fn delete_local_exit_line(&self, endpoint_id: Uuid) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let access_node_id = self_hosted_exit_node_id_in_tx(&mut tx, endpoint_id).await?;
        self.delete_exit_endpoint_in_tx(&mut tx, endpoint_id)
            .await?;
        mark_access_node_dirty_in_tx(&mut tx, access_node_id, "local_exit_lines_changed").await?;
        tx.commit().await?;
        Ok(())
    }

    /// 列出某节点全部 self_hosted 出口线路,供 API/前端就地编辑。
    /// 只回显端点身份/协议/地址/端口/启用与选中域名,绝不回显凭据明文(凭据在 outbound_config,这里不展开)。
    pub async fn list_node_local_exit_lines(&self, node_id: Uuid) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, LocalExitLineListRow>(
            r#"
            SELECT e.id AS exit_endpoint_id,
                   e.exit_resource_id,
                   r.name AS resource_name,
                   e.name AS endpoint_name,
                   e.outbound_type::text AS outbound_type,
                   e.host,
                   e.port,
                   e.enabled,
                   r.enabled AS resource_enabled,
                   e.node_domain_id,
                   nd.domain AS node_domain,
                   nd.kind AS node_domain_kind
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            LEFT JOIN node_domains nd ON nd.id = e.node_domain_id
            WHERE r.ownership = 'self_hosted'
              AND r.access_node_id = $1
            ORDER BY e.created_at DESC, r.name ASC, e.name ASC, e.id ASC
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                json!({
                    "exit_endpoint_id": row.exit_endpoint_id,
                    "exit_resource_id": row.exit_resource_id,
                    "resource_name": row.resource_name,
                    "endpoint_name": row.endpoint_name,
                    "outbound_type": row.outbound_type,
                    "host": row.host,
                    "port": row.port.clamp(0, u16::MAX as i32),
                    "enabled": row.enabled,
                    "resource_enabled": row.resource_enabled,
                    "node_domain_id": row.node_domain_id,
                    "node_domain": row.node_domain_id.map(|_| {
                        json!({
                            "id": row.node_domain_id,
                            "domain": row.node_domain,
                            "kind": row.node_domain_kind,
                        })
                    })
                })
            })
            .collect::<Vec<_>>();
        Ok(json!(items))
    }
}

/// 本机出口清单行映射。
#[derive(Debug, sqlx::FromRow)]
struct LocalExitLineListRow {
    exit_endpoint_id: Uuid,
    exit_resource_id: Uuid,
    resource_name: String,
    endpoint_name: String,
    outbound_type: String,
    host: String,
    port: i32,
    enabled: bool,
    resource_enabled: bool,
    node_domain_id: Option<Uuid>,
    node_domain: Option<String>,
    node_domain_kind: Option<String>,
}

/// 在事务内反查端点所属节点 id,并强制要求该端点属于 ownership=self_hosted 出口。
/// 非 self_hosted / 无所属节点 / 不存在,统一返回业务错误(不暴露内部细节)。
async fn self_hosted_exit_node_id_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    endpoint_id: Uuid,
) -> Result<Uuid, DbError> {
    let row = sqlx::query_as::<_, (Option<Uuid>, String)>(
        r#"
        SELECT r.access_node_id, r.ownership
        FROM exit_endpoints e
        JOIN exit_resources r ON r.id = e.exit_resource_id
        WHERE e.id = $1
        FOR UPDATE OF e
        "#,
    )
    .bind(endpoint_id)
    .fetch_optional(&mut **tx)
    .await?;
    match row {
        Some((Some(node_id), ownership)) if ownership == "self_hosted" => Ok(node_id),
        Some(_) => Err(DbError::InvalidInput(
            "该出口不是本机出口(self_hosted),请在出口管理页维护".to_string(),
        )),
        None => Err(DbError::InvalidInput(format!(
            "本机出口线路不存在: {endpoint_id}"
        ))),
    }
}

/// 在事务内校验选中域名属于本节点(node_domain_id 内层 None 表示清空选择,直接放行)。
async fn ensure_node_domain_belongs_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    node_domain_id: Option<Uuid>,
) -> Result<(), DbError> {
    let Some(domain_id) = node_domain_id else {
        return Ok(());
    };
    let owner =
        sqlx::query_scalar::<_, Uuid>("SELECT access_node_id FROM node_domains WHERE id = $1")
            .bind(domain_id)
            .fetch_optional(&mut **tx)
            .await?;
    match owner {
        Some(owner_id) if owner_id == access_node_id => Ok(()),
        Some(_) => Err(DbError::InvalidInput(
            "选中的域名不属于该中转节点,请重新选择本节点的域名".to_string(),
        )),
        None => Err(DbError::InvalidInput(
            "选中的节点域名不存在,请刷新后重新选择".to_string(),
        )),
    }
}

/// 端点当前协议/地址/选中域名/网络配置读模型,供就地改配置时复用最终生效值。
#[derive(Debug, sqlx::FromRow)]
struct LocalExitConfigRow {
    outbound_type: String,
    host: String,
    node_domain_id: Option<Uuid>,
    stream_config: Value,
    outbound_config: Value,
}

/// 就地改配置时的校验与填充(编辑不绕护栏)。
///
/// outbound_config/stream_config 都为 None 时直接返回 (None, None),不触碰协议配置。
/// 任一为 Some 时,反查该端点已存库的 outbound_type 与当前 host/node_domain_id/stream_config,
/// 用「最终生效 host」(本次改的或原值)与「最终生效 node_domain_id」(本次改的或原值)重跑出口护栏:
/// fill_local_exit_outbound_config + validate_exit_endpoint_protocol_config +
/// validate_local_exit_cert_domain(按选中域名,纯 IP/无证书节点仍拦要证书协议) +
/// validate_local_exit_network_security(按最终网络模式)。stream_config 按网络模式补默认值后落库。
async fn prepare_local_exit_config_edit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    endpoint_id: Uuid,
    update: &AdminLocalExitLineUpdate,
    host_override: &Option<String>,
) -> Result<(Option<Value>, Option<Value>), DbError> {
    // 两者都不改:不读库、不校验,保留原值(交由 COALESCE 处理)。
    if update.outbound_config.is_none() && update.stream_config.is_none() {
        return Ok((None, None));
    }

    let current = sqlx::query_as::<_, LocalExitConfigRow>(
        r#"
        SELECT outbound_type::text AS outbound_type, host, node_domain_id, stream_config, outbound_config
        FROM exit_endpoints
        WHERE id = $1
        "#,
    )
    .bind(endpoint_id)
    .fetch_one(&mut **tx)
    .await?;

    // 最终生效 host:本次改的优先,否则用原值——Reality 默认 dest/sni 与证书锚定都要对齐它。
    let effective_host = host_override.clone().unwrap_or(current.host);
    // 最终生效 node_domain_id:外层 Some 用本次值(内层 None=清空),外层 None 用原值。
    let effective_domain_id = match update.node_domain_id {
        Some(value) => value,
        None => current.node_domain_id,
    };
    // 选中域名的 kind 决定 cert 护栏分流;cert_domain 决定无选时的节点级回退。
    let selected_kind =
        selected_local_exit_domain_kind_in_tx(tx, access_node_id, effective_domain_id).await?;
    let node_has_cert_domain = node_has_cert_domain_in_tx(tx, access_node_id).await?;

    // 最终生效网络模式:改 stream_config 时按其网络模式;否则沿用原 stream_config 里的 network_mode。
    let stream_mode_source = update
        .stream_config
        .as_ref()
        .or(Some(&current.stream_config));
    let network_mode = local_exit_network_mode_from_stream(
        &current.outbound_type,
        stream_mode_source,
        &current.stream_config,
    )?;

    // outbound_config:fill 补默认/凭据 → 协议字段校验 → 证书域名护栏 → 网络安全护栏,通过后才落库。
    let filled_outbound = match update.outbound_config.clone() {
        Some(raw) => {
            let filled =
                fill_local_exit_outbound_config(&current.outbound_type, &effective_host, raw)?;
            validate_exit_endpoint_protocol_config(&current.outbound_type, &filled)?;
            validate_local_exit_cert_domain(
                &current.outbound_type,
                &filled,
                node_has_cert_domain,
                selected_kind.as_deref(),
            )?;
            validate_local_exit_network_security(&current.outbound_type, network_mode, &filled)?;
            Some(filled)
        }
        None => None,
    };

    // stream_config:按最终网络模式补 network/network_mode 默认值后落库。
    // 只改网络模式(没动 outbound_config)时,也要用最终生效配置(此处沿用原 outbound_config)
    // 重跑网络安全护栏——否则 VLESS Reality 能被偷偷切到 udp/xudp 绕过 TCP-only 约束。
    let next_stream = match update.stream_config.clone() {
        Some(raw) => {
            let effective_outbound = filled_outbound.as_ref().unwrap_or(&current.outbound_config);
            validate_local_exit_network_security(
                &current.outbound_type,
                network_mode,
                effective_outbound,
            )?;
            Some(local_exit_stream_config(raw, network_mode)?)
        }
        None => None,
    };

    Ok((filled_outbound, next_stream))
}

/// 在事务内判定节点是否配了可签证书的直连域名(cert_domain 去空白后非空)。
async fn node_has_cert_domain_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
) -> Result<bool, DbError> {
    let cert_domain = sqlx::query_scalar::<_, Option<String>>(
        "SELECT cert_domain FROM access_nodes WHERE id = $1",
    )
    .bind(access_node_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(cert_domain
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false))
}

/// 从 stream_config 解析本机出口网络模式并校验:优先读源里的 network_mode,缺省回退默认。
/// HY2 固定 udp;其余默认 tcp。校验交给 validate_local_exit_network_mode 统一口径。
fn local_exit_network_mode_from_stream(
    outbound_type: &str,
    source: Option<&Value>,
    fallback: &Value,
) -> Result<&'static str, DbError> {
    let read_mode = |value: &Value| -> String {
        value
            .get("network_mode")
            .and_then(Value::as_str)
            .or_else(|| value.get("network").and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    // 改 stream_config 时读新值的 network_mode;只改 outbound_config 时沿用原 stream_config。
    let raw_mode = source.map(read_mode).unwrap_or_else(|| read_mode(fallback));
    validate_local_exit_network_mode(outbound_type, &raw_mode)
}
