//! 中转入口和节点出口池绑定写接口。
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
use super::probes::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::Value;
use uuid::Uuid;

impl PgStore {
    pub async fn create_admin_access_line(
        &self,
        input: AdminAccessLineInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "中转入口名称", 128)?;
        let listen_host = required_admin_text(&input.listen_host, "监听地址", 255)?;
        if input.listen_port == 0 {
            return Err(DbError::InvalidAgentPayload(
                "监听端口必须大于 0".to_string(),
            ));
        }
        let protocol = validate_access_protocol(&input.protocol)?;
        let transport = validate_access_transport(&input.transport)?;
        validate_access_protocol_transport(protocol, transport)?;
        validate_hong_kong_hy2(protocol, &input.region_code)?;
        let udp_packet_encoding = normalize_udp_packet_encoding(&input.udp_packet_encoding)?;
        let user_uuid = if input.user_uuid.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            required_admin_text(&input.user_uuid, "线路 UUID", 128)?
        };
        let xhttp_mode = validate_xhttp_mode(&input.xhttp_mode)?;
        let identity_mode = validate_identity_mode(&input.identity_mode)?;
        let user_key_source = validate_user_key_source(&input.user_key_source)?;
        let inbound_config =
            normalize_access_inbound_config(protocol, input.inbound_config, &input.server_name)?;
        validate_access_inbound_config(protocol, &inbound_config, &input.server_name)?;

        let mut tx = self.pool.begin().await?;
        ensure_access_node_exists_in_tx(&mut tx, input.access_node_id).await?;
        ensure_exit_pool_exists_in_tx(&mut tx, input.exit_pool_id).await?;
        ensure_exit_pool_compatible_with_access_node_in_tx(
            &mut tx,
            input.exit_pool_id,
            input.access_node_id,
        )
        .await?;
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO access_lines (
                name, access_node_id, exit_pool_id, listen_host, listen_port,
                protocol, transport, user_uuid, server_name, public_key, short_id,
                enabled, region_code, region_name, region_flag,
                flow, udp_enabled, udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode,
                identity_mode, user_key_source, inbound_config, visibility_weight
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16, $17, $18, $19,
                $20, $21, $22, $23, $24, $25
            )
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(input.access_node_id)
        .bind(input.exit_pool_id)
        .bind(listen_host)
        .bind(i32::from(input.listen_port))
        .bind(protocol)
        .bind(transport)
        .bind(user_uuid)
        .bind(optional_admin_text(&input.server_name, 255))
        .bind(optional_admin_text(&input.public_key, 255))
        .bind(optional_admin_text(&input.short_id, 64))
        .bind(input.enabled)
        .bind(optional_admin_text(&input.region_code, 32))
        .bind(optional_admin_text(&input.region_name, 64))
        .bind(optional_admin_text(&input.region_flag, 16))
        .bind(optional_admin_text(&input.flow, 64))
        .bind(input.udp_enabled)
        .bind(udp_packet_encoding)
        .bind(optional_admin_text(&input.xhttp_path, 255))
        .bind(optional_admin_text(&input.xhttp_host, 255))
        .bind(xhttp_mode)
        .bind(identity_mode)
        .bind(user_key_source)
        .bind(inbound_config)
        .bind(input.visibility_weight.max(0))
        .fetch_one(&mut *tx)
        .await?;
        mark_access_node_dirty_in_tx(&mut tx, input.access_node_id, "admin_created_access_line")
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn update_admin_access_line_exit_pool(
        &self,
        access_line_id: Uuid,
        exit_pool_id: Uuid,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        ensure_exit_pool_exists_in_tx(&mut tx, exit_pool_id).await?;
        let access_node_id =
            sqlx::query_scalar::<_, Uuid>("SELECT access_node_id FROM access_lines WHERE id = $1")
                .bind(access_line_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(DbError::AccessLineNotFound)?;
        ensure_exit_pool_compatible_with_access_node_in_tx(&mut tx, exit_pool_id, access_node_id)
            .await?;
        sqlx::query("UPDATE access_lines SET exit_pool_id = $2 WHERE id = $1")
            .bind(access_line_id)
            .bind(exit_pool_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM user_exit_assignments WHERE access_line_id = $1")
            .bind(access_line_id)
            .execute(&mut *tx)
            .await?;
        mark_access_node_dirty_in_tx(
            &mut tx,
            access_node_id,
            "admin_updated_access_line_exit_pool",
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_admin_access_node_deploy_failed(
        &self,
        access_node_id: Uuid,
        access_node_created: bool,
        exit_resource_id: Option<Uuid>,
        exit_endpoint_id: Option<Uuid>,
        exit_pool_id: Option<Uuid>,
        access_line_id: Option<Uuid>,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;

        if access_node_created {
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET status = 'offline',
                    config_dirty = TRUE,
                    config_dirty_at = now(),
                    desired_config_hash = NULL,
                    config_dirty_reason = 'admin_deploy_failed'
                WHERE id = $1
                "#,
            )
            .bind(access_node_id)
            .execute(&mut *tx)
            .await?;
        } else {
            mark_access_node_dirty_in_tx(&mut tx, access_node_id, "admin_deploy_failed").await?;
        }

        if let Some(access_line_id) = access_line_id {
            sqlx::query(
                r#"
                UPDATE access_lines
                SET enabled = FALSE
                WHERE id = $1 AND access_node_id = $2
                "#,
            )
            .bind(access_line_id)
            .bind(access_node_id)
            .execute(&mut *tx)
            .await?;
        }

        if let Some(exit_pool_id) = exit_pool_id {
            sqlx::query(
                r#"
                UPDATE exit_pools
                SET enabled = FALSE,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(exit_pool_id)
            .execute(&mut *tx)
            .await?;

            if let Some(exit_endpoint_id) = exit_endpoint_id {
                sqlx::query(
                    r#"
                    UPDATE exit_pool_members
                    SET status = 'offline',
                        allow_new_assignments = FALSE,
                        updated_at = now()
                    WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
                    "#,
                )
                .bind(exit_pool_id)
                .bind(exit_endpoint_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    r#"
                    UPDATE exit_pool_members
                    SET status = 'offline',
                        allow_new_assignments = FALSE,
                        updated_at = now()
                    WHERE exit_pool_id = $1
                    "#,
                )
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?;
            }
        }

        if let Some(exit_endpoint_id) = exit_endpoint_id {
            if let Some(exit_resource_id) = exit_resource_id {
                sqlx::query(
                    r#"
                    UPDATE exit_endpoints
                    SET enabled = FALSE
                    WHERE id = $1 AND exit_resource_id = $2
                    "#,
                )
                .bind(exit_endpoint_id)
                .bind(exit_resource_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    r#"
                    UPDATE exit_endpoints
                    SET enabled = FALSE
                    WHERE id = $1
                    "#,
                )
                .bind(exit_endpoint_id)
                .execute(&mut *tx)
                .await?;
            }
        }

        if let Some(exit_resource_id) = exit_resource_id {
            sqlx::query(
                r#"
                UPDATE exit_resources
                SET enabled = FALSE,
                    status = 'offline',
                    last_endpoint_error = 'admin deploy failed'
                WHERE id = $1
                  AND ownership IN ('local_direct', 'self_hosted')
                  AND access_node_id = $2
                "#,
            )
            .bind(exit_resource_id)
            .bind(access_node_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
