//! 后台接入节点模块。
//! 本文件由原 API 入口按路由域拆分而来。
//! 只移动 handler 与相关 helper，不改变路由、字段和状态码。
//! 模块保持 crate 内可见，供 lib.rs 路由装配使用。
//! 响应体、cookie、token 与审计摘要沿用原实现。
//! 数据库访问仍通过既有 PgStore 方法完成。
//! 内存模式回退逻辑保持原有分支。
//! 新增代码控制在 500 行以内便于审阅。
//! 中文注释位于文件前十行满足仓库约束。
//! 请勿在此写入部署主机、密钥或其它敏感信息。

use super::*;

const INSTALLED_AGENT_AUTH_CODE_PREFIX: &str = "xrayc-agent-v1:";

pub(crate) struct AccessNodeAuthCode {
    pub(crate) installed_node_id: Option<Uuid>,
    pub(crate) agent_token: String,
}

pub(crate) async fn create_access_node(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateAccessNodeRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let auth_code = match optional_request_text(body.agent_token, "agent_token", 512) {
        Ok(Some(token)) => match parse_access_node_auth_code(&token) {
            Ok(value) => value,
            Err(err) => return unprocessable(err).into_response(),
        },
        Ok(None) => return unprocessable("鉴权码不能为空").into_response(),
        Err(err) => return unprocessable(err).into_response(),
    };
    let public_port = match require_nonzero_port(body.public_port.unwrap_or(443), "public_port") {
        Ok(port) => port,
        Err(err) => return unprocessable(err).into_response(),
    };
    let remark = match body.remark {
        Some(value) => match validate_request_text(value, "remark", 512) {
            Ok(value) => value,
            Err(err) => return unprocessable(err).into_response(),
        },
        None => String::new(),
    };
    let ssh_host = match body.ssh_host {
        Some(value) => match validate_request_text(value, "ssh_host", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    // 节点多模式 CF 字段:可选域名/邮箱裁剪校验,空白归一为 None。
    let cert_domain = match optional_request_text(body.cert_domain, "cert_domain", 255) {
        Ok(value) => value,
        Err(err) => return unprocessable(err).into_response(),
    };
    let acme_email = match optional_request_text(body.acme_email, "acme_email", 255) {
        Ok(value) => value,
        Err(err) => return unprocessable(err).into_response(),
    };
    let cf_domain = match optional_request_text(body.cf_domain, "cf_domain", 255) {
        Ok(value) => value,
        Err(err) => return unprocessable(err).into_response(),
    };
    // IP 直连地址:可选,裁剪空白后透传;cf_cert_mode 由 store 按 cf_domain 派生(DTO 字段仅作兼容入参)。
    let ip_direct_address =
        match optional_request_text(body.ip_direct_address, "ip_direct_address", 255) {
            Ok(value) => value,
            Err(err) => return unprocessable(err).into_response(),
        };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "public_host": body.public_host.clone(),
        "public_port": public_port,
        "ssh_host_present": ssh_host.as_ref().is_some_and(|value| !value.is_empty()),
        "remark_present": !remark.is_empty(),
        "cert_domain_present": cert_domain.is_some(),
        "cf_enabled": body.cf_enabled,
        "cf_domain_present": cf_domain.is_some(),
        "ip_direct_address_present": ip_direct_address.is_some(),
        // cf_cert_mode 实际由 store 按 cf_domain 派生;此处仅记录客户端是否带了该兼容入参。
        "cf_cert_mode_requested": body.cf_cert_mode.is_some(),
        "installed_agent_auth_code": auth_code.installed_node_id.is_some(),
        "agent_token_redacted": true
    });
    // 先克隆单字段主域名/邮箱,供创建后的多域名对账用(下方 input 会 move 掉原值)。
    let primary_cert_domain = cert_domain.clone();
    let primary_cf_domain = cf_domain.clone();
    let primary_acme_email = acme_email.clone();
    let input = AdminAccessNodeInput {
        name: body.name,
        public_host: body.public_host,
        public_port,
        agent_token: auth_code.agent_token,
        remark,
        cert_domain,
        acme_email,
        // cf_enabled 入参仅占位,实际由 store 按 cf_domain 派生。
        cf_enabled: body.cf_enabled,
        cf_domain,
        ip_direct_address,
    };
    let created = if let Some(installed_node_id) = auth_code.installed_node_id {
        pg.rebind_admin_installed_access_node(installed_node_id, input)
            .await
    } else {
        pg.create_admin_access_node(input).await
    };
    match created {
        Ok(id) => {
            if ssh_host.is_some() {
                if let Err(err) = pg
                    .update_admin_access_node(
                        id,
                        AdminAccessNodeUpdate {
                            ssh_host,
                            ..Default::default()
                        },
                    )
                    .await
                {
                    return unprocessable(err).into_response();
                }
            }
            // 多域名对账(多域名 Phase 6):创建对齐「单字段主域名 ∪ domains[]」。
            // create_admin_access_node 路径会自行同步 cert_domain/cf_domain 进 node_domains,
            // 但安装态 rebind 路径不会;这里统一把单字段主域名并入 desired,确保两条路径落库一致(按域名去重幂等)。
            let desired = merge_primary_domains_into(
                &body.domains,
                primary_cert_domain.as_deref(),
                primary_cf_domain.as_deref(),
                primary_acme_email.as_deref(),
            );
            if let Err(err) = reconcile_node_domains(pg, id, &desired).await {
                return unprocessable(err).into_response();
            }
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.create",
                "access_node",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) fn parse_access_node_auth_code(value: &str) -> Result<AccessNodeAuthCode, String> {
    if let Some(payload) = value.strip_prefix(INSTALLED_AGENT_AUTH_CODE_PREFIX) {
        let (node_id, agent_token) = payload
            .split_once(':')
            .ok_or_else(|| "节点鉴权码格式无效".to_string())?;
        let installed_node_id =
            Uuid::parse_str(node_id).map_err(|_| "节点鉴权码里的节点 ID 格式无效".to_string())?;
        let agent_token = validate_request_text(agent_token.to_string(), "agent_token", 255)?;
        if agent_token.is_empty() {
            return Err("鉴权码不能为空".to_string());
        }
        return Ok(AccessNodeAuthCode {
            installed_node_id: Some(installed_node_id),
            agent_token,
        });
    }

    Ok(AccessNodeAuthCode {
        installed_node_id: None,
        agent_token: validate_request_text(value.to_string(), "agent_token", 255)?,
    })
}

pub(crate) async fn update_access_node(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UpdateAccessNodeRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let remark = match body.remark {
        Some(value) => match validate_request_text(value, "remark", 512) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    let ssh_host = match body.ssh_host {
        Some(value) => match validate_request_text(value, "ssh_host", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    // 节点多模式 CF 字段:仅在请求带有该字段时透传,空值由 store 归一为 NULL。
    let cert_domain = match body.cert_domain {
        Some(value) => match validate_request_text(value, "cert_domain", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    let acme_email = match body.acme_email {
        Some(value) => match validate_request_text(value, "acme_email", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    let cf_domain = match body.cf_domain {
        Some(value) => match validate_request_text(value, "cf_domain", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    // IP 直连地址:None 表示本次不更新;cf_cert_mode 由 store 在 cf_domain 变更时派生(DTO 字段仅兼容)。
    let ip_direct_address = match body.ip_direct_address {
        Some(value) => match validate_request_text(value, "ip_direct_address", 255) {
            Ok(value) => Some(value),
            Err(err) => return unprocessable(err).into_response(),
        },
        None => None,
    };
    // domains[] 是节点域名的唯一真值源:按「节点单字段主域名 ∪ 请求 domains[]」算好对账目标,
    // 保证 access_nodes 单字段主域名与 node_domains 始终一致(merge 内部会过滤空主域名)。
    // 必须在 cert_domain/cf_domain/acme_email 被 move 进下方节点单字段更新前算(merge 只借引用)。
    let desired_domains = merge_primary_domains_into(
        &body.domains,
        cert_domain.as_deref(),
        cf_domain.as_deref(),
        acme_email.as_deref(),
    );
    // 是否有任一节点单字段需要更新:仅传 domains[] 时不调 update_admin_access_node(它要求至少一个字段),
    // 避免「没有可更新的中转节点字段」误报;否则正常走单字段更新。
    let has_node_field_update = body.name.is_some()
        || body.public_host.is_some()
        || body.public_port.is_some()
        || ssh_host.is_some()
        || remark.is_some()
        || cert_domain.is_some()
        || acme_email.is_some()
        || body.cf_enabled.is_some()
        || cf_domain.is_some()
        || ip_direct_address.is_some();
    if has_node_field_update {
        if let Err(err) = pg
            .update_admin_access_node(
                access_node_id,
                AdminAccessNodeUpdate {
                    name: body.name,
                    public_host: body.public_host,
                    public_port: body.public_port,
                    ssh_host,
                    remark,
                    cert_domain,
                    acme_email,
                    cf_enabled: body.cf_enabled,
                    cf_domain,
                    ip_direct_address,
                },
            )
            .await
        {
            return unprocessable(err).into_response();
        }
    } else if body.domains.is_empty() {
        // 既无可更新的节点单字段、也没带任何 domains[]:确无可改内容,回 422。
        return unprocessable("没有可更新的中转节点字段".to_string()).into_response();
    }
    // 多域名对账:domains[] 是节点域名唯一真值源,一律以请求清单为准对账 node_domains
    // (新增缺失、删除多余;传空清单即删光全部未被入口/出口引用的域名)。
    // 不再保留旧「空=不动」兼容分支——此前用 Vec 区分不了「没传」与「传了空[]」,删最后一个域名时
    // 列表变空被误判成不改、域名永远删不掉;现在前端始终回传完整 domains[],空清单是明确的「清空」语义。
    if let Err(err) = reconcile_node_domains(pg, access_node_id, &desired_domains).await {
        return unprocessable(err).into_response();
    }
    record_admin_audit(
        pg,
        &claims,
        &headers,
        "access_node.update",
        "access_node",
        Some(access_node_id),
        // cf_cert_mode 由 store 按 cf_domain 派生;审计只记录客户端是否带了该兼容入参。
        serde_json::json!({
            "updated": true,
            "cf_cert_mode_requested": body.cf_cert_mode.is_some(),
            "domain_count": body.domains.len()
        }),
    )
    .await;
    Json(serde_json::json!({"success": true, "data": {"updated": true}})).into_response()
}

pub(crate) async fn delete_access_node(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    delete_access_nodes_with_ids(state, headers, vec![access_node_id], Some(access_node_id)).await
}

pub(crate) async fn renew_access_node_tls(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.request_access_node_tls_renewal(access_node_id).await {
        Ok(result) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.tls_renew.request",
                "access_node",
                Some(access_node_id),
                serde_json::json!({
                    "request_id": result.request_id,
                    "domain_count": result.domains.len(),
                }),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": result})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn reboot_access_node(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // 管理员触发整机重启:只记待执行请求,经心跳下发给 agent 由其安全自检后执行;
    // api 侧不直接 SSH、不存凭据(§13)。重启会导致节点整机停机数分钟,前端须二次确认。
    match pg.request_access_node_reboot(access_node_id).await {
        Ok(result) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.reboot.request",
                "access_node",
                Some(access_node_id),
                serde_json::json!({ "request_id": result.request_id }),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": result})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn batch_delete_access_nodes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<BatchDeleteAccessNodesRequest>,
) -> Response {
    delete_access_nodes_with_ids(state, headers, body.access_node_ids, None).await
}

async fn delete_access_nodes_with_ids(
    state: Arc<AppState>,
    headers: HeaderMap,
    access_node_ids: Vec<Uuid>,
    audit_resource_id: Option<Uuid>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_access_nodes(access_node_ids).await {
        Ok(result) => {
            let summary = serde_json::to_value(&result).unwrap_or_else(|_| serde_json::json!({}));
            record_admin_audit(
                pg,
                &claims,
                &headers,
                if result.deleted_node_count > 1 {
                    "access_node.batch_delete"
                } else {
                    "access_node.delete"
                },
                "access_node",
                audit_resource_id,
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true, "data": result})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn create_local_exit_lines(
    State(state): State<Arc<AppState>>,
    Path(access_node_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<CreateLocalExitLinesRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .create_admin_local_exit_lines(
            access_node_id,
            AdminLocalExitLinesInput {
                lines: body
                    .lines
                    .into_iter()
                    .map(|line| AdminLocalExitLineInput {
                        resource_name: line.resource_name,
                        endpoint_name: line.endpoint_name,
                        region_code: line.region_code,
                        outbound_type: line.outbound_type,
                        network_mode: line.network_mode,
                        host: line.host,
                        port: line.port,
                        outbound_config: line.outbound_config,
                        stream_config: line.stream_config,
                        probe_config: line.probe_config,
                        enabled: line.enabled,
                        node_domain_id: line.node_domain_id,
                    })
                    .collect(),
            },
        )
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_node.local_exit_lines.create",
                "access_node",
                Some(access_node_id),
                serde_json::json!({"line_count": data["created_count"]}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}
