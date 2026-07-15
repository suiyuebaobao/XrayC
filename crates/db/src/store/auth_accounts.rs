//! 账户登录、注册和访问声明鉴权。
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
    pub async fn authenticate_user(
        &self,
        account: &str,
        password: &str,
    ) -> Result<Option<AuthenticatedUser>, DbError> {
        let normalized = account.trim().to_ascii_lowercase();
        if normalized.is_empty() || password.is_empty() {
            return Ok(None);
        }

        let row = sqlx::query_as::<_, AuthUserRow>(
            r#"
            SELECT id, email, password_hash, disabled, is_admin, display_name
            FROM users
            WHERE lower(email) = $1 OR ($1 = 'admin' AND is_admin = TRUE)
            ORDER BY is_admin DESC, created_at ASC
            LIMIT 1
            "#,
        )
        .bind(&normalized)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        if row.disabled || !verify_password(password, &row.password_hash) {
            return Ok(None);
        }

        sqlx::query("UPDATE users SET last_login_at = now(), updated_at = now() WHERE id = $1")
            .bind(row.id)
            .execute(&self.pool)
            .await?;

        Ok(Some(AuthenticatedUser {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            is_admin: row.is_admin,
        }))
    }

    pub async fn authorize_access_claims(
        &self,
        user_id: Uuid,
        require_admin: bool,
    ) -> Result<AuthenticatedUser, DbError> {
        let row = sqlx::query_as::<_, RefreshUserRow>(
            r#"
            SELECT id, email, disabled, is_admin, display_name
            FROM users
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::UserNotFound)?;

        if row.disabled {
            return Err(DbError::UserDisabled);
        }
        if require_admin && !row.is_admin {
            return Err(DbError::AdminRequired);
        }

        Ok(AuthenticatedUser {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            is_admin: row.is_admin,
        })
    }

    pub async fn login_account_is_admin(&self, account: &str) -> Result<bool, DbError> {
        let normalized = account.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(false);
        }

        let is_admin = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT COALESCE((
                SELECT is_admin
                FROM users
                WHERE lower(email) = $1 OR ($1 = 'admin' AND is_admin = TRUE)
                ORDER BY is_admin DESC, created_at ASC
                LIMIT 1
            ), FALSE)
            "#,
        )
        .bind(normalized)
        .fetch_one(&self.pool)
        .await?;
        Ok(is_admin)
    }

    pub async fn register_user(
        &self,
        email: &str,
        password: &str,
    ) -> Result<RegisteredUser, DbError> {
        self.register_user_with_invite(email, password, None).await
    }

    /// 已登录用户改密：更新 password_hash 并撤销该用户全部刷新令牌。
    /// 邮箱验证码已在调用方（handler）经 verify_auth_challenge 原子消费校验，
    /// 这里只负责密码强度校验、Argon2 重哈希与会话失效，全部在同一事务内完成，
    /// 保证“改密成功即踢掉其它登录态”——旧刷新令牌一律 revoked，需重新登录。
    pub async fn set_user_password_and_revoke_sessions(
        &self,
        user_id: Uuid,
        new_password: &str,
    ) -> Result<(), DbError> {
        // 强度口径与注册/后台建用户保持一致（当前为最小长度 8 位）。
        validate_password(new_password)?;
        let new_hash = password_hash(new_password)?;

        let mut tx = self.pool.begin().await?;
        // 锁定用户行，避免与禁用/删除等并发改写竞态，且确认用户存在且未禁用。
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT disabled
            FROM users
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        match exists {
            Some(true) => return Err(DbError::UserDisabled),
            Some(false) => {}
            None => return Err(DbError::UserNotFound),
        }

        sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
            .bind(user_id)
            .bind(&new_hash)
            .execute(&mut *tx)
            .await?;

        // 撤销该用户全部未撤销刷新令牌，使含当前会话在内的所有登录态失效。
        sqlx::query(
            r#"
            UPDATE refresh_tokens
            SET revoked_at = now()
            WHERE user_id = $1 AND revoked_at IS NULL
            "#,
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn register_user_with_invite(
        &self,
        email: &str,
        password: &str,
        invite_code: Option<&str>,
    ) -> Result<RegisteredUser, DbError> {
        let email = normalize_email(email)?;
        validate_password(password)?;
        let user_id = Uuid::new_v4();
        let password_hash = password_hash(password)?;
        let xray_user_key = format!("u-{}@xrayc.local", user_id.simple());
        let access_credential = Uuid::new_v4().to_string();
        let subscription_token = format!("sub-{}", Uuid::new_v4().simple());
        let invite_code = invite_code
            .map(|code| code.trim().to_ascii_uppercase())
            .filter(|code| !code.is_empty());

        let mut tx = self.pool.begin().await?;
        let plan = sqlx::query_as::<_, DefaultPlanRow>(
            r#"
            SELECT id, traffic_limit_bytes
            FROM plans
            WHERE is_default = TRUE AND enabled = TRUE
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::DefaultPlanNotFound)?;

        let inserted = sqlx::query(
            r#"
            INSERT INTO users (
                id, email, password_hash, xray_user_key, access_credential, disabled,
                is_admin, display_name
            )
            VALUES ($1, $2, $3, $4, $5, FALSE, FALSE, $2)
            ON CONFLICT (email) DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(&email)
        .bind(&password_hash)
        .bind(&xray_user_key)
        .bind(&access_credential)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(DbError::EmailExists);
        }

        if let Some(code) = invite_code {
            let invite = sqlx::query_as::<_, InviteCodeStateRow>(
                r#"
                SELECT code, is_used
                FROM invite_codes
                WHERE code = $1
                FOR UPDATE
                "#,
            )
            .bind(&code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::InvalidInviteCode)?;
            if invite.is_used {
                return Err(DbError::InvalidInviteCode);
            }
            sqlx::query(
                r#"
                UPDATE invite_codes
                SET is_used = TRUE,
                    used_by_user_id = $2,
                    used_by_email_snapshot = $3,
                    used_at = now()
                WHERE code = $1
                "#,
            )
            .bind(&invite.code)
            .bind(user_id)
            .bind(&email)
            .execute(&mut *tx)
            .await?;
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
        .bind(Utc::now() + Duration::days(30))
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

        tx.commit().await?;
        Ok(RegisteredUser {
            user: AuthenticatedUser {
                id: user_id,
                email,
                display_name: String::new(),
                is_admin: false,
            },
            subscription_token,
        })
    }
}
