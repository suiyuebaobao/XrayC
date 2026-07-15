//! 后台用户模块。
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
use chrono::{DateTime, Utc};

pub(crate) async fn admin_users(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let filters = match admin_user_filters(&query) {
        Ok(filters) => filters,
        Err(message) => return bad_request(&message).into_response(),
    };
    match pg.admin_users_json(filters).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn admin_user_detail(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_user_json(user_id).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_admin_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminUserCreateRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "email": body.email.clone(),
        "disabled": body.disabled,
        "is_admin": body.is_admin,
        "plan_id": body.plan_id,
        "rate_limit_bps": body.rate_limit_bps
    });
    match pg.create_admin_user_json(admin_user_create(body)).await {
        Ok(data) => {
            let resource_id = data["id"].as_str().and_then(|id| Uuid::parse_str(id).ok());
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "user.create",
                "user",
                resource_id,
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(DbError::EmailExists) => conflict("邮箱已注册").into_response(),
        Err(DbError::InvalidEmail) => bad_request("邮箱格式无效").into_response(),
        Err(DbError::WeakPassword) => bad_request("密码长度不能少于 8 位").into_response(),
        Err(DbError::DefaultPlanNotFound) => bad_request("基础套餐未初始化").into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_user_traffic_logs(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let page = match parse_i64_query(&query, "page", 1) {
        Ok(value) => value.max(1),
        Err(message) => return bad_request(&message).into_response(),
    };
    let page_size = match parse_i64_query(&query, "page_size", 50) {
        Ok(value) => value.clamp(1, 200),
        Err(message) => return bad_request(&message).into_response(),
    };
    let from = match parse_time_query(&query, "from") {
        Ok(value) => value,
        Err(message) => return bad_request(&message).into_response(),
    };
    let to = match parse_time_query(&query, "to") {
        Ok(value) => value,
        Err(message) => return bad_request(&message).into_response(),
    };
    if matches!((&from, &to), (Some(from), Some(to)) if from > to) {
        return bad_request("from 不能晚于 to").into_response();
    }
    let access_line_id = match parse_uuid_query(&query, "access_line_id") {
        Ok(value) => value,
        Err(message) => return bad_request(&message).into_response(),
    };
    let exit_endpoint_id = match parse_uuid_query(&query, "exit_endpoint_id") {
        Ok(value) => value,
        Err(message) => return bad_request(&message).into_response(),
    };

    match pg
        .admin_user_traffic_logs_json(
            user_id,
            page,
            page_size,
            from,
            to,
            access_line_id,
            exit_endpoint_id,
        )
        .await
    {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn admin_user_devices(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let page = match parse_i64_query(&query, "page", 1) {
        Ok(value) => value.max(1),
        Err(message) => return bad_request(&message).into_response(),
    };
    let page_size = match parse_i64_query(&query, "page_size", 50) {
        Ok(value) => value.clamp(1, 200),
        Err(message) => return bad_request(&message).into_response(),
    };

    match pg.admin_user_devices_json(user_id, page, page_size).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_admin_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AdminUserUpdateRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "email": body.email.clone(),
        "disabled": body.disabled,
        "is_admin": body.is_admin,
        "plan_id": body.plan_id,
        "rate_limit_bps": match body.rate_limit_bps {
            NullableI64Patch::Unset => serde_json::Value::String("unchanged".to_string()),
            NullableI64Patch::Null => serde_json::Value::Null,
            NullableI64Patch::Value(value) => serde_json::json!(value),
        }
    });
    match pg
        .update_admin_user_json(user_id, admin_user_update(body))
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "user.update",
                "user",
                Some(user_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(DbError::EmailExists) => conflict("邮箱已注册").into_response(),
        Err(DbError::InvalidEmail) => bad_request("邮箱格式无效").into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_admin_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if claims.user_id() == user_id {
        return bad_request("不能删除当前登录管理员").into_response();
    }
    match pg.delete_admin_user_json(user_id).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "user.delete",
                "user",
                Some(user_id),
                serde_json::json!({"deleted": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn batch_delete_admin_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminUsersBatchDeleteRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut user_ids = body.user_ids;
    user_ids.sort_unstable();
    user_ids.dedup();
    if user_ids.is_empty() {
        return bad_request("请选择要删除的用户").into_response();
    }
    if user_ids.contains(&claims.user_id()) {
        return bad_request("不能删除当前登录管理员").into_response();
    }

    let deleted_items = match pg.delete_admin_users_json(&user_ids).await {
        Ok(data) => data,
        Err(DbError::UserNotFound) => return not_found("用户不存在").into_response(),
        Err(err) => return unprocessable(err).into_response(),
    };
    record_admin_audit(
        pg,
        &claims,
        &headers,
        "user.batch_delete",
        "user",
        None,
        serde_json::json!({"deleted_count": deleted_items.len()}),
    )
    .await;
    Json(serde_json::json!({
        "success": true,
        "data": {
            "deleted_count": deleted_items.len(),
            "items": deleted_items
        }
    }))
    .into_response()
}

fn parse_i64_query(
    query: &HashMap<String, String>,
    key: &str,
    default: i64,
) -> Result<i64, String> {
    let Some(value) = query
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(default);
    };
    value
        .parse::<i64>()
        .map_err(|_| format!("{key} 必须是整数"))
}

fn parse_uuid_query(query: &HashMap<String, String>, key: &str) -> Result<Option<Uuid>, String> {
    let Some(value) = query
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    Uuid::parse_str(value)
        .map(Some)
        .map_err(|_| format!("{key} 格式无效"))
}

fn parse_time_query(
    query: &HashMap<String, String>,
    key: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    let Some(value) = query
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    DateTime::parse_from_rfc3339(value)
        .map(|value| Some(value.with_timezone(&Utc)))
        .map_err(|_| format!("{key} 时间格式无效"))
}

fn admin_user_filters(query: &HashMap<String, String>) -> Result<AdminUserFilters, String> {
    Ok(AdminUserFilters {
        page: parse_optional_i64_query(query, "page")?,
        page_size: parse_optional_i64_query(query, "page_size")?,
        keyword: optional_text_query(query, "keyword"),
        email: optional_text_query(query, "email"),
        status: optional_text_query(query, "status"),
        role: optional_text_query(query, "role"),
        plan_id: parse_uuid_query(query, "plan_id")?,
    })
}

fn parse_optional_i64_query(
    query: &HashMap<String, String>,
    key: &str,
) -> Result<Option<i64>, String> {
    let Some(value) = query
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    value
        .parse::<i64>()
        .map(Some)
        .map_err(|_| format!("{key} 必须是整数"))
}

fn optional_text_query(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn admin_user_update(body: AdminUserUpdateRequest) -> AdminUserUpdate {
    AdminUserUpdate {
        email: body.email,
        disabled: body.disabled,
        is_admin: body.is_admin,
        plan_id: body.plan_id,
        rate_limit_bps: body.rate_limit_bps.into_option_option(),
        rate_limit_up_bps: body.rate_limit_up_bps.into_option_option(),
        rate_limit_down_bps: body.rate_limit_down_bps.into_option_option(),
    }
}

pub(crate) fn admin_user_create(body: AdminUserCreateRequest) -> AdminUserCreate {
    AdminUserCreate {
        email: body.email,
        password: body.password,
        disabled: body.disabled,
        is_admin: body.is_admin,
        plan_id: body.plan_id,
        rate_limit_bps: body.rate_limit_bps,
        rate_limit_up_bps: body.rate_limit_up_bps,
        rate_limit_down_bps: body.rate_limit_down_bps,
    }
}

pub(crate) async fn admin_user_subscription(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_user_subscription_json(user_id).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn reset_admin_user_subscription_token(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.reset_subscription_token_for_user_json(user_id).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "subscription_token.reset",
                "user",
                Some(user_id),
                serde_json::json!({"token_redacted": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn reset_admin_user_password(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AdminResetPasswordRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // admin 本身即授权，无需邮箱验证码，直接复用同一套强度校验 + 撤销目标用户全部刷新令牌。
    match pg
        .set_user_password_and_revoke_sessions(user_id, &body.new_password)
        .await
    {
        Ok(()) => {
            // 审计只记“某管理员重置了某用户密码”，绝不写明文新密码。
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "user.reset_password",
                "user",
                Some(user_id),
                serde_json::json!({"password_redacted": true, "sessions_revoked": true}),
            )
            .await;
            Json(serde_json::json!({
                "success": true,
                "message": "已重置，该用户需用新密码重新登录"
            }))
            .into_response()
        }
        Err(DbError::WeakPassword) => bad_request("新密码至少 8 位").into_response(),
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(DbError::UserDisabled) => unprocessable("用户已禁用，无法重置密码").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}
