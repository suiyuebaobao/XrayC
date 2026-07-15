//! 后台删除用户时的关联数据清理。
//! 本模块只封装用户删除前必须主动清理的账本和运行态事件。
//! 外键可置空的会话事件不能留下孤儿数据，避免后台仍能按用户 key 追到旧用户。
//! 调用方负责管理员保护、事务提交和节点脏标记。
//! 这里不做用户存在性判断，也不修改用户主表。
//! 所有 SQL 必须在调用方传入的事务中执行，保证删除用户的一致性。
//! 返回值只包含清理计数，供管理接口审计和前端展示。
//! 本文件不记录真实 IP、Token、密码或服务器信息。
//! 新增字段时要同步后台删除接口响应和前端类型。
//! 本头部满足前十行中文注释约束。

use crate::*;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(crate) struct DeletedUserRelatedRows {
    pub(crate) usage_ledgers: u64,
    pub(crate) snapshots: u64,
    pub(crate) sessions: u64,
    pub(crate) session_events: u64,
}

pub(crate) async fn lock_user_admin_guard_in_tx(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), DbError> {
    sqlx::query("SELECT pg_advisory_xact_lock(7043344627975397731)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn ensure_enabled_admins_remain_after_delete(
    tx: &mut Transaction<'_, Postgres>,
    deleting_enabled_admins: i64,
) -> Result<(), DbError> {
    if deleting_enabled_admins <= 0 {
        return Ok(());
    }
    let enabled_admin_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM users
        WHERE is_admin = TRUE AND disabled = FALSE
        FOR UPDATE
        "#,
    )
    .fetch_all(&mut **tx)
    .await?;
    if enabled_admin_ids.len() as i64 - deleting_enabled_admins <= 0 {
        return Err(DbError::InvalidAgentPayload(
            "不能删除最后一个启用管理员".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn delete_user_related_rows(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    xray_user_key: &str,
) -> Result<DeletedUserRelatedRows, DbError> {
    let usage_ledgers = sqlx::query("DELETE FROM usage_ledgers WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let snapshots = sqlx::query("DELETE FROM access_traffic_snapshots WHERE xray_user_key = $1")
        .bind(xray_user_key)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let sessions = sqlx::query("DELETE FROM access_user_sessions WHERE xray_user_key = $1")
        .bind(xray_user_key)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let session_events = sqlx::query(
        "DELETE FROM access_user_session_events WHERE user_id = $1 OR xray_user_key = $2",
    )
    .bind(user_id)
    .bind(xray_user_key)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(DeletedUserRelatedRows {
        usage_ledgers,
        snapshots,
        sessions,
        session_events,
    })
}
