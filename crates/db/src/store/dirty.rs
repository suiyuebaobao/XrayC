//! 中转节点配置脏标记 helper。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::rows::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub(crate) async fn mark_access_node_dirty_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes
        SET config_dirty = TRUE,
            config_dirty_at = now(),
            desired_config_hash = NULL,
            config_dirty_reason = $2
        WHERE id = $1
        "#,
    )
    .bind(access_node_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_nodes_dirty_for_pool_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_id: Uuid,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes n
        SET config_dirty = TRUE,
            config_dirty_at = now(),
            desired_config_hash = NULL,
            config_dirty_reason = $2
        WHERE EXISTS (
            SELECT 1
            FROM access_lines l
            WHERE l.access_node_id = n.id AND l.exit_pool_id = $1
        )
        "#,
    )
    .bind(exit_pool_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_all_nodes_dirty_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes
        SET config_dirty = TRUE,
            config_dirty_at = now(),
            desired_config_hash = NULL,
            config_dirty_reason = $1
        "#,
    )
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_nodes_dirty_for_endpoint_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes n
        SET config_dirty = TRUE,
            config_dirty_at = now(),
            desired_config_hash = NULL,
            config_dirty_reason = $2
        WHERE EXISTS (
            SELECT 1
            FROM access_lines l
            WHERE l.access_node_id = n.id
              AND l.exit_endpoint_id = $1
        )
        OR EXISTS (
            SELECT 1
            FROM access_lines l
            JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
            WHERE l.access_node_id = n.id
              AND m.exit_endpoint_id = $1
        )
        OR n.id = (
            SELECT r.access_node_id
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE e.id = $1
              AND r.ownership = 'self_hosted'
              AND r.access_node_id IS NOT NULL
        )
        "#,
    )
    .bind(exit_endpoint_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_nodes_dirty_for_resource_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_resource_id: Uuid,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes n
        SET config_dirty = TRUE,
            config_dirty_at = now(),
            desired_config_hash = NULL,
            config_dirty_reason = $2
        WHERE EXISTS (
            SELECT 1
            FROM access_lines l
            JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            WHERE l.access_node_id = n.id
              AND e.exit_resource_id = $1
        )
        OR n.id = (
            SELECT r.access_node_id
            FROM exit_resources r
            WHERE r.id = $1
              AND r.ownership = 'self_hosted'
              AND r.access_node_id IS NOT NULL
        )
        "#,
    )
    .bind(exit_resource_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
