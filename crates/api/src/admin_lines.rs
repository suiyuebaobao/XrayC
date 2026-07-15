//! 后台线路模块。
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
pub(crate) struct CreateLineGroupRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) country_code: String,
    #[serde(default)]
    pub(crate) dedicated_rules: Vec<String>,
    pub(crate) sort_weight: Option<i32>,
    #[serde(default)]
    pub(crate) billing_multiplier: Option<f64>,
    pub(crate) enabled: Option<bool>,
    pub(crate) rule_set_bindings: Option<Vec<LineGroupRuleSetBindingRequest>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateLineGroupRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) country_code: String,
    pub(crate) dedicated_rules: Option<Vec<String>>,
    pub(crate) sort_weight: Option<i32>,
    #[serde(default)]
    pub(crate) billing_multiplier: Option<f64>,
    pub(crate) enabled: Option<bool>,
    pub(crate) rule_set_bindings: Option<Vec<LineGroupRuleSetBindingRequest>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LineGroupRuleSetBindingRequest {
    pub(crate) rule_set_id: Uuid,
    pub(crate) position: Option<i32>,
    pub(crate) enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RuleSetRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) rules: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplaceLineGroupLinesRequest {
    #[serde(default)]
    pub(crate) exit_endpoint_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplacePlanLineGroupsRequest {
    #[serde(default)]
    pub(crate) line_groups: Vec<PlanLineGroupRequest>,
    pub(crate) default_line_group_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlanLineGroupRequest {
    pub(crate) line_group_id: Uuid,
    pub(crate) billing_multiplier: Option<f64>,
}

pub(crate) async fn create_line_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateLineGroupRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if body.billing_multiplier.is_some() {
        return validation_error("分组本身不设置扣费倍率，请在套餐授权分组里设置倍率。");
    }
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "country_code": body.country_code.clone(),
        "dedicated_rules_count": body.dedicated_rules.len(),
        "rule_set_bindings_count": body.rule_set_bindings.as_ref().map(Vec::len),
        "sort_weight": body.sort_weight,
        "enabled": body.enabled
    });
    let rule_set_bindings = line_group_rule_set_bindings_from_request(body.rule_set_bindings);
    match pg
        .create_admin_line_group(AdminLineGroupInput {
            name: body.name,
            country_code: body.country_code,
            icon: String::new(),
            group_level: None,
            parent_group_id: None,
            sort_weight: body.sort_weight,
            billing_multiplier: None,
            enabled: body.enabled,
            dedicated_rules: Some(body.dedicated_rules),
            rule_set_bindings,
        })
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "line_group.create",
                "line_group",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_line_group(
    State(state): State<Arc<AppState>>,
    Path(line_group_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UpdateLineGroupRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if body.billing_multiplier.is_some() {
        return validation_error("分组本身不设置扣费倍率，请在套餐授权分组里设置倍率。");
    }
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "country_code": body.country_code.clone(),
        "dedicated_rules_count": body.dedicated_rules.as_ref().map(Vec::len),
        "rule_set_bindings_count": body.rule_set_bindings.as_ref().map(Vec::len),
        "sort_weight": body.sort_weight,
        "enabled": body.enabled
    });
    let rule_set_bindings = line_group_rule_set_bindings_from_request(body.rule_set_bindings);
    match pg
        .update_admin_line_group(
            line_group_id,
            AdminLineGroupInput {
                name: body.name,
                country_code: body.country_code,
                icon: String::new(),
                group_level: None,
                parent_group_id: None,
                sort_weight: body.sort_weight,
                billing_multiplier: None,
                enabled: body.enabled,
                dedicated_rules: body.dedicated_rules,
                rule_set_bindings,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "line_group.update",
                "line_group",
                Some(line_group_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn list_subscription_rule_sets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.list_subscription_rule_sets().await {
        Ok(rule_sets) => {
            Json(serde_json::json!({"success": true, "data": {"rule_sets": rule_sets}}))
                .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn create_subscription_rule_set(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RuleSetRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "enabled": body.enabled,
        "rules_count": body.rules.len()
    });
    match pg
        .create_subscription_rule_set(AdminSubscriptionRuleSetInput {
            name: body.name,
            description: body.description,
            enabled: body.enabled,
            rules: body.rules,
        })
        .await
    {
        Ok(id) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "subscription_rule_set.create",
                "subscription_rule_set",
                Some(id),
                summary,
            )
            .await;
            created_id_response(id)
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn update_subscription_rule_set(
    State(state): State<Arc<AppState>>,
    Path(rule_set_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<RuleSetRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let summary = serde_json::json!({
        "name": body.name.clone(),
        "enabled": body.enabled,
        "rules_count": body.rules.len()
    });
    match pg
        .update_subscription_rule_set(
            rule_set_id,
            AdminSubscriptionRuleSetInput {
                name: body.name,
                description: body.description,
                enabled: body.enabled,
                rules: body.rules,
            },
        )
        .await
    {
        Ok(()) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "subscription_rule_set.update",
                "subscription_rule_set",
                Some(rule_set_id),
                summary,
            )
            .await;
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn delete_subscription_rule_set(
    State(state): State<Arc<AppState>>,
    Path(rule_set_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_subscription_rule_set(rule_set_id).await {
        Ok(deleted) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "subscription_rule_set.delete",
                "subscription_rule_set",
                Some(rule_set_id),
                serde_json::json!({"deleted": deleted}),
            )
            .await;
            Json(serde_json::json!({"success": true, "deleted": deleted})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

fn line_group_rule_set_bindings_from_request(
    bindings: Option<Vec<LineGroupRuleSetBindingRequest>>,
) -> Option<Vec<LineGroupRuleSetBindingInput>> {
    bindings.map(|items| {
        items
            .into_iter()
            .enumerate()
            .map(|(index, item)| LineGroupRuleSetBindingInput {
                rule_set_id: item.rule_set_id,
                position: item.position.unwrap_or(((index + 1) * 100) as i32),
                enabled: item.enabled.unwrap_or(true),
            })
            .collect()
    })
}

pub(crate) async fn delete_line_group(
    State(state): State<Arc<AppState>>,
    Path(line_group_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.delete_admin_line_group(line_group_id).await {
        Ok(deleted) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "line_group.delete",
                "line_group",
                Some(line_group_id),
                serde_json::json!({ "deleted": deleted }),
            )
            .await;
            Json(serde_json::json!({ "success": true, "deleted": deleted })).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn replace_line_group_lines(
    State(state): State<Arc<AppState>>,
    Path(line_group_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ReplaceLineGroupLinesRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let endpoint_ids = body.exit_endpoint_ids;
    let line_count = endpoint_ids.len();
    match pg
        .replace_admin_line_group_lines(line_group_id, endpoint_ids)
        .await
    {
        Ok(count) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "line_group.lines.replace",
                "line_group",
                Some(line_group_id),
                serde_json::json!({"line_count": line_count}),
            )
            .await;
            Json(serde_json::json!({
                "success": true,
                "data": {"count": count}
            }))
            .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn replace_plan_line_groups(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ReplacePlanLineGroupsRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let binding_count = body.line_groups.len();
    let bindings = body
        .line_groups
        .into_iter()
        .map(|binding| AdminPlanLineGroupInput {
            line_group_id: binding.line_group_id,
            billing_multiplier: binding.billing_multiplier,
        })
        .collect();
    match pg
        .replace_admin_plan_line_groups_with_default(plan_id, bindings, body.default_line_group_id)
        .await
    {
        Ok(count) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "plan.line_groups.replace",
                "plan",
                Some(plan_id),
                serde_json::json!({"binding_count": binding_count}),
            )
            .await;
            Json(serde_json::json!({
                "success": true,
                "data": {"count": count}
            }))
            .into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}
