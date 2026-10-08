//! 无变化心跳只读取目标节点及其已下发用户，不全量同步授权或编译配置。
//! 到期、配额、令牌与设置版本是缓存有效条件；冷启动和脏配置继续走完整校验。
use crate::xray_render::selected_node_id;
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    pub(super) async fn unchanged_heartbeat_json(
        &self,
        node_id: Uuid,
        applied_hash: Option<&str>,
    ) -> Result<Option<Value>, DbError> {
        let Some(applied_hash) = applied_hash.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let cached = sqlx::query_as::<_, (String, Vec<Uuid>)>(
            r#"
            UPDATE access_nodes n SET last_heartbeat_at=now(), applied_config_hash=$2
            WHERE n.id=$1 AND n.config_dirty=FALSE
              AND n.desired_config_hash=$2 AND n.config_built_at IS NOT NULL
              AND n.config_built_revision=n.config_revision
              AND NOT EXISTS (
                SELECT 1 FROM unnest(n.config_authorized_user_ids) AS cached(user_id)
                LEFT JOIN users u ON u.id=cached.user_id
                LEFT JOIN user_subscriptions s ON s.user_id=u.id
                WHERE u.id IS NULL OR u.disabled OR s.user_id IS NULL OR NOT s.active
                  OR s.expires_at <= now() OR (s.limit_bytes <> -1 AND s.used_bytes >= s.limit_bytes)
                  OR NOT EXISTS (SELECT 1 FROM subscription_tokens t WHERE t.user_id=u.id
                    AND t.revoked_at IS NULL AND t.expires_at > now())
              )
            RETURNING n.desired_config_hash, n.config_authorized_user_ids
            "#,
        ).bind(node_id).bind(applied_hash).fetch_optional(&self.pool).await?;
        let Some((hash, user_ids)) = cached else {
            return Ok(None);
        };
        let users = sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT id, xray_user_key, access_credential FROM users WHERE id=ANY($1) ORDER BY xray_user_key",
        ).bind(user_ids).fetch_all(&self.pool).await?;
        Ok(Some(json!({
            "success": true, "accepted": true, "config_hash": hash,
            "desired_config_version": hash,
            "config_status": {"required": false, "reason": "up_to_date"},
            "authorized_users": users.into_iter().map(|(id, key, credential)| json!({
                "user_id":id, "xray_user_key":key, "uuid":credential
            })).collect::<Vec<_>>(),
            "config": null
        })))
    }

    pub(super) async fn refreshed_heartbeat_json(
        &self,
        reported_node_id: Option<Uuid>,
        applied_hash: Option<&str>,
    ) -> Result<(Value, Option<Uuid>), DbError> {
        // 生成开始时间作为设置版本界线，晚于该时刻的变更必须触发下一轮重建。
        let build_started_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
            .fetch_one(&self.pool)
            .await?;
        let block_unhealthy_lines = self.subscription_blocks_unhealthy_lines().await?;
        if let Some(node_id) = reported_node_id {
            let mut tx = self.pool.begin().await?;
            let pools = sqlx::query_scalar::<_, Uuid>(
                "SELECT DISTINCT exit_pool_id FROM access_lines WHERE access_node_id=$1",
            )
            .bind(node_id)
            .fetch_all(&mut *tx)
            .await?;
            super::line_binding::reconcile_binding_lines_in_tx(&mut tx, &pools).await?;
            tx.commit().await?;
        }
        let revision = if let Some(node_id) = reported_node_id {
            sqlx::query_scalar::<_, i64>("SELECT config_revision FROM access_nodes WHERE id=$1")
                .bind(node_id)
                .fetch_optional(&self.pool)
                .await?
        } else {
            None
        };
        self.sync_assignments_for_active_users().await?;
        let data = self.load_store_data().await?;
        let node_id = selected_node_id(&data, reported_node_id);
        let response =
            heartbeat_json_with_probe_policy(&data, node_id, applied_hash, block_unhealthy_lines);
        if let Some(node) = node_id.and_then(|id| data.access_nodes.get(&id)) {
            let hash = response["desired_config_version"].as_str();
            let users: Vec<Uuid> = response["authorized_users"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|user| {
                    user["user_id"]
                        .as_str()
                        .and_then(|id| Uuid::parse_str(id).ok())
                })
                .collect();
            let applied = sqlx::query(
                r#"
                UPDATE access_nodes SET last_heartbeat_at=now(), desired_config_hash=$2,
                    applied_config_hash=COALESCE($3, applied_config_hash),
                    config_built_at=$4, config_authorized_user_ids=$5, config_built_revision=config_revision
                WHERE id=$1 AND ($6::bigint IS NULL OR config_revision=$6)
                "#,
            )
            .bind(node.id)
            .bind(hash)
            .bind(applied_hash)
            .bind(build_started_at)
            .bind(users)
            .bind(revision)
            .execute(&self.pool)
            .await?
            .rows_affected();
            if applied == 0 {
                // 不发布被并发修改覆盖的旧版本；Agent 保留已有配置，下次心跳安全重试。
                return Err(DbError::InvalidAgentPayload(
                    "配置正在变更，请在下一次心跳重试".to_string(),
                ));
            }
        }
        Ok((response, node_id))
    }
}
