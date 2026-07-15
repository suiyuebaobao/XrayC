//! 后台入口管理模块。
//! 本模块提供入口、入口出口绑定节点和分组绑定节点接口。
//! 入口绑定节点是订阅节点的真实来源。
//! 旧 access-lines 接口仍保留兼容，本模块只承载新主路径。
//! handler 只做请求转换、鉴权、审计和响应包装。
//! 数据库事务、兼容 access_lines 镜像和脏标记由 db crate 处理。
//! 列表接口返回数据库 JSON 读模型，避免在 API 层重复拼装。
//! 请求内真实出口敏感配置不在本模块接收。
//! 中文注释位于文件前十行满足仓库约束。
//! 请勿在源码或测试示例写入真实服务器信息。

use super::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct AccessEntryRequest {
    pub(crate) access_node_id: Uuid,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) listen_host: String,
    pub(crate) listen_port: u16,
    pub(crate) protocol: String,
    #[serde(default)]
    pub(crate) transport: String,
    #[serde(default)]
    pub(crate) security: String,
    #[serde(default)]
    pub(crate) server_name: String,
    #[serde(default)]
    pub(crate) ws_path: String,
    #[serde(default)]
    pub(crate) ws_host: String,
    #[serde(default)]
    pub(crate) cdn_enabled: bool,
    #[serde(default)]
    pub(crate) cdn_provider: String,
    #[serde(default)]
    pub(crate) cdn_hostname: String,
    #[serde(default)]
    pub(crate) cdn_server: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) sort_weight: i32,
    // 选中的节点域名 id(多域名 Phase 4):None 表示免证书(回退节点单域名护栏)。
    #[serde(default)]
    pub(crate) node_domain_id: Option<Uuid>,
    // 量子加密(VLESS native encryption,后量子)开关:仅非 Reality 的 VLESS 生效。
    #[serde(default)]
    pub(crate) vless_quantum_encryption: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccessEntryExitBindingRequest {
    pub(crate) exit_endpoint_id: Uuid,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) sort_weight: i32,
    #[serde(default)]
    pub(crate) remark: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplaceLineGroupBindingNodesRequest {
    #[serde(default)]
    pub(crate) binding_node_ids: Vec<Uuid>,
}

