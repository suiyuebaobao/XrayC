//! 用户资料、后台用户和订阅令牌管理。
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
use super::user_delete::{
    delete_user_related_rows, ensure_enabled_admins_remain_after_delete,
    lock_user_admin_guard_in_tx,
};
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn user_me_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let user = sqlx::query_as::<_, UserProfileRow>(
            r#"
            SELECT id, email, display_name, disabled, is_admin, created_at
            FROM users
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::UserNotFound)?;
        Ok(json!({
            "id": user.id,
            "email": user.email,
            "account": user.email,
            "name": if user.display_name.is_empty() { user.email.clone() } else { user.display_name },
            "disabled": user.disabled,
            "is_admin": user.is_admin,
            "created_at": user.created_at
        }))
    }

    pub async fn user_usage_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let row = sqlx::query_as::<_, UserUsageRow>(
            r#"
            SELECT COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                   COALESCE(SUM(delta_total), 0)::bigint AS real_bytes
            FROM usage_ledgers
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(json!({
            "real_bytes": row.real_bytes.max(0),
            "billed_bytes": row.billed_bytes.max(0),
            "real_gb": bytes_to_gb(row.real_bytes.max(0) as u64),
            "billed_gb": bytes_to_gb(row.billed_bytes.max(0) as u64)
        }))
    }

    pub async fn create_admin_user_json(&self, input: AdminUserCreate) -> Result<Value, DbError> {
        let email = normalize_email(&input.email)?;
        validate_password(&input.password)?;
        let user_id = Uuid::new_v4();
        let password_hash = password_hash(&input.password)?;
        let xray_user_key = format!("u-{}@xrayc.local", user_id.simple());
        let access_credential = Uuid::new_v4().to_string();
        let subscription_token = format!("sub-{}", Uuid::new_v4().simple());
        let rate_limit_bps = input
            .rate_limit_bps
            .map(|value| validate_non_negative_i64(value, "用户限速"))
            .transpose()?;
        // 用户级方向限速覆盖:None=沿用套餐/对称,Some(v)=该方向独立(0=该方向不限)。
        let rate_limit_up_bps = input
            .rate_limit_up_bps
            .map(|value| validate_non_negative_i64(value, "用户上行限速"))
            .transpose()?;
        let rate_limit_down_bps = input
            .rate_limit_down_bps
            .map(|value| validate_non_negative_i64(value, "用户下行限速"))
            .transpose()?;

        let mut tx = self.pool.begin().await?;
        let plan = admin_user_plan_for_create(&mut tx, input.plan_id).await?;
        let inserted = sqlx::query(
            r#"
            INSERT INTO users (
                id, email, password_hash, xray_user_key, access_credential, disabled,
                is_admin, display_name, rate_limit_bps, rate_limit_up_bps, rate_limit_down_bps
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $2, $8, $9, $10)
            ON CONFLICT (email) DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(&email)
        .bind(&password_hash)
        .bind(&xray_user_key)
        .bind(&access_credential)
        .bind(input.disabled)
        .bind(input.is_admin)
        .bind(rate_limit_bps)
        .bind(rate_limit_up_bps)
        .bind(rate_limit_down_bps)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(DbError::EmailExists);
        }

        sqlx::query(
            r#"
            INSERT INTO user_subscriptions (
                user_id, plan_id, active, expires_at, used_bytes,
                limit_bytes
            )
            VALUES ($1, $2, TRUE, $3, 0, $4)
            "#,
        )
        .bind(user_id)
        .bind(plan.id)
        .bind(Utc::now() + Duration::days(i64::from(plan.duration_days.max(1))))
        .bind(plan.traffic_limit_bytes)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO subscription_tokens (
                token, token_hash, user_id, revoked_at, expires_at
            )
            VALUES ($1, $2, $3, NULL, $4)
            "#,
        )
        .bind(&subscription_token)
        .bind(agent_token_hash(&subscription_token))
        .bind(user_id)
        .bind(subscription_token_expires_at())
        .execute(&mut *tx)
        .await?;

        if !input.disabled {
            mark_all_nodes_dirty_in_tx(&mut tx, "admin_created_user").await?;
        }
        tx.commit().await?;
        if !input.disabled {
            self.sync_user_access_line_assignments(user_id).await?;
            self.sync_user_exit_assignments(user_id).await?;
        }
        self.admin_user_json(user_id).await
    }

    pub async fn update_admin_user_json(
        &self,
        user_id: Uuid,
        input: AdminUserUpdate,
    ) -> Result<Value, DbError> {
        let rate_limit_bps = validate_user_rate_limit_patch(input.rate_limit_bps)?;
        let rate_limit_up_bps = validate_user_rate_limit_patch(input.rate_limit_up_bps)?;
        let rate_limit_down_bps = validate_user_rate_limit_patch(input.rate_limit_down_bps)?;
        let mut tx = self.pool.begin().await?;
        lock_user_admin_guard_in_tx(&mut tx).await?;
        let current = sqlx::query_as::<_, AdminUserStateRow>(
            r#"
            SELECT email, disabled, is_admin, rate_limit_bps
            FROM users
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::UserNotFound)?;

        // 编辑用户不可修改邮箱:邮箱是账号身份/登录/邮箱验证码锚点,创建后不可变。
        // 编辑既不改邮箱,就不再对邮箱做格式归一/校验——admin 等账号邮箱可能是 "admin" 这类非标准
        // 格式,旧实现更新时 normalize 报「邮箱格式无效」把 admin 卡死;现按原始串比对:与现值不同
        // 一律拒(前端编辑态已只读,此处兜底防直连绕过),相同或未传(None)为无害空操作、邮箱不参与 UPDATE。
        if let Some(email) = &input.email {
            if email != &current.email {
                return Err(DbError::InvalidAgentPayload(
                    "用户邮箱创建后不可修改".to_string(),
                ));
            }
        }

        let next_disabled = input.disabled.unwrap_or(current.disabled);
        let next_is_admin = input.is_admin.unwrap_or(current.is_admin);
        if current.is_admin && !current.disabled && (!next_is_admin || next_disabled) {
            let enabled_admin_ids = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id
                FROM users
                WHERE is_admin = TRUE AND disabled = FALSE
                FOR UPDATE
                "#,
            )
            .fetch_all(&mut *tx)
            .await?;
            if enabled_admin_ids.len() <= 1 {
                return Err(DbError::InvalidAgentPayload(
                    "不能撤销或禁用最后一个启用管理员".to_string(),
                ));
            }
        }

        let plan = if let Some(plan_id) = input.plan_id {
            Some(
                sqlx::query_as::<_, AdminUserPlanRow>(
                    r#"
                    SELECT id, traffic_limit_bytes, duration_days
                    FROM plans
                    WHERE id = $1 AND enabled = TRUE AND is_deleted = FALSE
                    "#,
                )
                .bind(plan_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    DbError::InvalidAgentPayload(format!("套餐不存在或已禁用: {plan_id}"))
                })?,
            )
        } else {
            None
        };

        sqlx::query(
            r#"
            UPDATE users
            SET disabled = COALESCE($2, disabled),
                is_admin = COALESCE($3, is_admin),
                rate_limit_bps = CASE
                    WHEN $4::boolean THEN $5
                    ELSE rate_limit_bps
                END,
                rate_limit_up_bps = CASE
                    WHEN $6::boolean THEN $7
                    ELSE rate_limit_up_bps
                END,
                rate_limit_down_bps = CASE
                    WHEN $8::boolean THEN $9
                    ELSE rate_limit_down_bps
                END
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .bind(input.disabled)
        .bind(input.is_admin)
        .bind(rate_limit_bps.is_some())
        .bind(rate_limit_bps.flatten())
        .bind(rate_limit_up_bps.is_some())
        .bind(rate_limit_up_bps.flatten())
        .bind(rate_limit_down_bps.is_some())
        .bind(rate_limit_down_bps.flatten())
        .execute(&mut *tx)
        .await?;

        let plan_changed = plan.is_some();
        if let Some(plan) = plan {
            let next_expires_at = Utc::now() + Duration::days(i64::from(plan.duration_days.max(1)));
            sqlx::query(
                r#"
                INSERT INTO user_subscriptions (
                    user_id, plan_id, active, expires_at,
                    used_bytes, limit_bytes
                )
                VALUES ($1, $2, TRUE, $3, 0, $4)
                ON CONFLICT (user_id) DO UPDATE SET
                    plan_id = EXCLUDED.plan_id,
                    active = TRUE,
                    expires_at = GREATEST(user_subscriptions.expires_at, EXCLUDED.expires_at),
                    used_bytes = 0,
                    limit_bytes = EXCLUDED.limit_bytes,
                    updated_at = now()
                "#,
            )
            .bind(user_id)
            .bind(plan.id)
            .bind(next_expires_at)
            .bind(plan.traffic_limit_bytes)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        if input.disabled == Some(true) && !current.disabled {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        if input
            .disabled
            .is_some_and(|disabled| disabled != current.disabled)
            || plan_changed
            || rate_limit_bps.is_some_and(|value| value != current.rate_limit_bps)
        {
            mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_user").await?;
        }
        tx.commit().await?;

        if plan_changed {
            self.sync_user_access_line_assignments(user_id).await?;
            self.sync_user_exit_assignments(user_id).await?;
        }
        self.admin_user_json(user_id).await
    }

    pub async fn delete_admin_user_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let mut tx = self.pool.begin().await?;
        lock_user_admin_guard_in_tx(&mut tx).await?;
        let current = sqlx::query_as::<_, (String, bool, bool, String)>(
            r#"
            SELECT email, disabled, is_admin, xray_user_key
            FROM users
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::UserNotFound)?;

        let deleting_enabled_admins = if current.2 && !current.1 { 1 } else { 0 };
        ensure_enabled_admins_remain_after_delete(&mut tx, deleting_enabled_admins).await?;

        let deleted_related = delete_user_related_rows(&mut tx, user_id, &current.3).await?;
        sqlx::query("DELETE FROM login_guard_states WHERE guard_key = $1")
            .bind(login_guard_key(&current.0))
            .execute(&mut *tx)
            .await?;
        let deleted_users = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if !current.1 {
            mark_all_nodes_dirty_in_tx(&mut tx, "admin_deleted_user").await?;
        }
        tx.commit().await?;

        Ok(json!({
            "deleted": deleted_users == 1,
            "id": user_id,
            "email": current.0,
            "deleted_usage_ledger_count": deleted_related.usage_ledgers,
            "deleted_snapshot_count": deleted_related.snapshots,
            "deleted_session_count": deleted_related.sessions,
            "deleted_session_event_count": deleted_related.session_events
        }))
    }

    pub async fn admin_user_subscription_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        self.user_subscription_json_for_user(user_id).await
    }

    pub async fn reset_subscription_token_for_user_json(
        &self,
        user_id: Uuid,
    ) -> Result<Value, DbError> {
        let Some(old_xray_user_key) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT xray_user_key
            FROM users
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Err(DbError::UserNotFound);
        };

        let subscription_token = format!("sub-{}", Uuid::new_v4().simple());
        let access_credential = Uuid::new_v4().to_string();
        let xray_user_key = format!("u-{}@xrayc.local", Uuid::new_v4().simple());
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            UPDATE users
            SET access_credential = $2,
                xray_user_key = $3,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .bind(&access_credential)
        .bind(&xray_user_key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO subscription_tokens (
                token, token_hash, user_id, revoked_at, expires_at, last_used_at
            )
            VALUES ($1, $2, $3, NULL, $4, NULL)
            ON CONFLICT (user_id) DO UPDATE SET
                token = EXCLUDED.token,
                token_hash = EXCLUDED.token_hash,
                revoked_at = NULL,
                expires_at = EXCLUDED.expires_at,
                last_used_at = NULL,
                created_at = now()
            "#,
        )
        .bind(&subscription_token)
        .bind(agent_token_hash(&subscription_token))
        .bind(user_id)
        .bind(subscription_token_expires_at())
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM access_traffic_snapshots WHERE xray_user_key = $1")
            .bind(&old_xray_user_key)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM access_user_sessions WHERE xray_user_key = $1")
            .bind(&old_xray_user_key)
            .execute(&mut *tx)
            .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "subscription_token_reset_rotated_credential").await?;
        tx.commit().await?;

        self.user_subscription_json_for_user(user_id).await
    }
}

async fn admin_user_plan_for_create(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    plan_id: Option<Uuid>,
) -> Result<AdminUserPlanRow, DbError> {
    if let Some(plan_id) = plan_id {
        return sqlx::query_as::<_, AdminUserPlanRow>(
            r#"
            SELECT id, traffic_limit_bytes, duration_days
            FROM plans
            WHERE id = $1 AND enabled = TRUE AND is_deleted = FALSE
            "#,
        )
        .bind(plan_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("套餐不存在或已禁用: {plan_id}")));
    }

    sqlx::query_as::<_, AdminUserPlanRow>(
        r#"
        SELECT id, traffic_limit_bytes, duration_days
        FROM plans
        WHERE is_default = TRUE AND enabled = TRUE AND is_deleted = FALSE
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(DbError::DefaultPlanNotFound)
}

fn validate_user_rate_limit_patch(
    value: Option<Option<i64>>,
) -> Result<Option<Option<i64>>, DbError> {
    match value {
        Some(Some(rate_limit_bps)) => {
            validate_non_negative_i64(rate_limit_bps, "用户限速").map(|value| Some(Some(value)))
        }
        Some(None) => Ok(Some(None)),
        None => Ok(None),
    }
}
