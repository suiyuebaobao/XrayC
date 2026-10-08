//! 兑换码后台和用户兑换逻辑。
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
    pub async fn admin_redeem_codes_json(&self) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, RedeemCodeRow>(
            r#"
            SELECT r.code,
                   r.plan_id,
                   p.name AS plan_name,
                   r.traffic_bytes,
                   r.duration_days,
                   r.is_used,
                   r.used_by_user_id,
                   used.email AS used_by_email,
                   r.used_at,
                   r.expires_at,
                   r.created_at
            FROM redeem_codes r
            JOIN plans p ON p.id = r.plan_id
            LEFT JOIN users used ON used.id = r.used_by_user_id
            ORDER BY r.created_at DESC
            LIMIT 500
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "items": rows.into_iter().map(redeem_code_json).collect::<Vec<_>>()
        }))
    }

    pub async fn create_admin_redeem_codes_json(
        &self,
        admin_user_id: Uuid,
        plan_id: Uuid,
        count: i32,
        duration_days: Option<i32>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Value, DbError> {
        if !(1..=100).contains(&count) {
            return Err(DbError::RedeemCountOutOfRange);
        }
        let plan = sqlx::query_as::<_, PlanRedeemSeedRow>(
            r#"
            SELECT id,
                   name,
                   traffic_limit_bytes,
                   duration_days
            FROM plans
            WHERE id = $1 AND enabled = TRUE AND is_deleted = FALSE
            "#,
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::DefaultPlanNotFound)?;
        let mut tx = self.pool.begin().await?;
        let mut items = Vec::new();
        for _ in 0..count {
            let code = format!(
                "XRAYC-{}",
                Uuid::new_v4().simple().to_string()[..12].to_ascii_uppercase()
            );
            sqlx::query(
                r#"
                INSERT INTO redeem_codes (
                    code, plan_id, created_by_user_id, traffic_bytes,
                    duration_days, expires_at
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(&code)
            .bind(plan.id)
            .bind(admin_user_id)
            .bind(plan.traffic_limit_bytes)
            .bind(duration_days.unwrap_or(plan.duration_days))
            .bind(expires_at)
            .execute(&mut *tx)
            .await?;
            items.push(json!({
                "code": code,
                "plan_id": plan.id,
                "plan_name": plan.name,
                "duration_days": duration_days.unwrap_or(plan.duration_days)
            }));
        }
        tx.commit().await?;
        Ok(json!({ "items": items }))
    }

    pub async fn redeem_code_for_user(&self, user_id: Uuid, code: &str) -> Result<Value, DbError> {
        let code = code.trim().to_ascii_uppercase();
        if code.is_empty() || code.len() > 64 {
            return Err(DbError::RedeemCodeInvalid);
        }
        let mut tx = self.pool.begin().await?;
        let redeem = sqlx::query_as::<_, RedeemApplyRow>(
            r#"
            SELECT r.code,
                   r.plan_id,
                   p.name AS plan_name,
                   COALESCE(r.traffic_bytes, p.traffic_limit_bytes) AS traffic_bytes,
                   COALESCE(r.duration_days, p.duration_days) AS duration_days,
                   r.is_used,
                   r.expires_at
            FROM redeem_codes r
            JOIN plans p ON p.id = r.plan_id
            WHERE r.code = $1
            FOR UPDATE OF r
            "#,
        )
        .bind(&code)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::RedeemCodeNotFound)?;
        if redeem.is_used {
            return Err(DbError::RedeemCodeUsed);
        }
        if redeem
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(DbError::RedeemCodeExpired);
        }
        let base_expires_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            "SELECT expires_at FROM user_subscriptions WHERE user_id = $1 FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .flatten()
        .unwrap_or_else(Utc::now);
        let next_expires_at = std::cmp::max(base_expires_at, Utc::now())
            + Duration::days(redeem.duration_days.max(0) as i64);
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
                expires_at = $3,
                limit_bytes = CASE WHEN user_subscriptions.limit_bytes = -1 OR EXCLUDED.limit_bytes = -1
                    THEN -1 ELSE user_subscriptions.limit_bytes + EXCLUDED.limit_bytes END,
                updated_at = now()
            "#,
        )
        .bind(user_id)
        .bind(redeem.plan_id)
        .bind(next_expires_at)
        .bind(redeem.traffic_bytes)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE redeem_codes
            SET is_used = TRUE, used_by_user_id = $2, used_at = now()
            WHERE code = $1
            "#,
        )
        .bind(&redeem.code)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "redeem_code_applied").await?;
        tx.commit().await?;
        Ok(json!({
            "code": redeem.code,
            "plan_id": redeem.plan_id,
            "plan_name": redeem.plan_name,
            "expires_at": next_expires_at,
            "traffic_bytes": redeem.traffic_bytes
        }))
    }
}
