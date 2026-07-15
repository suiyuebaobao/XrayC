//! 路由读模型模块。
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
pub(crate) async fn access_routing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if state.pg.is_some() {
        let pg = match require_admin_pg(&state, &headers).await {
            Ok(pg) => pg,
            Err(response) => return *response,
        };
        match pg.access_routing_json().await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state.store.read(access_routing_json);
        Json(serde_json::json!({"success": true, "data": data})).into_response()
    }
}

pub(crate) async fn admin_access_nodes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    admin_access_routing_collection(state, headers, "access_nodes").await
}

pub(crate) async fn admin_access_lines(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    admin_access_routing_collection(state, headers, "access_lines").await
}

pub(crate) async fn admin_exit_resources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    admin_access_routing_collection(state, headers, "exit_resources").await
}

pub(crate) async fn admin_exit_endpoints(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    admin_access_routing_collection(state, headers, "exit_endpoints").await
}

pub(crate) async fn admin_access_routing_collection(
    state: Arc<AppState>,
    headers: HeaderMap,
    field: &str,
) -> Response {
    if state.pg.is_some() {
        let pg = match require_admin_pg(&state, &headers).await {
            Ok(pg) => pg,
            Err(response) => return *response,
        };
        let result = match field {
            "access_nodes" => pg.admin_access_nodes_json().await,
            "access_lines" => pg.admin_access_lines_json().await,
            "exit_resources" => pg.admin_exit_resources_json().await,
            "exit_endpoints" => pg.admin_exit_endpoints_json().await,
            _ => Ok(admin_collection_payload(field, serde_json::json!([]))),
        };
        match result {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state.store.read(access_routing_json);
        let items = data
            .get(field)
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        Json(serde_json::json!({
            "success": true,
            "data": admin_collection_payload(field, items)
        }))
        .into_response()
    }
}

pub(crate) fn admin_collection_payload(field: &str, items: serde_json::Value) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("items".to_string(), items.clone());
    payload.insert(field.to_string(), items);
    serde_json::Value::Object(payload)
}

pub(crate) async fn exit_pools(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if state.pg.is_some() {
        let pg = match require_admin_pg(&state, &headers).await {
            Ok(pg) => pg,
            Err(response) => return *response,
        };
        match pg.exit_pools_json().await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state.store.read(exit_pools_json);
        Json(serde_json::json!({"success": true, "data": data})).into_response()
    }
}
