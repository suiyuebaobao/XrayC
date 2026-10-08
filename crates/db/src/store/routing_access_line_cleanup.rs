//! 入口线路删除前的历史数据解绑 helper。
//! 删除 access_lines 前必须先处理历史账本和聚合表。
//! usage_ledgers 的汇总触发器会读取旧 access_line_id。
//! 如果完全依赖外键的 ON DELETE SET NULL，触发器可能在父行已删除后重建汇总。
//! 因此业务删除路径先显式置空历史账本，再删除运行线路。
//! 这里不删除历史流量；稳定身份和当时名称由独立历史路由快照保留。
//! 调用方仍负责删除用户分配、运行线路和脏标记。
//! 本模块只访问 PostgreSQL，不访问远端节点。
//! 新增 access_lines 依赖表时，应同步维护本 helper。
//! 本头部满足前十行中文注释约束。
use crate::*;
use uuid::Uuid;

pub(crate) async fn detach_usage_for_access_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_line_ids: &[Uuid],
) -> Result<(), DbError> {
    if access_line_ids.is_empty() {
        return Ok(());
    }

    sqlx::query("UPDATE usage_ledgers SET access_line_id = NULL WHERE access_line_id = ANY($1)")
        .bind(access_line_ids)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE usage_daily_rollups SET access_line_id = NULL WHERE access_line_id = ANY($1)",
    )
    .bind(access_line_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE usage_hourly_rollups SET access_line_id = NULL WHERE access_line_id = ANY($1)",
    )
    .bind(access_line_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE access_user_session_events SET access_line_id = NULL WHERE access_line_id = ANY($1)",
    )
    .bind(access_line_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM access_line_usage_rollups WHERE access_line_id = ANY($1)")
        .bind(access_line_ids)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// 删除出口或节点前先解除所有历史活跃引用，避免多级 SET NULL 的中间状态触发外键错误。
pub(crate) async fn detach_usage_for_routing_objects_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_ids: &[Uuid],
    exit_endpoint_ids: &[Uuid],
) -> Result<(), DbError> {
    if !exit_endpoint_ids.is_empty() {
        sqlx::query(
            "UPDATE usage_ledgers SET exit_endpoint_id = NULL WHERE exit_endpoint_id = ANY($1)",
        )
        .bind(exit_endpoint_ids)
        .execute(&mut **tx)
        .await?;
    }
    // 表名为代码内常量；同时解绑两种维度，历史快照与唯一维度不变。
    for table in ["usage_daily_rollups", "usage_hourly_rollups"] {
        sqlx::query(&format!(
            "UPDATE {table} SET
             access_node_id = CASE WHEN access_node_id = ANY($1) THEN NULL ELSE access_node_id END,
             exit_endpoint_id = CASE WHEN exit_endpoint_id = ANY($2) THEN NULL ELSE exit_endpoint_id END
             WHERE access_node_id = ANY($1) OR exit_endpoint_id = ANY($2)"
        ))
        .bind(access_node_ids)
        .bind(exit_endpoint_ids)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
