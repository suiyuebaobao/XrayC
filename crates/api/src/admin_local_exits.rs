//! 后台中转节点「本机出口就地编辑」与「多域名对账」模块。
//! 从 admin_nodes 拆出,避免节点 handler 文件继续膨胀超过 550 行硬上限。
//! 提供本机出口 GET/PUT/DELETE handler:列出/改端口/改域名/删一条 self_hosted 出口。
//! reconcile_node_domains 把请求里的 domains[] 对账进 node_domains(新增缺失、删除多余)。
//! 主域名(cert_domain/cf_domain)已由 store 单字段同步进 node_domains,这里只补齐其余并清理多余。
//! 域名归属/引用校验由 db crate 负责;被入口/出口引用的域名删除会被 store 拒绝并回友好 422。
//! handler 只做鉴权、入参转换、审计与响应包装,数据库事务全在 db crate。
//! 不在此回显出口真实凭据,只透传 store 已脱敏的清单 JSON。
//! 注释保持中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use super::*;

/// 列出某中转节点的全部本机出口线路(限 self_hosted),供前端就地编辑。
/// 透传 store 的脱敏清单 JSON,不展开出口凭据明文。
pub(crate) async fn list_local_exit_lines(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.list_node_local_exit_lines(access_node_id).await {
        Ok(lines) => Json(serde_json::json!({
            "success": true,
            "data": {"lines": lines}
        }))
        .into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 就地更新一条本机出口线路(改名称/地址/端口/启用/选中域名/协议配置)。
/// node_domain_id 三态:字段缺省=不改、显式 null=清空、UUID=设值;限 self_hosted 出口。
/// outbound_config/stream_config 透传进 store——改配置走 store 内护栏重校验后持久化,不绕护栏。
pub(crate) async fn update_local_exit_line(
    State(state): State<Arc<AppState>>,
    Path((access_node_id, exit_endpoint_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<UpdateLocalExitLineRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let node_domain_patch = body.node_domain_id.into_option_option();
    // 先取出审计用的「是否变更」标志,后续字段会被 move 进 store 入参。
    // outbound_config/stream_config 透传进 store 持久化(改配置在 store 内重跑出口护栏校验);
    // 审计只记客户端是否带了它们,不回显内容(redacted),守住凭据红线。
    let summary = serde_json::json!({
        "access_node_id": access_node_id,
        "fields": {
            "endpoint_name": body.endpoint_name.is_some(),
            "host": body.host.is_some(),
            "port": body.port.is_some(),
            "enabled": body.enabled.is_some(),
            "node_domain_id": node_domain_patch.is_some(),
            "outbound_config_requested": body.outbound_config.is_some(),
            "stream_config_requested": body.stream_config.is_some()
        },
        "sensitive_redacted": true
    });
    match pg
        .update_local_exit_line(
            exit_endpoint_id,
            AdminLocalExitLineUpdate {
                endpoint_name: body.endpoint_name,
                host: body.host,
                port: body.port,
                enabled: body.enabled,
                node_domain_id: node_domain_patch,
                outbound_config: body.outbound_config,
                stream_config: body.stream_config,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.local_exit_line.update",
                "exit_endpoint",
                Some(exit_endpoint_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true, "data": {"updated": true}})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 就地删除一条本机出口线路(限 self_hosted),同时清理对应自建出口资源。
pub(crate) async fn delete_local_exit_line(
    State(state): State<Arc<AppState>>,
    Path((access_node_id, exit_endpoint_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_local_exit_line(exit_endpoint_id).await {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.local_exit_line.delete",
                "exit_endpoint",
                Some(exit_endpoint_id),
                serde_json::json!({"access_node_id": access_node_id, "deleted": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": {"deleted": true}})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 把节点单字段主域名(cert_domain=direct 主、cf_domain=cf 主)并入请求 domains[],构造 create 对账的 desired 集。
/// 单字段主域名已在 domains[] 出现则不重复加(按域名去重);单字段为空则不加。
/// 仅 create 路径调用:保证 create_admin_access_node 与安装态 rebind 两条 store 路径落库一致。
pub(crate) fn merge_primary_domains_into(
    domains: &[NodeDomainInput],
    cert_domain: Option<&str>,
    cf_domain: Option<&str>,
    acme_email: Option<&str>,
) -> Vec<NodeDomainInput> {
    let mut desired: Vec<NodeDomainInput> = Vec::with_capacity(domains.len() + 2);
    let mut push_primary = |domain: Option<&str>, kind: &str| {
        let Some(domain) = domain.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let already = domains.iter().any(|item| item.domain.trim() == domain)
            || desired.iter().any(|item| item.domain == domain);
        if already {
            return;
        }
        desired.push(NodeDomainInput {
            domain: domain.to_string(),
            kind: kind.to_string(),
            is_primary: true,
            acme_email: acme_email.map(str::to_string),
            cf_cert_mode: None,
        });
    };
    push_primary(cert_domain, "direct");
    push_primary(cf_domain, "cf");
    // 再追加请求里显式给的域名(主域名若与单字段同名,上面已先占位,这里复制其余字段)。
    for item in domains {
        desired.push(NodeDomainInput {
            domain: item.domain.clone(),
            kind: item.kind.clone(),
            is_primary: item.is_primary,
            acme_email: item.acme_email.clone(),
            cf_cert_mode: item.cf_cert_mode.clone(),
        });
    }
    desired
}

/// 一键安装节点登记后的多域名对账:把预算好的主域名集落进 node_domains。
/// 安装态 rebind 路径不会自行同步主域名进 node_domains,这里补齐使其与后台手动创建一致。
/// 失败只记日志不上抛——节点已登记可用,域名缺失可后台补,不能因此把整次安装判失败。
pub(crate) async fn reconcile_one_click_node_domains(
    pg: &PgStore,
    node_id: Uuid,
    desired: &[NodeDomainInput],
) {
    if desired.is_empty() {
        return;
    }
    if let Err(err) = reconcile_node_domains(pg, node_id, desired).await {
        tracing::warn!(error = %err, "一键安装节点多域名对账失败(节点已登记,不阻断)");
    }
}

/// 把请求里的 domains[] 对账进 node_domains:补齐缺失域名、删除请求里没有的多余域名。
/// 主域名(cert_domain/cf_domain)由 store 单字段同步过,这里对它幂等跳过(domain 已存在则不重复新增)。
/// 删除多余域名时,若仍被入口/出口引用,store 会返回 Err,本函数把错误原样上抛由 handler 回 422。
/// desired 为空时直接返回(由 handler 决定是否调用),不在此判断空表语义。
pub(crate) async fn reconcile_node_domains(
    pg: &PgStore,
    node_id: Uuid,
    desired: &[NodeDomainInput],
) -> Result<(), DbError> {
    let existing = pg.list_node_domains(node_id).await?;
    // 新增:请求里有、库里(按 domain 去重)没有的域名,逐条 add_node_domain。
    // added 同时记本轮已加的名字,避免 desired 内重名(如单字段主域名同时出现在 domains[])重复 add 报错。
    let mut added: Vec<String> = Vec::new();
    for item in desired {
        let domain = item.domain.trim();
        if domain.is_empty() {
            continue;
        }
        let already =
            existing.iter().any(|row| row.domain == domain) || added.iter().any(|d| d == domain);
        if already {
            // 主域名等已存在的行幂等跳过,避免 add_node_domain 的重复域名 Err。
            continue;
        }
        added.push(domain.to_string());
        pg.add_node_domain(
            node_id,
            AddNodeDomainInput {
                domain: domain.to_string(),
                kind: item.kind.clone(),
                cf_cert_mode: item.cf_cert_mode.clone(),
                acme_email: item.acme_email.clone(),
                is_primary: item.is_primary,
            },
        )
        .await?;
    }
    // 删除:库里有、请求里没有的多余域名;被引用则 store 拒绝,错误上抛回 422。
    for row in &existing {
        let kept = desired
            .iter()
            .any(|item| item.domain.trim() == row.domain && !row.domain.is_empty());
        if !kept {
            pg.delete_node_domain(row.id).await?;
        }
    }
    Ok(())
}
