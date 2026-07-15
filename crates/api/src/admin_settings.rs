//! 后台设置模块。
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
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminAuditLogsQuery {
    pub(crate) page: Option<i64>,
    pub(crate) page_size: Option<i64>,
}

pub(crate) async fn admin_auth_security(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_auth_security_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_admin_auth_security(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_admin_auth_security_json(body).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "auth_security.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "auth_security", "smtp_password_redacted": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_subscription_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.subscription_settings_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_admin_subscription_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_subscription_settings_json(body).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "subscription_settings.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "subscription_settings"}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_sales_landing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.sales_landing_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_admin_sales_landing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_sales_landing_json(body).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "sales_landing.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "sales_landing"}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_payment_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    // 脱敏读:私钥/key 不回显,只给 *_set 布尔。
    match pg.public_payment_settings_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_admin_payment_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_payment_settings_json(body).await {
        Ok(_) => {
            // 审计只记设置键,不记任何密钥。
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "payment_settings.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "payment", "secrets_redacted": true}),
            )
            .await;
            // 返回脱敏版(重读),不把明文私钥回吐。
            match pg.public_payment_settings_json().await {
                Ok(data) => {
                    Json(serde_json::json!({"success": true, "data": data})).into_response()
                }
                Err(err) => internal_error(err).into_response(),
            }
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn admin_audit_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AdminAuditLogsQuery>,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(50);
    match pg.admin_audit_logs_json(page, page_size).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}
