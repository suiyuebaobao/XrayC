//! 认证安全设置、验证码和邀请码逻辑。
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
use super::json_util::preserve_write_only_smtp_password;
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
    pub async fn auth_security_public_json(&self) -> Result<Value, DbError> {
        let value = self
            .site_setting_json("auth_security", default_auth_security_setting())
            .await?;
        Ok(public_auth_security(value))
    }

    pub async fn admin_auth_security_json(&self) -> Result<Value, DbError> {
        // 字段加密已移除，SMTP 密码以明文落库，这里直接返回原值。
        let value = self
            .site_setting_json("auth_security", default_auth_security_setting())
            .await?;
        Ok(value)
    }

    pub async fn auth_security_private_json(&self) -> Result<Value, DbError> {
        // 字段加密已移除，SMTP 密码以明文落库，这里直接返回原值。
        let value = self
            .site_setting_json("auth_security", default_auth_security_setting())
            .await?;
        Ok(value)
    }

    pub async fn update_admin_auth_security_json(&self, value: Value) -> Result<Value, DbError> {
        let current = self
            .site_setting_json("auth_security", default_auth_security_setting())
            .await?;
        // 字段加密已移除，SMTP 密码以明文落库；仍保留“写时保留旧值”逻辑避免占位符覆盖。
        let value = preserve_write_only_smtp_password(
            merge_json(
                merge_json(default_auth_security_setting(), current.clone()),
                value,
            ),
            &current,
        );
        self.upsert_site_setting_json("auth_security", value)
            .await?;
        self.admin_auth_security_json().await
    }

    pub async fn create_auth_challenge(
        &self,
        scene: &str,
        target: &str,
        code: &str,
        ttl_seconds: i64,
    ) -> Result<Uuid, DbError> {
        let id = Uuid::new_v4();
        let scene = normalize_challenge_scene(scene);
        let target_hash = challenge_target_hash(&scene, target);
        let code_hash = challenge_code_hash(id, code);
        let ttl_seconds = ttl_seconds.clamp(1, 600);
        sqlx::query(
            r#"
            INSERT INTO auth_challenges (id, scene, target_hash, code_hash, expires_at)
            VALUES ($1, $2, $3, $4, now() + ($5::TEXT || ' seconds')::INTERVAL)
            "#,
        )
        .bind(id)
        .bind(scene)
        .bind(target_hash)
        .bind(code_hash)
        .bind(ttl_seconds)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn auth_challenge_recently_created(
        &self,
        scene: &str,
        target: &str,
        cooldown_seconds: i64,
    ) -> Result<bool, DbError> {
        let scene = normalize_challenge_scene(scene);
        let target_hash = challenge_target_hash(&scene, target);
        let cooldown_seconds = cooldown_seconds.clamp(1, 3600);
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM auth_challenges
                WHERE scene = $1
                  AND target_hash = $2
                  AND created_at > now() - ($3::TEXT || ' seconds')::INTERVAL
                  AND used_at IS NULL
            )
            "#,
        )
        .bind(scene)
        .bind(target_hash)
        .bind(cooldown_seconds)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    pub async fn verify_auth_challenge(
        &self,
        scene: &str,
        target: &str,
        challenge_id: Uuid,
        code: &str,
    ) -> Result<bool, DbError> {
        let scene = normalize_challenge_scene(scene);
        let row = sqlx::query_as::<_, AuthChallengeRow>(
            r#"
            UPDATE auth_challenges
            SET used_at = now()
            WHERE id = $1
              AND scene = $2
              AND used_at IS NULL
              AND expires_at > now()
            RETURNING id, target_hash, code_hash
            "#,
        )
        .bind(challenge_id)
        .bind(&scene)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(false);
        };

        // 原子 UPDATE ... RETURNING 先占用挑战，再做常量时间比较，避免多副本并发重放。
        let expected_target_hash = challenge_target_hash(&scene, target);
        let expected_code_hash = challenge_code_hash(row.id, code);
        Ok(
            constant_time_eq(row.target_hash.as_bytes(), expected_target_hash.as_bytes())
                && constant_time_eq(row.code_hash.as_bytes(), expected_code_hash.as_bytes()),
        )
    }

    pub async fn user_invite_codes_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let security = self.auth_security_public_json().await?;
        let enabled = security
            .get("allow_user_invite_generation")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_count = security
            .get("max_invite_codes_per_user")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0);
        let rows = sqlx::query_as::<_, UserInviteCodeRow>(
            r#"
            SELECT code, is_used, used_by_user_id, used_by_email_snapshot, used_at, created_at
            FROM invite_codes
            WHERE inviter_user_id = $1
            ORDER BY created_at DESC, code ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        let generated_count = rows.len() as i64;
        let remaining_count = if enabled {
            max_count.saturating_sub(generated_count)
        } else {
            0
        };
        Ok(json!({
            "enabled": enabled,
            "max_count": max_count,
            "generated_count": generated_count,
            "remaining_count": remaining_count,
            "items": rows.into_iter().map(user_invite_code_json).collect::<Vec<_>>()
        }))
    }

    pub async fn create_user_invite_code_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let security = self.auth_security_public_json().await?;
        let enabled = security
            .get("allow_user_invite_generation")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_count = security
            .get("max_invite_codes_per_user")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .clamp(0, 1000);
        if !enabled || max_count <= 0 {
            return Err(DbError::InviteGenerationDisabled);
        }

        let mut tx = self.pool.begin().await?;
        // 锁定用户行，保证同一用户并发生成邀请码时不会突破后台额度。
        let inviter_email = sqlx::query_scalar::<_, String>(
            r#"
            SELECT email
            FROM users
            WHERE id = $1 AND disabled = FALSE
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::UserNotFound)?;
        let generated_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::BIGINT
            FROM invite_codes
            WHERE inviter_user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if generated_count >= max_count {
            return Err(DbError::InviteQuotaExceeded);
        }

        let mut inserted = None;
        for _ in 0..5 {
            let code = format!("INV-{}", Uuid::new_v4().simple()).to_ascii_uppercase();
            let row = sqlx::query_as::<_, UserInviteCodeRow>(
                r#"
                INSERT INTO invite_codes (code, inviter_user_id, inviter_email_snapshot)
                VALUES ($1, $2, $3)
                ON CONFLICT (code) DO NOTHING
                RETURNING code, is_used, used_by_user_id, used_by_email_snapshot, used_at, created_at
                "#,
            )
            .bind(&code)
            .bind(user_id)
            .bind(&inviter_email)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(row) = row {
                inserted = Some(row);
                break;
            }
        }
        let row = inserted
            .ok_or_else(|| DbError::InvalidAgentPayload("邀请码生成冲突，请重试".to_string()))?;
        tx.commit().await?;
        Ok(json!({
            "enabled": true,
            "max_count": max_count,
            "generated_count": generated_count + 1,
            "remaining_count": max_count.saturating_sub(generated_count + 1),
            "item": user_invite_code_json(row)
        }))
    }

    pub async fn admin_invite_codes_json(&self) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, AdminInviteCodeRow>(
            r#"
            SELECT code, inviter_user_id, inviter_email_snapshot, is_used,
                   used_by_user_id, used_by_email_snapshot, used_at, created_at
            FROM invite_codes
            ORDER BY created_at DESC, code ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "items": rows.into_iter().map(admin_invite_code_json).collect::<Vec<_>>()
        }))
    }

    pub async fn create_admin_invite_codes_json(
        &self,
        admin_id: Uuid,
        count: i32,
    ) -> Result<Value, DbError> {
        let count = count.clamp(1, 100);
        let mut tx = self.pool.begin().await?;
        let inviter_email = sqlx::query_scalar::<_, String>(
            r#"
            SELECT email
            FROM users
            WHERE id = $1 AND is_admin = TRUE AND disabled = FALSE
            FOR UPDATE
            "#,
        )
        .bind(admin_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::AdminRequired)?;

        let mut rows = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let mut inserted = None;
            for _ in 0..5 {
                let code = format!("INV-{}", Uuid::new_v4().simple()).to_ascii_uppercase();
                let row = sqlx::query_as::<_, AdminInviteCodeRow>(
                    r#"
                    INSERT INTO invite_codes (code, inviter_user_id, inviter_email_snapshot)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (code) DO NOTHING
                    RETURNING code, inviter_user_id, inviter_email_snapshot, is_used,
                              used_by_user_id, used_by_email_snapshot, used_at, created_at
                    "#,
                )
                .bind(&code)
                .bind(admin_id)
                .bind(&inviter_email)
                .fetch_optional(&mut *tx)
                .await?;
                if let Some(row) = row {
                    inserted = Some(row);
                    break;
                }
            }
            rows.push(inserted.ok_or_else(|| {
                DbError::InvalidAgentPayload("邀请码生成冲突，请重试".to_string())
            })?);
        }
        tx.commit().await?;
        Ok(json!({
            "items": rows.into_iter().map(admin_invite_code_json).collect::<Vec<_>>()
        }))
    }

    pub async fn delete_admin_invite_code_json(&self, code: &str) -> Result<Value, DbError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(DbError::InvalidInviteCode);
        }
        let row = sqlx::query_as::<_, AdminInviteCodeRow>(
            r#"
            DELETE FROM invite_codes
            WHERE code = $1 AND is_used = FALSE
            RETURNING code, inviter_user_id, inviter_email_snapshot, is_used,
                      used_by_user_id, used_by_email_snapshot, used_at, created_at
            "#,
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return Ok(json!({
                "deleted": true,
                "item": admin_invite_code_json(row)
            }));
        }
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM invite_codes WHERE code = $1 AND is_used = TRUE)",
        )
        .bind(code)
        .fetch_one(&self.pool)
        .await?;
        if exists {
            return Err(DbError::InvalidInput(
                "已使用的邀请码不能删除，需保留邀请追责记录".to_string(),
            ));
        }
        Err(DbError::InvalidInviteCode)
    }
}
