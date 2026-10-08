//! 订阅读模型和订阅令牌解析入口。
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
    pub async fn user_subscription_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.load_store_data().await?;
        Ok(user_subscription_json(&data))
    }

    pub async fn user_subscription_json_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<serde_json::Value, DbError> {
        let block_unhealthy_lines = self.subscription_blocks_unhealthy_lines().await?;
        self.sync_user_access_line_assignments(user_id).await?;
        self.sync_user_exit_assignments(user_id).await?;
        let data = self.load_store_data_for_user_assignments(user_id).await?;
        Ok(user_subscription_json_for_user_with_probe_policy(
            &data,
            user_id,
            block_unhealthy_lines,
        ))
    }

    pub async fn subscription_userinfo_header(&self, token: &str) -> Result<String, DbError> {
        let token_hash = agent_token_hash(token);
        let row = sqlx::query_as::<_, SubscriptionUserinfoRow>(
            r#"
            SELECT s.used_bytes,
                   s.limit_bytes,
                   s.expires_at
            FROM subscription_tokens t
            JOIN user_subscriptions s ON s.user_id = t.user_id
            JOIN users u ON u.id = t.user_id
            WHERE t.token_hash = $1
              AND t.revoked_at IS NULL
              AND t.expires_at > now()
              AND u.disabled = FALSE
              AND s.active = TRUE
              AND s.expires_at > now()
            LIMIT 1
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            self.clear_runtime_authorization_for_subscription_token_hash(
                &token_hash,
                "subscription_token_invalidated",
            )
            .await?;
            return Err(DbError::Subscription(SubscriptionError::TokenNotFound));
        };
        let upload = 0_i64;
        let download = row.used_bytes.max(0);
        // 客户端以 total=0 表示没有流量上限，期限仍由 expire 指定。
        let total = row.limit_bytes.max(0);
        Ok(format!(
            "upload={}; download={}; total={}; expire={}",
            upload,
            download,
            total,
            row.expires_at.timestamp()
        ))
    }

    pub(crate) async fn user_id_for_subscription_token(
        &self,
        token: &str,
    ) -> Result<Uuid, DbError> {
        let token_hash = agent_token_hash(token);
        let user_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT t.user_id
            FROM subscription_tokens t
            JOIN users u ON u.id = t.user_id
            JOIN user_subscriptions s ON s.user_id = t.user_id
            WHERE t.token_hash = $1
              AND t.revoked_at IS NULL
              AND t.expires_at > now()
              AND u.disabled = FALSE
              AND s.active = TRUE
              AND s.expires_at > now()
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(user_id) = user_id else {
            self.clear_runtime_authorization_for_subscription_token_hash(
                &token_hash,
                "subscription_token_invalidated",
            )
            .await?;
            return Err(DbError::Subscription(SubscriptionError::TokenNotFound));
        };
        sqlx::query(
            r#"
            UPDATE subscription_tokens
            SET last_used_at = now()
            WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(user_id)
    }

    async fn clear_runtime_authorization_for_subscription_token_hash(
        &self,
        token_hash: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        let Some(user_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM subscription_tokens WHERE token_hash = $1 LIMIT 1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(());
        };
        let mut tx = self.pool.begin().await?;
        let access_rows =
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let exit_rows = sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if access_rows > 0 || exit_rows > 0 {
            mark_all_nodes_dirty_in_tx(&mut tx, reason).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
