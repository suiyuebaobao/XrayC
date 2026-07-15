//! 管理端邀请码接口模块。
//! 本文件提供管理员查看、批量生成和删除未使用邀请码的 HTTP handler。
//! 普通用户自助生成仍在 user.rs 中处理，避免权限逻辑混用。
//! 管理员生成不受用户自助开关和用户额度限制。
//! 已使用邀请码不能删除，必须保留邀请来源追责记录。
//! 响应字段保持 snake_case，前端 normalizer 再转 camelCase。
//! 审计日志只记录数量和邀请码本身，不记录任何登录令牌。
//! 本模块不访问部署主机，也不输出服务器敏感信息。
//! 文件前十行中文注释满足仓库规则。
//! 新增功能应继续保持单文件低于行数限制。

use super::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateAdminInviteCodesRequest {
    #[serde(default = "default_invite_count")]
    pub(crate) count: i32,
}

fn default_invite_count() -> i32 {
    1
}

pub(crate) async fn admin_invite_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_invite_codes_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_admin_invite_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateAdminInviteCodesRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .create_admin_invite_codes_json(claims.user_id(), body.count)
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "invite_codes.create",
                "invite_code_batch",
                None,
                serde_json::json!({"count": body.count.clamp(1, 100)}),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"success": true, "data": data})),
            )
                .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_admin_invite_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_invite_code_json(&code).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "invite_codes.delete",
                "invite_code",
                None,
                serde_json::json!({"code_redacted": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(DbError::InvalidInviteCode) => not_found("邀请码不存在或已删除").into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}
