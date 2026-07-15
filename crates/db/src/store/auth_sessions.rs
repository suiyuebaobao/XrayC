//! 登录保护和刷新令牌生命周期。
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
    pub async fn login_guard_locked_until(
        &self,
        account: &str,
    ) -> Result<Option<DateTime<Utc>>, DbError> {
        let guard_key = login_guard_key(account);
        let locked_until = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            r#"
            SELECT locked_until
            FROM login_guard_states
            WHERE guard_key = $1 AND expires_at > now()
            "#,
        )
        .bind(guard_key)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(locked_until.filter(|value| *value > Utc::now()))
    }

    pub async fn record_login_failure(
        &self,
        account: &str,
        failure_threshold: i32,
        lock_minutes: i32,
    ) -> Result<(), DbError> {
        let guard_key = login_guard_key(account);
        let failure_threshold = failure_threshold.clamp(1, 100);
        let lock_minutes = lock_minutes.clamp(1, 1440);
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, LoginGuardStateRow>(
            r#"
            SELECT failure_count, expires_at
            FROM login_guard_states
            WHERE guard_key = $1
            FOR UPDATE
            "#,
        )
        .bind(&guard_key)
        .fetch_optional(&mut *tx)
        .await?;
        let now = Utc::now();
        let current_count = row
            .filter(|row| row.expires_at > now)
            .map(|row| row.failure_count)
            .unwrap_or_default();
        let next_count = current_count + 1;
        let locked_until = if next_count >= failure_threshold {
            Some(now + Duration::minutes(lock_minutes as i64))
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO login_guard_states (
                guard_key, failure_count, locked_until, expires_at, updated_at
            )
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (guard_key) DO UPDATE SET
                failure_count = EXCLUDED.failure_count,
                locked_until = EXCLUDED.locked_until,
                expires_at = EXCLUDED.expires_at,
                updated_at = now()
            "#,
        )
        .bind(guard_key)
        .bind(next_count)
        .bind(locked_until)
        .bind(now + Duration::days(1))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn clear_login_guard(&self, account: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM login_guard_states WHERE guard_key = $1")
            .bind(login_guard_key(account))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_refresh_token(
        &self,
        user_id: Uuid,
        ttl: Duration,
    ) -> Result<String, DbError> {
        // 刷新凭证是长期 Cookie 凭证，使用两个 UUIDv4 拼接提高随机长度；
        // 数据库只保存哈希，明文只在本次 Set-Cookie 响应中出现。
        let token = format!("rt-{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token_hash = agent_token_hash(&token);
        sqlx::query(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(Utc::now() + ttl)
        .execute(&self.pool)
        .await?;
        Ok(token)
    }

    pub async fn authenticate_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<AuthenticatedUser>, DbError> {
        if token.trim().is_empty() {
            return Ok(None);
        }
        let token_hash = agent_token_hash(token);
        let row = sqlx::query_as::<_, RefreshUserRow>(
            r#"
            SELECT u.id, u.email, u.disabled, u.is_admin, u.display_name
            FROM refresh_tokens r
            JOIN users u ON u.id = r.user_id
            WHERE r.token_hash = $1
              AND r.revoked_at IS NULL
              AND r.expires_at > now()
            LIMIT 1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|row| {
            if row.disabled {
                None
            } else {
                Some(AuthenticatedUser {
                    id: row.id,
                    email: row.email,
                    display_name: row.display_name,
                    is_admin: row.is_admin,
                })
            }
        }))
    }

    pub async fn rotate_refresh_token(
        &self,
        token: &str,
        ttl: Duration,
    ) -> Result<Option<(AuthenticatedUser, String)>, DbError> {
        if token.trim().is_empty() {
            return Ok(None);
        }
        let token_hash = agent_token_hash(token);
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, RefreshTokenUserRow>(
            r#"
            SELECT r.id AS refresh_token_id,
                   u.id AS user_id,
                   u.email,
                   u.disabled,
                   u.is_admin,
                   u.display_name
            FROM refresh_tokens r
            JOIN users u ON u.id = r.user_id
            WHERE r.token_hash = $1
              AND r.revoked_at IS NULL
              AND r.expires_at > now()
            FOR UPDATE OF r
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        if row.disabled {
            sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE id = $1")
                .bind(row.refresh_token_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(None);
        }

        let new_token = format!("rt-{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let new_token_hash = agent_token_hash(&new_token);
        sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE id = $1")
            .bind(row.refresh_token_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(row.user_id)
        .bind(new_token_hash)
        .bind(Utc::now() + ttl)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some((
            AuthenticatedUser {
                id: row.user_id,
                email: row.email,
                display_name: row.display_name,
                is_admin: row.is_admin,
            },
            new_token,
        )))
    }

    pub async fn revoke_refresh_token(&self, token: &str) -> Result<(), DbError> {
        if token.trim().is_empty() {
            return Ok(());
        }
        let token_hash = agent_token_hash(token);
        sqlx::query(
            r#"
            UPDATE refresh_tokens
            SET revoked_at = now()
            WHERE token_hash = $1 AND revoked_at IS NULL
            "#,
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
