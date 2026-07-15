//! 公开接口模块。
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

/// 公开:返回已启用的支付渠道(供购买页选择),不含任何密钥。
pub(crate) async fn public_payment_channels(State(state): State<Arc<AppState>>) -> Response {
    let Some(pg) = state.pg.as_ref() else {
        return Json(
            serde_json::json!({"success": true, "data": {"enabled": false, "channels": []}}),
        )
        .into_response();
    };
    let settings = pg.payment_settings_json().await.unwrap_or_default();
    let enabled = settings
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut channels = Vec::new();
    if enabled {
        for (channel, label) in [
            ("alipay", "支付宝"),
            ("epay", "聚合支付"),
            ("wechat", "微信支付"),
        ] {
            let on = settings
                .get("providers")
                .and_then(|providers| providers.get(channel))
                .and_then(|provider| provider.get("enabled"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if on {
                channels.push(serde_json::json!({"channel": channel, "label": label}));
            }
        }
    }
    Json(serde_json::json!({"success": true, "data": {"enabled": enabled, "channels": channels}}))
        .into_response()
}

pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "service": "xrayc-api",
        "stack": "rust-axum",
    }))
}

pub(crate) async fn download_deploy_artifact(
    Path(artifact_name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let expected = std::env::var("DEPLOY_ARTIFACT_TOKEN").unwrap_or_default();
    if expected.is_empty() || bearer_token(&headers) != Some(expected.as_str()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"success": false, "message": "部署产物 token 无效"})),
        )
            .into_response();
    }
    if !is_safe_artifact_name(&artifact_name) {
        return bad_request("部署产物名称无效").into_response();
    }

    let artifact_dir = std::env::var("DEPLOY_ARTIFACT_DIR")
        .unwrap_or_else(|_| "/root/xrayc-artifacts".to_string());
    let path = PathBuf::from(artifact_dir).join(&artifact_name);
    let bytes = match std_fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"success": false, "message": "部署产物不存在"})),
            )
                .into_response();
        }
    };

    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{artifact_name}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

pub(crate) async fn auth_security(State(state): State<Arc<AppState>>) -> Response {
    if let Some(pg) = &state.pg {
        match pg.auth_security_public_json().await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        Json(serde_json::json!({
            "success": true,
            "data": {
                "captcha_enabled": false,
                "email_verification_enabled": false,
                "invite_required": false,
                "login_lock_enabled": true,
                "login_failure_threshold": 5,
                "login_lock_minutes": 5
            }
        }))
        .into_response()
    }
}

pub(crate) async fn sales_landing(State(state): State<Arc<AppState>>) -> Response {
    if let Some(pg) = &state.pg {
        match pg.sales_landing_json().await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        Json(serde_json::json!({
            "success": true,
            "data": default_sales_landing_json()
        }))
        .into_response()
    }
}

pub(crate) async fn download_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(token): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if let Some(response) = enforce_rate_limit(&state, &headers, RateLimitScope::Subscription).await
    {
        return response;
    }
    if !query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": "订阅只支持默认 Clash/mihomo YAML，不再支持 legacy 格式参数"
            })),
        )
            .into_response();
    }
    let subscription_options = if let Some(pg) = &state.pg {
        pg.subscription_settings_json()
            .await
            .ok()
            .map(|value| SubscriptionOptions::from_json(&value))
            .unwrap_or_default()
    } else {
        SubscriptionOptions::default()
    };
    let result = if let Some(pg) = &state.pg {
        pg.generate_subscription_yaml(&token)
            .await
            .map_err(subscription_error_from_db)
    } else {
        state.store.read(|data| generate_clash_yaml(data, &token))
    };

    match result {
        Ok(yaml) => {
            if let Some(pg) = &state.pg {
                let peer_addr = connect_info.map(|ConnectInfo(addr)| addr);
                let client_ip = subscription_client_ip(&headers, peer_addr);
                if !client_ip.is_empty() {
                    let user_agent =
                        header_value(&headers, header::USER_AGENT.as_str()).unwrap_or_default();
                    let _ = pg
                        .record_subscription_pull_event_for_token(
                            &token, &client_ip, "", user_agent,
                        )
                        .await;
                }
            }
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/yaml; charset=utf-8"),
            );
            if let Ok(value) = HeaderValue::from_str(&percent_encode_header_value(
                &subscription_options.profile_name,
            )) {
                headers.insert("profile-title", value);
            }
            if let Ok(value) =
                HeaderValue::from_str(&subscription_options.update_interval_hours.to_string())
            {
                headers.insert("profile-update-interval", value);
            }
            if let Ok(value) = HeaderValue::from_str(&content_disposition_filename(
                &subscription_options.profile_name,
            )) {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
            let userinfo = if let Some(pg) = &state.pg {
                pg.subscription_userinfo_header(&token).await.ok()
            } else {
                Some("upload=0; download=0; total=10737418240; expire=0".to_string())
            };
            if let Some(userinfo) = userinfo.and_then(|value| HeaderValue::from_str(&value).ok()) {
                headers.insert("subscription-userinfo", userinfo);
            }
            (headers, yaml).into_response()
        }
        Err(SubscriptionError::NoAvailableLines) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"success": false, "message": "当前套餐没有可用线路"})),
        )
            .into_response(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "message": err.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) async fn list_plans(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    if let Some(pg) = &state.pg {
        match pg.list_plans().await {
            Ok(plans) => Json(serde_json::json!({"success": true, "data": plans})),
            Err(err) => Json(serde_json::json!({"success": false, "message": err.to_string()})),
        }
    } else {
        let plans = state.store.read(|data| {
            data.plans
                .values()
                .cloned()
                .collect::<Vec<xrayc_core::Plan>>()
        });
        Json(serde_json::json!({"success": true, "data": plans}))
    }
}

fn subscription_client_ip(headers: &HeaderMap, peer_addr: Option<SocketAddr>) -> String {
    let peer_ip = peer_addr.map(|addr| addr.ip());
    if subscription_trusts_proxy_headers(peer_ip) {
        if let Some(header_ip) = subscription_forwarded_ip(headers) {
            return header_ip;
        }
    }
    peer_ip.map(|ip| ip.to_string()).unwrap_or_default()
}

fn subscription_forwarded_ip(headers: &HeaderMap) -> Option<String> {
    header_value(headers, "x-real-ip")
        .map(str::trim)
        .filter(|value| value.parse::<IpAddr>().is_ok())
        .or_else(|| {
            forwarded_for_last_hop(headers)
                .map(str::trim)
                .filter(|value| value.parse::<IpAddr>().is_ok())
        })
        .or_else(|| {
            header_value(headers, "cf-connecting-ip")
                .map(str::trim)
                .filter(|value| value.parse::<IpAddr>().is_ok())
        })
        .map(str::to_string)
}

fn subscription_trusts_proxy_headers(peer_ip: Option<IpAddr>) -> bool {
    let Some(peer_ip) = peer_ip else {
        return true;
    };
    match peer_ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

pub(crate) fn subscription_error_from_db(error: DbError) -> SubscriptionError {
    match error {
        DbError::Subscription(error) => error,
        _ => SubscriptionError::NoAvailableLines,
    }
}
