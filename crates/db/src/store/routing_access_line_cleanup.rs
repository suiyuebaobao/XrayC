//! 入口线路删除前的历史数据解绑 helper。
//! 删除 access_lines 前必须先处理历史账本和聚合表。
//! usage_ledgers 的汇总触发器会读取旧 access_line_id。
//! 如果完全依赖外键的 ON DELETE SET NULL，触发器可能在父行已删除后重建汇总。
//! 因此业务删除路径先显式置空历史账本，再删除运行线路。
//! 这里不删除历史流量，只断开已删除运行线路的外键引用。
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
