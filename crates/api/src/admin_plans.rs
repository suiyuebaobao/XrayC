//! 后台套餐模块。
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
pub(crate) async fn admin_plans(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.admin_plans_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn create_admin_plan(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminPlanRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "enabled": body.enabled,
        "rate_limit_bps": body.rate_limit_bps
    });
    match pg.create_admin_plan_json(admin_plan_input(body)).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "plan.create",
                "plan",
                json_uuid_field(&data, "id"),
                summary,
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

pub(crate) async fn update_admin_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AdminPlanUpdateRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "enabled": body.enabled,
        "rate_limit_bps": body.rate_limit_bps
    });
    match pg
        .update_admin_plan_json(plan_id, admin_plan_update(body))
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "plan.update",
                "plan",
                Some(plan_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_admin_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_plan_json(plan_id).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "plan.delete",
                "plan",
                Some(plan_id),
                serde_json::json!({}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) fn admin_plan_input(body: AdminPlanRequest) -> AdminPlanInput {
    AdminPlanInput {
        name: body.name,
        traffic_limit_bytes: body.traffic_limit_bytes,
        rate_limit_bps: body.rate_limit_bps,
        rate_limit_up_bps: body.rate_limit_up_bps,
        rate_limit_down_bps: body.rate_limit_down_bps,
        billing_multiplier: body.billing_multiplier,
        enabled: body.enabled,
        price_cents: body.price_cents,
        currency: body.currency,
        duration_days: body.duration_days,
        sort_weight: body.sort_weight,
    }
}

pub(crate) fn admin_plan_update(body: AdminPlanUpdateRequest) -> AdminPlanUpdate {
    AdminPlanUpdate {
        name: body.name,
        traffic_limit_bytes: body.traffic_limit_bytes,
        rate_limit_bps: body.rate_limit_bps,
        rate_limit_up_bps: body.rate_limit_up_bps.into_option_option(),
        rate_limit_down_bps: body.rate_limit_down_bps.into_option_option(),
        billing_multiplier: body.billing_multiplier,
        enabled: body.enabled,
        price_cents: body.price_cents,
        currency: body.currency,
        duration_days: body.duration_days,
        sort_weight: body.sort_weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_plan_patch_omitted_fields_do_not_reset_quota() {
        let body: AdminPlanUpdateRequest =
            serde_json::from_value(serde_json::json!({"name": "仅改名称"})).unwrap();
        let update = admin_plan_update(body);
        assert_eq!(update.name.as_deref(), Some("仅改名称"));
        assert_eq!(update.traffic_limit_bytes, None);
        assert_eq!(update.rate_limit_bps, None);
        assert_eq!(update.billing_multiplier, None);
        assert_eq!(update.enabled, None);
    }
}
