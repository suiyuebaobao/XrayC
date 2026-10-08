//! 管理员只读版本信息，展示实际构建标识与 Worker/节点回报，不暴露环境变量或凭据。
use super::*;

pub(crate) async fn system_info(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = require_admin_pg(&state, &headers).await {
        return *response;
    }
    let Some(pg) = &state.pg else {
        return internal_error("版本信息需要 PostgreSQL").into_response();
    };
    match pg.runtime_versions_json().await {
        Ok(mut data) => {
            data["api"] = serde_json::json!({
                "package_version":env!("CARGO_PKG_VERSION"),
                "environment":std::env::var("XRAYC_ENV").unwrap_or_else(|_| "development".to_string()),
                "release_id":option_env!("XRAYC_BUILD_ID").unwrap_or("development"),
                "target_os":std::env::consts::OS,"target_arch":std::env::consts::ARCH,
            });
            Json(serde_json::json!({"success":true,"data":data})).into_response()
        }
        Err(error) => internal_error(error).into_response(),
    }
}
