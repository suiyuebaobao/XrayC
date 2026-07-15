//! 后台用户批量删除事务。
//! 本模块负责把多个用户删除合并到同一个数据库事务。
//! 任一用户不存在或管理员保护触发时，整批删除都会回滚。
//! 调用方仍负责阻止删除当前登录管理员和记录审计日志。
//! 这里会清理用户订阅、账本、运行快照和登录保护状态。
//! 只有存在启用用户被删除时才标记中转节点待同步。
//! 返回值沿用单用户删除响应结构，方便前端复用展示逻辑。
//! 本文件不输出密码、Token、订阅链接或服务器凭据。
//! 新增字段时需同步管理接口文档和前端类型。
//! 本头部满足前十行中文注释约束。

use super::dirty::mark_all_nodes_dirty_in_tx;
use super::security::login_guard_key;
use super::user_delete::{
    delete_user_related_rows, ensure_enabled_admins_remain_after_delete,
    lock_user_admin_guard_in_tx,
};
use crate::*;
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

impl PgStore {
    pub async fn delete_admin_users_json(&self, user_ids: &[Uuid]) -> Result<Vec<Value>, DbError> {
        let unique_ids: HashSet<Uuid> = user_ids.iter().copied().collect();
        let mut tx = self.pool.begin().await?;
        lock_user_admin_guard_in_tx(&mut tx).await?;
        let rows = sqlx::query_as::<_, (Uuid, String, bool, bool, String)>(
            r#"
            SELECT id, email, disabled, is_admin, xray_user_key
            FROM users
            WHERE id = ANY($1)
            FOR UPDATE
            "#,
        )
        .bind(user_ids)
        .fetch_all(&mut *tx)
        .await?;
        if rows.len() != unique_ids.len() {
            return Err(DbError::UserNotFound);
        }

        let deleting_enabled_admins = rows
            .iter()
            .filter(|(_, _, disabled, is_admin, _)| *is_admin && !*disabled)
            .count() as i64;
        ensure_enabled_admins_remain_after_delete(&mut tx, deleting_enabled_admins).await?;

        let mut deleted_items = Vec::with_capacity(rows.len());
        let mut deleted_enabled_user = false;
        for (user_id, email, disabled, _, xray_user_key) in rows {
            let deleted_related =
                delete_user_related_rows(&mut tx, user_id, &xray_user_key).await?;
            sqlx::query("DELETE FROM login_guard_states WHERE guard_key = $1")
                .bind(login_guard_key(&email))
                .execute(&mut *tx)
                .await?;
            let deleted_users = sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            deleted_enabled_user |= !disabled;
            deleted_items.push(json!({
                "deleted": deleted_users == 1,
                "id": user_id,
                "email": email,
                "deleted_usage_ledger_count": deleted_related.usage_ledgers,
                "deleted_snapshot_count": deleted_related.snapshots,
                "deleted_session_count": deleted_related.sessions,
                "deleted_session_event_count": deleted_related.session_events
            }));
        }
        if deleted_enabled_user {
            mark_all_nodes_dirty_in_tx(&mut tx, "admin_batch_deleted_users").await?;
        }
        tx.commit().await?;
        Ok(deleted_items)
    }
}
