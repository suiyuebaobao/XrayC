//! 数据库实体存在性和兼容性校验 helper。
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

pub(crate) async fn ensure_access_node_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM access_nodes WHERE id = $1)")
            .bind(access_node_id)
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "中转节点不存在: {access_node_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_exit_pool_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM exit_pools WHERE id = $1)")
            .bind(exit_pool_id)
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "出口池不存在: {exit_pool_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_exit_pool_compatible_with_access_node_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_id: Uuid,
    access_node_id: Uuid,
) -> Result<(), DbError> {
    let incompatible = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM exit_pool_members m
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE m.exit_pool_id = $1
              AND r.ownership = 'local_direct'
              AND (r.access_node_id IS NULL OR r.access_node_id <> $2)
        )
        "#,
    )
    .bind(exit_pool_id)
    .bind(access_node_id)
    .fetch_one(&mut **tx)
    .await?;
    if incompatible {
        return Err(DbError::InvalidAgentPayload(
            "本机 direct 出口池只能绑定所属中转节点".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn ensure_exit_pool_member_set_compatible_with_bound_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_id: Uuid,
    endpoint_ids: &[Uuid],
) -> Result<(), DbError> {
    if endpoint_ids.is_empty() {
        return Ok(());
    }

    let incompatible = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM access_lines l
            JOIN exit_endpoints e ON e.id = ANY($2::uuid[])
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE l.exit_pool_id = $1
              AND r.ownership = 'local_direct'
              AND (r.access_node_id IS NULL OR r.access_node_id <> l.access_node_id)
        )
        "#,
    )
    .bind(exit_pool_id)
    .bind(endpoint_ids)
    .fetch_one(&mut **tx)
    .await?;
    if incompatible {
        return Err(DbError::InvalidAgentPayload(
            "本机 direct 出口不能加入已绑定其他中转节点的出口池".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn ensure_exit_endpoint_compatible_with_bound_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
) -> Result<(), DbError> {
    let incompatible = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM exit_pool_members m
            JOIN access_lines l ON l.exit_pool_id = m.exit_pool_id
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE m.exit_endpoint_id = $1
              AND r.ownership = 'local_direct'
              AND (r.access_node_id IS NULL OR r.access_node_id <> l.access_node_id)
        )
        "#,
    )
    .bind(exit_endpoint_id)
    .fetch_one(&mut **tx)
    .await?;
    if incompatible {
        return Err(DbError::InvalidAgentPayload(
            "本机 direct 出口不能绑定到其他中转节点的入口".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn ensure_line_group_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM line_groups WHERE id = $1)")
            .bind(line_group_id)
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "线路组不存在: {line_group_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_plan_not_deleted_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    plan_id: Uuid,
) -> Result<(), DbError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM plans WHERE id = $1 AND is_deleted = FALSE)",
    )
    .bind(plan_id)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "套餐不存在或已删除: {plan_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_line_belongs_to_node_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    access_line_id: Uuid,
) -> Result<(), DbError> {
    let owner =
        sqlx::query_scalar::<_, Uuid>("SELECT access_node_id FROM access_lines WHERE id = $1")
            .bind(access_line_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(DbError::AccessLineNotFound)?;
    if owner != access_node_id {
        return Err(DbError::AccessLineNodeMismatch);
    }
    Ok(())
}

pub(crate) async fn ensure_exit_endpoint_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM exit_endpoints WHERE id = $1)")
            .bind(exit_endpoint_id)
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "出口端点不存在: {exit_endpoint_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_exit_resource_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_resource_id: Uuid,
) -> Result<(), DbError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM exit_resources WHERE id = $1)")
            .bind(exit_resource_id)
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "出口资源不存在: {exit_resource_id}"
        )));
    }
    Ok(())
}
