//! 后台出口资源模块。
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
pub(crate) async fn create_exit_resource(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateExitResourceRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "region_code": body.region_code.clone(),
        "ownership": body.ownership.clone(),
        "enabled": body.enabled
    });
    match pg
        .create_admin_exit_resource(AdminExitResourceInput {
            name: body.name,
            region_code: body.region_code,
            provider_name: body.provider_name,
            ownership: body.ownership,
            enabled: body.enabled,
        })
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_resource.create",
                "exit_resource",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_exit_resource(
    State(state): State<Arc<AppState>>,
    Path(exit_resource_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UpdateExitResourceRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "fields": {
            "name": body.name.is_some(),
            "region_code": body.region_code.is_some(),
            "provider_name": body.provider_name.is_some(),
            "ownership": body.ownership.is_some(),
            "enabled": body.enabled.is_some()
        }
    });
    match pg
        .update_admin_exit_resource(
            exit_resource_id,
            AdminExitResourceUpdate {
                name: body.name,
                region_code: body.region_code,
                provider_name: body.provider_name,
                ownership: body.ownership,
                enabled: body.enabled,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_resource.update",
                "exit_resource",
                Some(exit_resource_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn create_exit_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateExitEndpointRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "exit_resource_id": body.exit_resource_id,
        "name": body.name.clone(),
        "outbound_type": body.outbound_type.clone(),
        "port": body.port,
        "enabled": body.enabled,
        "outbound_config_redacted": true,
        "stream_config_redacted": true
    });
    match pg
        .create_admin_exit_endpoint(AdminExitEndpointInput {
            exit_resource_id: body.exit_resource_id,
            name: body.name,
            outbound_type: body.outbound_type,
            host: body.host,
            port: body.port,
            outbound_config: body.outbound_config,
            stream_config: body.stream_config,
            probe_config: body.probe_config,
            enabled: body.enabled,
        })
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_endpoint.create",
                "exit_endpoint",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_exit_endpoint(
    State(state): State<Arc<AppState>>,
    Path(exit_endpoint_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UpdateExitEndpointRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "fields": {
            "exit_resource_id": body.exit_resource_id.is_some(),
            "name": body.name.is_some(),
            "outbound_type": body.outbound_type.is_some(),
            "host": body.host.is_some(),
            "port": body.port.is_some(),
            "outbound_config": body.outbound_config.is_some(),
            "stream_config": body.stream_config.is_some(),
            "probe_config": body.probe_config.is_some(),
            "enabled": body.enabled.is_some()
        },
        "sensitive_redacted": true
    });
    match pg
        .update_admin_exit_endpoint(
            exit_endpoint_id,
            AdminExitEndpointUpdate {
                exit_resource_id: body.exit_resource_id,
                name: body.name,
                outbound_type: body.outbound_type,
                host: body.host,
                port: body.port,
                outbound_config: body.outbound_config,
                stream_config: body.stream_config,
                probe_config: body.probe_config,
                enabled: body.enabled,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_endpoint.update",
                "exit_endpoint",
                Some(exit_endpoint_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_exit_endpoint(
    State(state): State<Arc<AppState>>,
    Path(exit_endpoint_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_exit_endpoint(exit_endpoint_id).await {
        Ok(deleted_count) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_endpoint.delete",
                "exit_endpoint",
                Some(exit_endpoint_id),
                serde_json::json!({ "deleted_count": deleted_count }),
            )
            .await;
            Json(serde_json::json!({
                "success": true,
                "data": { "deleted_count": deleted_count }
            }))
            .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn trigger_exit_endpoint_probe(
    State(state): State<Arc<AppState>>,
    Path(exit_endpoint_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .record_admin_exit_endpoint_probe_request(exit_endpoint_id)
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "exit_endpoint.probe",
                "exit_endpoint",
                Some(exit_endpoint_id),
                serde_json::json!({"manual": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}