pub(crate) async fn list_access_entries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.list_admin_access_entries_json().await {
        Ok(entries) => Json(serde_json::json!({
            "success": true,
            "data": {"access_entries": entries}
        }))
        .into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn create_access_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AccessEntryRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // 控制面橙云自动识别:对外地址命中 CF 段则预填 cdn_enabled/cdn_provider(解析失败不阻断)。
    let detection = entry_cdn_detect::detect_cdn_for_entry(
        &body.cdn_hostname,
        &body.listen_host,
        body.cdn_enabled,
        &body.cdn_provider,
    )
    .await;
    let summary = serde_json::json!({
        "access_node_id": body.access_node_id,
        "name": body.name.clone(),
        "listen_port": body.listen_port,
        "protocol": body.protocol.clone(),
        "transport": body.transport.clone(),
        "security": body.security.clone(),
        "cdn_enabled": detection.cdn_enabled,
        "cdn_provider": detection.cdn_provider.clone()
    });
    match pg
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: body.access_node_id,
            name: body.name,
            listen_host: body.listen_host,
            listen_port: body.listen_port,
            protocol: body.protocol,
            transport: body.transport,
            security: body.security,
            server_name: body.server_name,
            ws_path: body.ws_path,
            ws_host: body.ws_host,
            cdn_enabled: detection.cdn_enabled,
            cdn_provider: detection.cdn_provider,
            cdn_hostname: body.cdn_hostname,
            cdn_server: body.cdn_server,
            enabled: body.enabled,
            sort_weight: body.sort_weight,
            node_domain_id: body.node_domain_id,
            vless_quantum_encryption: body.vless_quantum_encryption,
        })
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry.create",
                "access_entry",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_access_entry(
    State(state): State<Arc<AppState>>,
    Path(access_entry_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AccessEntryRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // 控制面橙云自动识别:对外地址命中 CF 段则预填 cdn_enabled/cdn_provider(解析失败不阻断)。
    let detection = entry_cdn_detect::detect_cdn_for_entry(
        &body.cdn_hostname,
        &body.listen_host,
        body.cdn_enabled,
        &body.cdn_provider,
    )
    .await;
    let summary = serde_json::json!({
        "access_entry_id": access_entry_id,
        "name": body.name.clone(),
        "listen_port": body.listen_port,
        "protocol": body.protocol.clone(),
        "transport": body.transport.clone(),
        "security": body.security.clone(),
        "cdn_enabled": detection.cdn_enabled,
        "cdn_provider": detection.cdn_provider.clone()
    });
    match pg
        .update_admin_access_entry(
            access_entry_id,
            AdminAccessEntryInput {
                access_node_id: body.access_node_id,
                name: body.name,
                listen_host: body.listen_host,
                listen_port: body.listen_port,
                protocol: body.protocol,
                transport: body.transport,
                security: body.security,
                server_name: body.server_name,
                ws_path: body.ws_path,
                ws_host: body.ws_host,
                cdn_enabled: detection.cdn_enabled,
                cdn_provider: detection.cdn_provider,
                cdn_hostname: body.cdn_hostname,
                cdn_server: body.cdn_server,
                enabled: body.enabled,
                sort_weight: body.sort_weight,
                node_domain_id: body.node_domain_id,
                vless_quantum_encryption: body.vless_quantum_encryption,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry.update",
                "access_entry",
                Some(access_entry_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_access_entry(
    State(state): State<Arc<AppState>>,
    Path(access_entry_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_access_entry(access_entry_id).await {
        Ok(deleted) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry.delete",
                "access_entry",
                Some(access_entry_id),
                serde_json::json!({"deleted": deleted}),
            )
            .await;
            Json(serde_json::json!({"success": true, "deleted": deleted})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn list_access_entry_exit_bindings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.list_admin_access_entry_exit_bindings_json().await {
        Ok(bindings) => Json(serde_json::json!({
            "success": true,
            "data": {"access_entry_exit_bindings": bindings}
        }))
        .into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn create_access_entry_exit_binding(
    State(state): State<Arc<AppState>>,
    Path(access_entry_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AccessEntryExitBindingRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "access_entry_id": access_entry_id,
        "exit_endpoint_id": body.exit_endpoint_id,
        "name": body.name.clone(),
        "enabled": body.enabled
    });
    match pg
        .create_admin_access_entry_exit_binding(
            access_entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id: body.exit_endpoint_id,
                name: body.name,
                enabled: body.enabled,
                sort_weight: body.sort_weight,
                remark: body.remark,
            },
        )
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry_exit_binding.create",
                "access_entry_exit_binding",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_access_entry_exit_binding(
    State(state): State<Arc<AppState>>,
    Path(binding_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AccessEntryExitBindingRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "binding_id": binding_id,
        "exit_endpoint_id": body.exit_endpoint_id,
        "name": body.name.clone(),
        "enabled": body.enabled
    });
    match pg
        .update_admin_access_entry_exit_binding(
            binding_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id: body.exit_endpoint_id,
                name: body.name,
                enabled: body.enabled,
                sort_weight: body.sort_weight,
                remark: body.remark,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry_exit_binding.update",
                "access_entry_exit_binding",
                Some(binding_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_access_entry_exit_binding(
    State(state): State<Arc<AppState>>,
    Path(binding_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_access_entry_exit_binding(binding_id).await {
        Ok(deleted) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_entry_exit_binding.delete",
                "access_entry_exit_binding",
                Some(binding_id),
                serde_json::json!({"deleted": deleted}),
            )
            .await;
            Json(serde_json::json!({"success": true, "deleted": deleted})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn replace_line_group_binding_nodes(
    State(state): State<Arc<AppState>>,
    Path(line_group_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ReplaceLineGroupBindingNodesRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let binding_count = body.binding_node_ids.len();
    match pg
        .replace_admin_line_group_binding_nodes(line_group_id, body.binding_node_ids)
        .await
    {
        Ok(count) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "line_group.binding_nodes.replace",
                "line_group",
                Some(line_group_id),
                serde_json::json!({"binding_node_count": binding_count}),
            )
            .await;
            Json(serde_json::json!({
                "success": true,
                "data": {"count": count}
            }))
            .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}
