//! 用户接口模块。
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
pub(crate) async fn user_me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.user_me_json(claims.user_id()).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn user_usage(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.user_usage_json(claims.user_id()).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn user_invite_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.user_invite_codes_json(claims.user_id()).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_user_invite_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.create_user_invite_code_json(claims.user_id()).await {
        Ok(data) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"success": true, "data": data})),
        )
            .into_response(),
        Err(DbError::InviteGenerationDisabled) => {
            forbidden_with_message("当前不允许用户自助生成邀请码")
        }
        Err(DbError::InviteQuotaExceeded) => {
            unprocessable("邀请码生成数量已达到上限").into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn redeem_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RedeemCodeRequest>,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.redeem_code_for_user(claims.user_id(), &body.code).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

// 已登录用户改密统一使用的验证码场景，与注册/登录场景隔离，互不串用。
const CHANGE_PASSWORD_SCENE: &str = "change_password_email";

pub(crate) async fn send_change_password_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    // 改密发码也走通用认证频控，叠加验证码冷却双重防滥发。
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // 邮箱以登录身份为准，绝不接受请求体传入的邮箱，避免给他人邮箱发码。
    let email = claims.email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') {
        return bad_request("账号邮箱无效").into_response();
    }

    let public_security = match pg.auth_security_public_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let cooldown_seconds = email_code_cooldown_seconds(&public_security);
    match pg
        .auth_challenge_recently_created(CHANGE_PASSWORD_SCENE, &email, cooldown_seconds)
        .await
    {
        Ok(true) => return too_many_requests("验证码发送过于频繁").into_response(),
        Ok(false) => {}
        Err(err) => return internal_error(err).into_response(),
    }

    let private_security = match pg.auth_security_private_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let code = six_digit_code();
    let challenge_id = match pg
        .create_auth_challenge(CHANGE_PASSWORD_SCENE, &email, &code, 60)
        .await
    {
        Ok(id) => id,
        Err(err) => return internal_error(err).into_response(),
    };

    // 发送失败时立刻消费掉挑战，避免占用冷却额度且不泄露明文验证码。
    if let Err(message) = send_email_code_via_smtp(&private_security, &email, &code).await {
        let _ = pg
            .verify_auth_challenge(CHANGE_PASSWORD_SCENE, &email, challenge_id, "")
            .await;
        return unprocessable(message).into_response();
    }

    Json(serde_json::json!({
        "success": true,
        "data": {
            "id": challenge_id,
            "expires_in": 60,
            "cooldown_seconds": cooldown_seconds
        }
    }))
    .into_response()
}

pub(crate) async fn change_user_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let email = claims.email.trim().to_ascii_lowercase();

    // 改密验证方式为邮箱验证码：码缺失/错误/过期/已用统一返回明确的 400。
    if !verify_required_challenge(
        pg,
        CHANGE_PASSWORD_SCENE,
        &email,
        body.email_code_id,
        body.email_code.as_deref(),
    )
    .await
    {
        return bad_request("邮箱验证码无效或已过期").into_response();
    }

    match pg
        .set_user_password_and_revoke_sessions(claims.user_id(), &body.new_password)
        .await
    {
        Ok(()) => {}
        Err(DbError::WeakPassword) => return bad_request("新密码至少 8 位").into_response(),
        Err(DbError::UserNotFound | DbError::UserDisabled) => {
            return unauthorized_user().into_response()
        }
        Err(err) => return internal_error(err).into_response(),
    }

    // 改密会撤销含当前会话在内的全部刷新令牌，这里同步清掉本次浏览器刷新 Cookie，
    // 前端据此提示并引导用户用新密码重新登录。
    let mut response_headers = HeaderMap::new();
    match HeaderValue::from_str(&clear_refresh_cookie_header(state.refresh_cookie_secure)) {
        Ok(value) => {
            response_headers.insert(header::SET_COOKIE, value);
        }
        Err(error) => return internal_error(error).into_response(),
    }
    (
        response_headers,
        Json(serde_json::json!({
            "success": true,
            "message": "密码已更新，请使用新密码重新登录"
        })),
    )
        .into_response()
}

pub(crate) async fn user_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if state.pg.is_some() {
        let (pg, claims) = match require_user_pg(&state, &headers).await {
            Ok(value) => value,
            Err(response) => return *response,
        };
        match pg.user_subscription_json_for_user(claims.user_id()).await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state.store.read(user_subscription_json);
        Json(serde_json::json!({"success": true, "data": data})).into_response()
    }
}

pub(crate) async fn reset_user_subscription_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .reset_subscription_token_for_user_json(claims.user_id())
        .await
    {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(DbError::UserNotFound) => not_found("用户不存在").into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}
