//! 认证接口模块。
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
pub(crate) async fn auth_captcha(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CaptchaQuery>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    let Some(pg) = &state.pg else {
        return internal_error("PostgreSQL 未配置").into_response();
    };
    let security = match pg.auth_security_public_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let scene = query.scene.trim().to_ascii_lowercase();
    if !captcha_enabled_for_scene(&security, &scene) {
        return forbidden_with_message("当前场景未开启算术验证码").into_response();
    }
    if query.target.trim().is_empty() {
        return bad_request("验证码目标不能为空").into_response();
    }

    let (question, answer) = arithmetic_challenge();
    let ttl_seconds = captcha_ttl_seconds(&security);
    match pg
        .create_auth_challenge(&scene, &query.target, &answer, ttl_seconds)
        .await
    {
        Ok(id) => Json(serde_json::json!({
            "success": true,
            "data": {
                "id": id,
                "scene": scene,
                "question": question,
                "expires_in": ttl_seconds
            }
        }))
        .into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn send_email_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<EmailCodeRequest>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    let Some(pg) = &state.pg else {
        return internal_error("PostgreSQL 未配置").into_response();
    };
    let public_security = match pg.auth_security_public_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    if !email_verification_enabled(&public_security) {
        return forbidden_with_message("邮箱验证码未开启").into_response();
    }
    if !email_domain_allowed(&public_security, &body.email) {
        return bad_request("当前邮箱域名不允许注册").into_response();
    }
    let email = body.email.trim().to_ascii_lowercase();
    if !email.contains('@') {
        return bad_request("邮箱格式无效").into_response();
    }
    let cooldown_seconds = email_code_cooldown_seconds(&public_security);
    match pg
        .auth_challenge_recently_created("register_email", &email, cooldown_seconds)
        .await
    {
        Ok(true) => return too_many_requests("邮箱验证码发送过于频繁").into_response(),
        Ok(false) => {}
        Err(err) => return internal_error(err).into_response(),
    }

    let private_security = match pg.auth_security_private_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let code = six_digit_code();
    let challenge_id = match pg
        .create_auth_challenge("register_email", &email, &code, 60)
        .await
    {
        Ok(id) => id,
        Err(err) => return internal_error(err).into_response(),
    };

    if let Err(message) = send_email_code_via_smtp(&private_security, &email, &code).await {
        let _ = pg
            .verify_auth_challenge("register_email", &email, challenge_id, "")
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

pub(crate) async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    let Some(pg) = &state.pg else {
        return Json(serde_json::json!({
            "success": true,
            "message": "注册成功",
            "data": {
                "access_token": format!("dev-token-{}", body.email),
                "token_type": "Bearer",
                "expires_in": state.access_token_ttl.num_seconds(),
                "user": {
                    "id": 2,
                    "account": body.email,
                    "name": body.email,
                    "role": "user",
                    "is_admin": false
                }
            }
        }))
        .into_response();
    };

    let security = match pg.auth_security_public_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    if !email_domain_allowed(&security, &body.email) {
        return bad_request("当前邮箱域名不允许注册").into_response();
    }
    if captcha_enabled_for_scene(&security, "register")
        && !verify_required_challenge(
            pg,
            "register",
            &body.email,
            body.captcha_id,
            body.captcha_answer.as_deref(),
        )
        .await
    {
        return bad_request("算术验证码无效或已过期").into_response();
    }
    if email_verification_enabled(&security)
        && !verify_required_challenge(
            pg,
            "register_email",
            &body.email,
            body.email_code_id,
            body.email_code.as_deref(),
        )
        .await
    {
        return bad_request("邮箱验证码无效或已过期").into_response();
    }
    let invite_code = body.invite_code.as_deref().map(str::trim);
    if invite_required(&security) && invite_code.is_none_or(str::is_empty) {
        return bad_request("当前注册必须填写邀请码").into_response();
    }

    // 注册要拥有首次订阅生命周期：用户、基础套餐订阅和订阅令牌
    // 必须在数据库事务里一次性创建，避免出现无套餐用户。
    let registered = match pg
        .register_user_with_invite(&body.email, &body.password, invite_code)
        .await
    {
        Ok(registered) => registered,
        Err(DbError::EmailExists) => return conflict("邮箱已注册").into_response(),
        Err(DbError::InvalidInviteCode) => {
            return bad_request("邀请码无效或已使用").into_response()
        }
        Err(DbError::InvalidEmail | DbError::WeakPassword) => {
            return bad_request("注册信息无效").into_response()
        }
        Err(DbError::DefaultPlanNotFound) => {
            return bad_request("基础套餐未初始化").into_response()
        }
        Err(err) => return internal_error(err).into_response(),
    };
    let access_token =
        match issue_access_token(&registered.user, &state.jwt_secret, state.access_token_ttl) {
            Ok(token) => token,
            Err(err) => return internal_error(err).into_response(),
        };
    let refresh_token = match pg
        .create_refresh_token(registered.user.id, state.refresh_token_ttl)
        .await
    {
        Ok(token) => token,
        Err(err) => return internal_error(err).into_response(),
    };
    auth_success_response(
        &registered.user,
        access_token,
        Some(refresh_token),
        Some(registered.subscription_token),
        state.auth_response_policy(),
        "注册成功",
    )
}

pub(crate) async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Auth).await {
        return response;
    }
    if let Some(pg) = &state.pg {
        let security = match pg.auth_security_public_json().await {
            Ok(value) => value,
            Err(err) => return internal_error(err).into_response(),
        };
        let account_is_admin = match pg.login_account_is_admin(&body.account).await {
            Ok(value) => value,
            Err(err) => return internal_error(err).into_response(),
        };
        let captcha_scene =
            if account_is_admin && captcha_enabled_for_scene(&security, "admin_login") {
                Some("admin_login")
            } else if captcha_enabled_for_scene(&security, "login") {
                Some("login")
            } else {
                None
            };
        if let Some(scene) = captcha_scene {
            if !verify_required_challenge(
                pg,
                scene,
                &body.account,
                body.captcha_id,
                body.captcha_answer.as_deref(),
            )
            .await
            {
                return bad_request("算术验证码无效或已过期").into_response();
            }
        }
        let guard = login_guard_config(&security);
        if guard.enabled {
            match pg.login_guard_locked_until(&body.account).await {
                Ok(Some(locked_until)) => return login_locked(locked_until).into_response(),
                Ok(None) => {}
                Err(err) => return internal_error(err).into_response(),
            }
        }
        let user = match pg.authenticate_user(&body.account, &body.password).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                if guard.enabled {
                    if let Err(err) = pg
                        .record_login_failure(
                            &body.account,
                            guard.failure_threshold,
                            guard.lock_minutes,
                        )
                        .await
                    {
                        return internal_error(err).into_response();
                    }
                }
                return invalid_login().into_response();
            }
            Err(err) => return internal_error(err).into_response(),
        };
        if guard.enabled {
            if let Err(err) = pg.clear_login_guard(&body.account).await {
                return internal_error(err).into_response();
            }
        }
        let access_token =
            match issue_access_token(&user, &state.jwt_secret, state.access_token_ttl) {
                Ok(token) => token,
                Err(err) => return internal_error(err).into_response(),
            };
        let refresh_token = match pg
            .create_refresh_token(user.id, state.refresh_token_ttl)
            .await
        {
            Ok(token) => token,
            Err(err) => return internal_error(err).into_response(),
        };

        return auth_success_response(
            &user,
            access_token,
            Some(refresh_token),
            None,
            state.auth_response_policy(),
            "登录成功",
        );
    }

    Json(serde_json::json!({
        "success": true,
        "message": "登录成功",
            "data": {
                "access_token": format!("dev-token-{}", body.account),
                "token_type": "Bearer",
                "expires_in": state.access_token_ttl.num_seconds(),
                "user": {
                "id": 1,
                "account": body.account,
                "name": "XrayC 管理员",
                "role": "admin"
            }
        }
    }))
    .into_response()
}

