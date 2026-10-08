//! 出口删除共用事务：解绑历史活动引用、撤销授权、清理衍生线路并同步关联节点。
//! 本机出口和全局出口使用同一路径，历史快照和账务总量保持不变。
use super::dirty::*;
use super::line_binding::*;
use super::routing_access_line_cleanup::*;
use crate::*;
use uuid::Uuid;

impl PgStore {
    pub(crate) async fn delete_exit_endpoint_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        exit_endpoint_id: Uuid,
    ) -> Result<u64, DbError> {
        let resource_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_resource_id FROM exit_endpoints WHERE id = $1 FOR UPDATE",
        )
        .bind(exit_endpoint_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            DbError::InvalidAgentPayload(format!("出口端点不存在: {exit_endpoint_id}"))
        })?;
        let affected_pool_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_pool_id FROM exit_pool_members WHERE exit_endpoint_id = $1",
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut **tx)
        .await?;
        let deleted_access_line_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM access_lines WHERE exit_endpoint_id = $1 ORDER BY id",
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut **tx)
        .await?;

        // 删除出口会改变运行出口集合成员和用户出口映射，必须先脏标记再让外键级联清理。
        mark_nodes_dirty_for_endpoint_in_tx(tx, exit_endpoint_id, "admin_deleted_exit_endpoint")
            .await?;
        sqlx::query("SELECT id FROM access_lines WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(&deleted_access_line_ids)
            .execute(&mut **tx)
            .await?;
        detach_usage_for_routing_objects_in_tx(tx, &[], &[exit_endpoint_id]).await?;
        detach_usage_for_access_lines_in_tx(tx, &deleted_access_line_ids).await?;
        sqlx::query("DELETE FROM user_exit_assignments WHERE exit_endpoint_id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query("DELETE FROM access_lines WHERE exit_endpoint_id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut **tx)
            .await?;
        let result = sqlx::query("DELETE FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut **tx)
            .await?;
        if !affected_pool_ids.is_empty() {
            prune_unusable_exit_pool_lines_in_tx(tx, &affected_pool_ids).await?;
        }
        // 收敛衍生状态之后保留本次操作的具体原因，避免被通用同步原因覆盖。
        for pool_id in &affected_pool_ids {
            mark_nodes_dirty_for_pool_in_tx(tx, *pool_id, "admin_deleted_exit_endpoint").await?;
        }
        sqlx::query(
            r#"
            DELETE FROM exit_resources r
            WHERE r.id = $1
              AND r.ownership IN ('third_party', 'self_hosted')
              AND NOT EXISTS (
                  SELECT 1 FROM exit_endpoints e WHERE e.exit_resource_id = r.id
              )
            "#,
        )
        .bind(resource_id)
        .execute(&mut **tx)
        .await?;
        Ok(result.rows_affected())
    }
}
