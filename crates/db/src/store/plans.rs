//! 套餐管理和套餐线路组绑定逻辑。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn list_plans(&self) -> Result<Vec<Plan>, DbError> {
        let data = self.load_store_data().await?;
        Ok(data.plans.into_values().collect())
    }

    pub async fn admin_plans_json(&self) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, AdminPlanRow>(
            r#"
            SELECT id, name, is_default, traffic_limit_bytes, rate_limit_bps,
                   rate_limit_up_bps, rate_limit_down_bps,
                   billing_multiplier::float8 AS billing_multiplier,
                   enabled, price_cents, currency,
                   duration_days, sort_weight, is_deleted, created_at,
                   default_line_group_id
            FROM plans
            WHERE is_deleted = FALSE
            ORDER BY is_default DESC, sort_weight, created_at, name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let plan_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
        let groups = self.admin_plan_line_groups_map(&plan_ids).await?;
        Ok(json!({
            "plans": rows
                .into_iter()
                .map(|row| {
                    let plan_id = row.id;
                    admin_plan_json(row, groups.get(&plan_id).cloned().unwrap_or_default())
                })
                .collect::<Vec<_>>()
        }))
    }

    pub async fn create_admin_plan_json(&self, input: AdminPlanInput) -> Result<Value, DbError> {
        let name = required_admin_text(&input.name, "套餐名称", 128)?;
        let currency = required_admin_text(&input.currency, "币种", 16)?.to_ascii_uppercase();
        let traffic_limit_bytes = validate_non_negative_i64(input.traffic_limit_bytes, "流量额度")?;
        let rate_limit_bps = validate_non_negative_i64(input.rate_limit_bps, "套餐限速")?;
        // 方向限速覆盖:None=对称沿用 rate_limit_bps,Some(v)=该方向独立限速(v 非负;0=该方向不限)。
        let rate_limit_up_bps = input
            .rate_limit_up_bps
            .map(|v| validate_non_negative_i64(v, "上行限速"))
            .transpose()?;
        let rate_limit_down_bps = input
            .rate_limit_down_bps
            .map(|v| validate_non_negative_i64(v, "下行限速"))
            .transpose()?;
        let price_cents = validate_non_negative_i64(input.price_cents, "套餐价格")?;
        let billing_multiplier = validate_plan_multiplier(input.billing_multiplier, "扣费倍率")?;
        let duration_days = validate_positive_i32(input.duration_days, "套餐周期")?;
        let sort_weight = input.sort_weight.clamp(0, 1_000_000);

        let row = sqlx::query_as::<_, AdminPlanRow>(
            r#"
            INSERT INTO plans (
                name, is_default, traffic_limit_bytes, rate_limit_bps,
                billing_multiplier, enabled,
                price_cents, currency, duration_days, sort_weight, is_deleted,
                rate_limit_up_bps, rate_limit_down_bps
            )
            VALUES ($1, FALSE, $2, $3, $4, $5, $6, $7, $8, $9, FALSE, $10, $11)
            RETURNING id, name, is_default, traffic_limit_bytes, rate_limit_bps,
                      rate_limit_up_bps, rate_limit_down_bps,
                      billing_multiplier::float8 AS billing_multiplier,
                      enabled, price_cents, currency,
                      duration_days, sort_weight, is_deleted, created_at,
                      default_line_group_id
            "#,
        )
        .bind(name)
        .bind(traffic_limit_bytes)
        .bind(rate_limit_bps)
        .bind(billing_multiplier)
        .bind(input.enabled)
        .bind(price_cents)
        .bind(currency)
        .bind(duration_days)
        .bind(sort_weight)
        .bind(rate_limit_up_bps)
        .bind(rate_limit_down_bps)
        .fetch_one(&self.pool)
        .await?;
        Ok(admin_plan_json(row, Vec::new()))
    }

    pub async fn update_admin_plan_json(
        &self,
        plan_id: Uuid,
        input: AdminPlanUpdate,
    ) -> Result<Value, DbError> {
        let mut tx = self.pool.begin().await?;
        let is_default = sqlx::query_scalar::<_, bool>(
            "SELECT is_default FROM plans WHERE id = $1 AND is_deleted = FALSE FOR UPDATE",
        )
        .bind(plan_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("套餐不存在: {plan_id}")))?;
        if is_default && input.enabled == Some(false) {
            return Err(DbError::InvalidAgentPayload(
                "基础套餐必须保持启用，不能停用".to_string(),
            ));
        }

        let name = input
            .name
            .map(|value| required_admin_text(&value, "套餐名称", 128))
            .transpose()?;
        let currency = input
            .currency
            .map(|value| {
                required_admin_text(&value, "币种", 16).map(|text| text.to_ascii_uppercase())
            })
            .transpose()?;
        let traffic_limit_bytes = input
            .traffic_limit_bytes
            .map(|value| validate_non_negative_i64(value, "流量额度"))
            .transpose()?;
        let rate_limit_bps = input
            .rate_limit_bps
            .map(|value| validate_non_negative_i64(value, "套餐限速"))
            .transpose()?;
        // 方向限速 patch:None=不改,Some(None)=清空回退对称,Some(Some(v))=设为 v(校验非负)。
        let rate_limit_up_bps = match input.rate_limit_up_bps {
            Some(Some(v)) => Some(Some(validate_non_negative_i64(v, "上行限速")?)),
            other => other,
        };
        let rate_limit_down_bps = match input.rate_limit_down_bps {
            Some(Some(v)) => Some(Some(validate_non_negative_i64(v, "下行限速")?)),
            other => other,
        };
        let price_cents = input
            .price_cents
            .map(|value| validate_non_negative_i64(value, "套餐价格"))
            .transpose()?;
        let billing_multiplier = input
            .billing_multiplier
            .map(|value| validate_plan_multiplier(value, "扣费倍率"))
            .transpose()?;
        let duration_days = input
            .duration_days
            .map(|value| validate_positive_i32(value, "套餐周期"))
            .transpose()?;
        let sort_weight = input.sort_weight.map(|value| value.clamp(0, 1_000_000));
        let row = sqlx::query_as::<_, AdminPlanRow>(
            r#"
            UPDATE plans
            SET name = COALESCE($2, name),
                traffic_limit_bytes = COALESCE($3, traffic_limit_bytes),
                rate_limit_bps = COALESCE($4, rate_limit_bps),
                billing_multiplier = COALESCE($5, billing_multiplier),
                enabled = COALESCE($6, enabled),
                price_cents = COALESCE($7, price_cents),
                currency = COALESCE($8, currency),
                duration_days = COALESCE($9, duration_days),
                sort_weight = COALESCE($10, sort_weight),
                rate_limit_up_bps = CASE WHEN $11 THEN $12 ELSE rate_limit_up_bps END,
                rate_limit_down_bps = CASE WHEN $13 THEN $14 ELSE rate_limit_down_bps END
            WHERE id = $1 AND is_deleted = FALSE
            RETURNING id, name, is_default, traffic_limit_bytes, rate_limit_bps,
                      rate_limit_up_bps, rate_limit_down_bps,
                      billing_multiplier::float8 AS billing_multiplier,
                      enabled, price_cents, currency,
                      duration_days, sort_weight, is_deleted, created_at,
                      default_line_group_id
            "#,
        )
        .bind(plan_id)
        .bind(name)
        .bind(traffic_limit_bytes)
        .bind(rate_limit_bps)
        .bind(billing_multiplier)
        .bind(input.enabled)
        .bind(price_cents)
        .bind(currency)
        .bind(duration_days)
        .bind(sort_weight)
        .bind(rate_limit_up_bps.is_some())
        .bind(rate_limit_up_bps.flatten())
        .bind(rate_limit_down_bps.is_some())
        .bind(rate_limit_down_bps.flatten())
        .fetch_one(&mut *tx)
        .await?;

        let affected_subscription_count = if traffic_limit_bytes.is_some() {
            sqlx::query(
                r#"
                UPDATE user_subscriptions
                SET limit_bytes = $2,
                    updated_at = now()
                WHERE plan_id = $1
                "#,
            )
            .bind(plan_id)
            .bind(row.traffic_limit_bytes)
            .execute(&mut *tx)
            .await?
            .rows_affected()
        } else {
            0
        };
        // 套餐额度、倍率或入口授权变化会影响现有用户授权与扣费，必须触发节点刷新。
        sqlx::query(
            "DELETE FROM user_access_line_assignments WHERE user_id IN (SELECT user_id FROM user_subscriptions WHERE plan_id = $1)",
        )
        .bind(plan_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM user_exit_assignments WHERE user_id IN (SELECT user_id FROM user_subscriptions WHERE plan_id = $1)",
        )
        .bind(plan_id)
        .execute(&mut *tx)
        .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_plan").await?;
        tx.commit().await?;
        let groups = self.admin_plan_line_groups_map(&[plan_id]).await?;
        let mut data = admin_plan_json(row, groups.get(&plan_id).cloned().unwrap_or_default());
        data["affected_subscriptions"] = json!(affected_subscription_count);
        Ok(data)
    }

    pub async fn delete_admin_plan_json(&self, plan_id: Uuid) -> Result<Value, DbError> {
        let mut tx = self.pool.begin().await?;
        let is_default = sqlx::query_scalar::<_, bool>(
            "SELECT is_default FROM plans WHERE id = $1 AND is_deleted = FALSE FOR UPDATE",
        )
        .bind(plan_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("套餐不存在: {plan_id}")))?;
        if is_default {
            return Err(DbError::InvalidAgentPayload(
                "基础套餐不能删除，只能编辑基础字段或授权".to_string(),
            ));
        }
        let default_plan = sqlx::query_as::<_, DefaultPlanRow>(
            r#"
            SELECT id, traffic_limit_bytes
            FROM plans
            WHERE is_default = TRUE AND enabled = TRUE AND is_deleted = FALSE
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::DefaultPlanNotFound)?;
        let default_duration_days =
            sqlx::query_scalar::<_, i32>("SELECT duration_days FROM plans WHERE id = $1")
                .bind(default_plan.id)
                .fetch_one(&mut *tx)
                .await?;

        let affected_users = sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM user_subscriptions WHERE plan_id = $1",
        )
        .bind(plan_id)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE plans
            SET is_deleted = TRUE, enabled = FALSE
            WHERE id = $1 AND is_deleted = FALSE
            "#,
        )
        .bind(plan_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM plan_line_groups WHERE plan_id = $1")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            UPDATE user_subscriptions
            SET plan_id = $2,
                used_bytes = 0,
                limit_bytes = $3,
                expires_at = CASE
                    WHEN active = TRUE THEN GREATEST(expires_at, now() + ($4::BIGINT * interval '1 day'))
                    ELSE expires_at
                END,
                updated_at = now()
            WHERE plan_id = $1
            "#,
        )
        .bind(plan_id)
        .bind(default_plan.id)
        .bind(default_plan.traffic_limit_bytes)
        .bind(i64::from(default_duration_days.max(1)))
        .execute(&mut *tx)
        .await?;
        if !affected_users.is_empty() {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = ANY($1)")
                .bind(&affected_users)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = ANY($1)")
                .bind(&affected_users)
                .execute(&mut *tx)
                .await?;
        }
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_deleted_plan").await?;
        tx.commit().await?;
        Ok(json!({
            "id": plan_id,
            "deleted": true,
            "fallback_plan_id": default_plan.id,
            "affected_subscriptions": affected_users.len()
        }))
    }

    pub(crate) async fn admin_plan_line_groups_map(
        &self,
        plan_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<Value>>, DbError> {
        if plan_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, AdminPlanLineGroupRow>(
            r#"
            SELECT plg.plan_id, plg.line_group_id,
                   lg.sort_weight, plg.billing_multiplier::float8 AS billing_multiplier, lg.enabled
            FROM plan_line_groups plg
            JOIN line_groups lg ON lg.id = plg.line_group_id
            WHERE plg.plan_id = ANY($1)
            ORDER BY lg.sort_weight, lg.name, plg.line_group_id
            "#,
        )
        .bind(plan_ids)
        .fetch_all(&self.pool)
        .await?;
        let mut groups: HashMap<Uuid, Vec<Value>> = HashMap::new();
        for row in rows {
            groups.entry(row.plan_id).or_default().push(json!({
                "line_group_id": row.line_group_id,
                "group_level": "group",
                "parent_group_id": null,
                "sort_weight": row.sort_weight,
                "billing_multiplier": row.billing_multiplier,
                "enabled": row.enabled
            }));
        }
        Ok(groups)
    }

    pub async fn replace_admin_plan_line_groups(
        &self,
        plan_id: Uuid,
        bindings: Vec<AdminPlanLineGroupInput>,
    ) -> Result<usize, DbError> {
        self.replace_admin_plan_line_groups_with_default(plan_id, bindings, None)
            .await
    }

    pub async fn replace_admin_plan_line_groups_with_default(
        &self,
        plan_id: Uuid,
        bindings: Vec<AdminPlanLineGroupInput>,
        default_line_group_id: Option<Uuid>,
    ) -> Result<usize, DbError> {
        let group_ids = bindings
            .iter()
            .map(|binding| binding.line_group_id)
            .collect::<Vec<_>>();
        ensure_unique_uuids(&group_ids, "套餐线路组重复")?;
        let resolved_default_line_group_id =
            default_line_group_id.or_else(|| group_ids.first().copied());
        if let Some(default_line_group_id) = resolved_default_line_group_id {
            if !group_ids.contains(&default_line_group_id) {
                return Err(DbError::InvalidInput(
                    "默认出口分组必须是套餐已授权分组".to_string(),
                ));
            }
        }
        let mut tx = self.pool.begin().await?;
        ensure_plan_not_deleted_in_tx(&mut tx, plan_id).await?;
        if group_ids.is_empty() {
            sqlx::query("DELETE FROM plan_line_groups WHERE plan_id = $1")
                .bind(plan_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query(
                r#"
                DELETE FROM plan_line_groups
                WHERE plan_id = $1 AND NOT (line_group_id = ANY($2))
                "#,
            )
            .bind(plan_id)
            .bind(&group_ids)
            .execute(&mut *tx)
            .await?;
        }
        for binding in bindings {
            ensure_line_group_exists_in_tx(&mut tx, binding.line_group_id).await?;
            ensure_plan_group_binding_exists_in_tx(&mut tx, binding.line_group_id).await?;
            sqlx::query(
                r#"
                INSERT INTO plan_line_groups (
                    plan_id, line_group_id, billing_multiplier
                )
                VALUES ($1, $2, $3)
                ON CONFLICT (plan_id, line_group_id) DO UPDATE SET
                    billing_multiplier = EXCLUDED.billing_multiplier
                "#,
            )
            .bind(plan_id)
            .bind(binding.line_group_id)
            .bind(validate_plan_multiplier(
                binding.billing_multiplier.unwrap_or(1.0),
                "套餐分组扣费倍率",
            )?)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE plans SET default_line_group_id = $2 WHERE id = $1")
            .bind(plan_id)
            .bind(resolved_default_line_group_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM user_access_line_assignments WHERE user_id IN (SELECT user_id FROM user_subscriptions WHERE plan_id = $1)",
        )
        .bind(plan_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM user_exit_assignments WHERE user_id IN (SELECT user_id FROM user_subscriptions WHERE plan_id = $1)",
        )
        .bind(plan_id)
        .execute(&mut *tx)
        .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_plan_line_groups").await?;
        tx.commit().await?;
        Ok(group_ids.len())
    }
}

async fn ensure_plan_group_binding_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM line_groups WHERE id = $1)")
            .bind(line_group_id)
            .fetch_one(&mut **tx)
            .await?;
    if exists {
        return Ok(());
    }
    Err(DbError::InvalidInput("分组不存在".to_string()))
}