pub(crate) async fn refresh(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(pg) = &state.pg else {
        return unauthorized_user().into_response();
    };
    let Some(refresh_token) = cookie_value(&headers, REFRESH_COOKIE_NAME) else {
        return unauthorized_user().into_response();
    };
    // 刷新凭证采用一次性轮换：旧刷新令牌在同一事务内撤销，
    // 新凭证只通过只读浏览器凭据下发，浏览器脚本不可读取。
    let (user, rotated_refresh_token) = match pg
        .rotate_refresh_token(refresh_token, state.refresh_token_ttl)
        .await
    {
        Ok(Some(result)) => result,
        Ok(None) => return unauthorized_user().into_response(),
        Err(err) => return internal_error(err).into_response(),
    };
    let access_token = match issue_access_token(&user, &state.jwt_secret, state.access_token_ttl) {
        Ok(token) => token,
        Err(err) => return internal_error(err).into_response(),
    };

    auth_success_response(
        &user,
        access_token,
        Some(rotated_refresh_token),
        None,
        state.auth_response_policy(),
        "刷新成功",
    )
}

pub(crate) async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let (Some(pg), Some(refresh_token)) =
        (&state.pg, cookie_value(&headers, REFRESH_COOKIE_NAME))
    {
        if let Err(err) = pg.revoke_refresh_token(refresh_token).await {
            return internal_error(err).into_response();
        }
    }

    let mut headers = HeaderMap::new();
    match HeaderValue::from_str(&clear_refresh_cookie_header(state.refresh_cookie_secure)) {
        Ok(value) => {
            headers.insert(header::SET_COOKIE, value);
        }
        Err(error) => return internal_error(error).into_response(),
    }
    (
        headers,
        Json(serde_json::json!({"success": true, "message": "已退出"})),
    )
        .into_response()
}
