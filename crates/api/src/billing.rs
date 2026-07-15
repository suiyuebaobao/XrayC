//! 订单兑换模块。
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
pub(crate) async fn user_orders(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.user_orders_json(claims.user_id()).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_order(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateOrderRequest>,
) -> Response {
    let (pg, claims) = match require_user_pg(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if !payment_enabled(pg).await {
        return payment_paused().into_response();
    }
    match pg.create_order_json(claims.user_id(), body.plan_id).await {
        Ok(mut data) => {
            attach_payment(&mut data, pg, body.channel.as_deref()).await;
            record_user_audit(
                pg,
                &claims,
                &headers,
                "order.create",
                "order",
                json_uuid_field(&data, "id"),
                serde_json::json!({
                    "plan_id": body.plan_id,
                    "order_no": data.get("order_no").cloned().unwrap_or(serde_json::Value::Null),
                    "amount_cents": data.get("amount_cents").cloned().unwrap_or(serde_json::Value::Null),
                    "currency": data.get("currency").cloned().unwrap_or(serde_json::Value::Null),
                    "status": data.get("status").cloned().unwrap_or(serde_json::Value::Null),
                }),
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

pub(crate) async fn payment_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !payment_feature_enabled() {
        return payment_paused().into_response();
    }
    if let Some(response) =
        enforce_rate_limit(&state, &headers, RateLimitScope::PaymentCallback).await
    {
        return response;
    }
    let pg = match state.pg.as_ref() {
        Some(pg) => pg,
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({
                    "success": false,
                    "message": "支付回调需要 PostgreSQL 模式"
                })),
            )
                .into_response();
        }
    };

    if let Some(response) = verify_payment_callback_signature(&headers, &body) {
        return response;
    }
    let raw_payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request("支付回调 JSON 格式无效").into_response(),
    };
    let parsed: PaymentCallbackRequest = match serde_json::from_value(raw_payload.clone()) {
        Ok(value) => value,
        Err(_) => return bad_request("支付回调字段无效").into_response(),
    };

    match pg
        .apply_payment_callback_json(PaymentCallbackInput {
            order_no: parsed.order_no,
            tx_id: parsed.tx_id,
            amount_cents: parsed.amount_cents,
            currency: parsed.currency,
            payment_address: parsed.payment_address,
            status: parsed.status,
            confirmations: parsed.confirmations,
            paid_at: parsed.paid_at,
            raw_payload,
        })
        .await
    {
        Ok(data) => {
            let is_idempotent = data
                .get("idempotent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !is_idempotent {
                if let Some(user_id) = json_uuid_field(&data, "user_id") {
                    record_system_audit(
                    pg,
                    &headers,
                    user_id,
                    "order.payment_callback_paid",
                    "order",
                    json_uuid_field(&data, "order_id"),
                    serde_json::json!({
                        "order_no": data.get("order_no").cloned().unwrap_or(serde_json::Value::Null),
                        "plan_id": data.get("plan_id").cloned().unwrap_or(serde_json::Value::Null),
                        "status": data.get("status").cloned().unwrap_or(serde_json::Value::Null),
                        "idempotent": data.get("idempotent").cloned().unwrap_or(serde_json::Value::Null),
                    }),
                )
                .await;
                }
            }
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_orders(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AdminOrdersQuery>,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg
        .admin_orders_filtered_json(AdminOrderFilters {
            page: query.page,
            page_size: query.page_size,
            status: query.status,
            user: query.user,
            email: query.email,
            keyword: query.keyword,
            order_no: query.order_no,
        })
        .await
    {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn admin_redeem_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_redeem_codes_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_admin_redeem_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateRedeemCodesRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .create_admin_redeem_codes_json(
            claims.user_id(),
            body.plan_id,
            body.count,
            body.duration_days,
            body.expires_at,
        )
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "redeem_codes.create",
                "redeem_code_batch",
                None,
                serde_json::json!({
                    "plan_id": body.plan_id,
                    "count": body.count,
                    "duration_days": body.duration_days
                }),
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

pub(crate) fn payment_feature_enabled() -> bool {
    matches!(
        std::env::var("XRAYC_PAYMENT_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// 按 DB 支付设置选定渠道,为订单出码(二维码)或出跳转 URL,写入响应;
/// 失败不阻断下单(用户可重试,订单到期自动失效)。
async fn attach_payment(
    data: &mut serde_json::Value,
    pg: &PgStore,
    requested_channel: Option<&str>,
) {
    let Ok(settings) = pg.payment_settings_json().await else {
        return;
    };
    if !settings
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    let order_no = data
        .get("order_no")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if order_no.is_empty() {
        return;
    }
    let amount_cents = data
        .get("amount_cents")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    // 选渠道:请求指定 → default_channel → 第一个启用渠道。
    let channel = requested_channel
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            settings
                .get("default_channel")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| first_enabled_channel(&settings));
    let Some(channel) = channel else {
        return;
    };
    let Some(provider) = crate::payment::payment_channel(&channel, &settings) else {
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "payment_error".to_string(),
                serde_json::Value::String(format!("支付渠道 {channel} 未启用")),
            );
        }
        return;
    };
    let subject = format!("XrayC 服务器套餐 {order_no}");
    match provider
        .create_payment(&order_no, amount_cents, &subject)
        .await
    {
        Ok(result) => {
            if let Some(object) = data.as_object_mut() {
                if let Some(qr_code) = result.qr_code {
                    object.insert("qr_code".to_string(), serde_json::Value::String(qr_code));
                }
                if let Some(pay_url) = result.pay_url {
                    object.insert("pay_url".to_string(), serde_json::Value::String(pay_url));
                }
                object.insert(
                    "pay_channel".to_string(),
                    serde_json::Value::String(channel),
                );
            }
        }
        Err(error) => {
            tracing::warn!(%order_no, %channel, %error, "下单出码失败");
            if let Some(object) = data.as_object_mut() {
                object.insert(
                    "payment_error".to_string(),
                    serde_json::Value::String(error),
                );
            }
        }
    }
}

/// 返回第一个启用的渠道(alipay→epay→wechat 顺序)。
fn first_enabled_channel(settings: &serde_json::Value) -> Option<String> {
    ["alipay", "epay", "wechat"]
        .into_iter()
        .find_map(|channel| {
            settings
                .get("providers")
                .and_then(|providers| providers.get(channel))
                .and_then(|provider| provider.get("enabled"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                .then(|| channel.to_string())
        })
}

/// 支付总开关(读 DB 设置的 enabled)。
async fn payment_enabled(pg: &PgStore) -> bool {
    pg.payment_settings_json()
        .await
        .ok()
        .and_then(|settings| settings.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// 多渠道异步通知统一处理:provider 验签 → 复用订单入账闭环 → 回纯文本 success。
/// 安全由各渠道验签(支付宝 RSA2 / epay MD5)+ 金额一致 + tx_id 幂等共同保证。
async fn handle_payment_notify(
    channel: &str,
    state: &Arc<AppState>,
    headers: &HeaderMap,
    body: &Bytes,
) -> Response {
    // 异步通知必须以纯文本 "success" 应答,否则渠道会持续重试。
    if enforce_rate_limit(state, headers, RateLimitScope::PaymentCallback)
        .await
        .is_some()
    {
        return (StatusCode::OK, "failure").into_response();
    }
    let Some(pg) = state.pg.as_ref() else {
        return (StatusCode::OK, "failure").into_response();
    };
    let Ok(settings) = pg.payment_settings_json().await else {
        return (StatusCode::OK, "failure").into_response();
    };
    if !settings
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return (StatusCode::OK, "failure").into_response();
    }
    let Some(provider) = crate::payment::payment_channel(channel, &settings) else {
        tracing::warn!(%channel, "支付通知:渠道未启用");
        return (StatusCode::OK, "failure").into_response();
    };
    let params: std::collections::BTreeMap<String, String> =
        url::form_urlencoded::parse(body).into_owned().collect();
    let Some(notice) = provider.verify_notify(&params) else {
        tracing::warn!(%channel, "支付通知验签/校验失败");
        return (StatusCode::OK, "failure").into_response();
    };
    let receive_address = std::env::var("PAYMENT_RECEIVE_ADDRESS").unwrap_or_default();
    let raw_payload = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
    let out_trade_no = notice.out_trade_no.clone();
    match pg
        .apply_payment_callback_json(PaymentCallbackInput {
            order_no: notice.out_trade_no,
            tx_id: notice.channel_tx_id,
            amount_cents: notice.amount_cents,
            currency: "CNY".to_string(),
            payment_address: Some(receive_address),
            status: "success".to_string(),
            confirmations: Some(1),
            paid_at: None,
            raw_payload,
        })
        .await
    {
        Ok(data) => {
            let is_idempotent = data
                .get("idempotent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !is_idempotent {
                if let Some(user_id) = json_uuid_field(&data, "user_id") {
                    record_system_audit(
                        pg,
                        headers,
                        user_id,
                        "order.payment_notify_paid",
                        "order",
                        json_uuid_field(&data, "order_id"),
                        serde_json::json!({"order_no": out_trade_no, "channel": channel}),
                    )
                    .await;
                }
            }
            (StatusCode::OK, "success").into_response()
        }
        Err(error) => {
            tracing::warn!(%channel, %out_trade_no, %error, "支付通知入账失败");
            (StatusCode::OK, "failure").into_response()
        }
    }
}

/// 支付宝当面付异步通知。
pub(crate) async fn alipay_notify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_payment_notify("alipay", &state, &headers, &body).await
}

/// 易支付(epay)异步通知。
pub(crate) async fn epay_notify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_payment_notify("epay", &state, &headers, &body).await
}

/// 微信支付异步通知(暂未启用:渠道工厂返回 None → failure)。
pub(crate) async fn wechat_notify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_payment_notify("wechat", &state, &headers, &body).await
}
