//! 中转节点、出口资源和出口端点写接口。
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
use super::routing_access_line_cleanup::detach_usage_for_access_lines_in_tx;
use super::rows::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn create_admin_exit_resource(
        &self,
        input: AdminExitResourceInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "出口资源名称", 128)?;
        let region_code = optional_admin_text(&input.region_code, 32);
        let provider_name = optional_admin_text(&input.provider_name, 128);
        let ownership = validate_ownership(&input.ownership)?;
        if ownership == "local_direct" {
            return Err(DbError::InvalidAgentPayload(
                "当前 V2 不支持新增 direct/local_direct 出口；历史记录仅用于只读兼容和遗留清理"
                    .to_string(),
            ));
        }
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_resources (
                name, region_code, provider_name, ownership, enabled, status
            )
            VALUES ($1, $2, $3, $4, $5, 'unknown')
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(region_code)
        .bind(provider_name)
        .bind(ownership)
        .bind(input.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_admin_exit_resource(
        &self,
        exit_resource_id: Uuid,
        input: AdminExitResourceUpdate,
    ) -> Result<(), DbError> {
        let name = optional_required_admin_text(input.name, "出口资源名称", 128)?;
        let region_code = input
            .region_code
            .map(|value| optional_admin_text(&value, 32));
        let provider_name = input
            .provider_name
            .map(|value| optional_admin_text(&value, 128));
        let ownership = input
            .ownership
            .as_deref()
            .map(validate_ownership)
            .transpose()?;
        if let Some(next_region_code) = &region_code {
            let has_hy2_endpoint = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM exit_endpoints
                    WHERE exit_resource_id = $1
                      AND outbound_type = 'hysteria'
                )
                "#,
            )
            .bind(exit_resource_id)
            .fetch_one(&self.pool)
            .await?;
            if has_hy2_endpoint {
                validate_hong_kong_hy2("hysteria", next_region_code)?;
            }
        }
        if ownership == Some("local_direct") {
            return Err(DbError::InvalidAgentPayload(
                "本机 direct 出口归属不能通过通用资源接口设置".to_string(),
            ));
        }
        if ownership.is_some() {
            let current_ownership = sqlx::query_scalar::<_, String>(
                "SELECT ownership FROM exit_resources WHERE id = $1",
            )
            .bind(exit_resource_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| {
                DbError::InvalidAgentPayload(format!("出口资源不存在: {exit_resource_id}"))
            })?;
            if current_ownership == "local_direct" {
                return Err(DbError::InvalidAgentPayload(
                    "本机 direct 出口不能通过通用资源接口变更归属".to_string(),
                ));
            }
        }
        let mut tx = self.pool.begin().await?;
        mark_nodes_dirty_for_resource_in_tx(
            &mut tx,
            exit_resource_id,
            "admin_updated_exit_resource",
        )
        .await?;
        let result = sqlx::query(
            r#"
            UPDATE exit_resources
            SET name = COALESCE($2, name),
                region_code = COALESCE($3, region_code),
                provider_name = COALESCE($4, provider_name),
                ownership = COALESCE($5, ownership),
                enabled = COALESCE($6, enabled)
            WHERE id = $1
            "#,
        )
        .bind(exit_resource_id)
        .bind(name)
        .bind(region_code)
        .bind(provider_name)
        .bind(ownership)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::InvalidAgentPayload(format!(
                "出口资源不存在: {exit_resource_id}"
            )));
        }
        let affected_pool_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT DISTINCT m.exit_pool_id
            FROM exit_pool_members m
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            WHERE e.exit_resource_id = $1
            "#,
        )
        .bind(exit_resource_id)
        .fetch_all(&mut *tx)
        .await?;
        prune_unusable_exit_pool_lines_in_tx(&mut tx, &affected_pool_ids).await?;
        mark_nodes_dirty_for_resource_in_tx(
            &mut tx,
            exit_resource_id,
            "admin_updated_exit_resource",
        )
        .await?;
        for pool_id in affected_pool_ids {
            mark_nodes_dirty_for_pool_in_tx(&mut tx, pool_id, "admin_updated_exit_resource")
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_admin_exit_endpoint(
        &self,
        input: AdminExitEndpointInput,
    ) -> Result<Uuid, DbError> {
        let outbound_type = validate_endpoint_type_name(&input.outbound_type)?;
        self.ensure_exit_endpoint_resource_compatible(input.exit_resource_id, outbound_type)
            .await?;
        let name = optional_admin_text(&input.name, 128);
        let host = if outbound_type == "direct" {
            optional_admin_text(&input.host, 255)
        } else {
            required_admin_text(&input.host, "出口地址", 255)?
        };
        if outbound_type != "direct" && input.port == 0 {
            return Err(DbError::InvalidAgentPayload(
                "出口端口必须大于 0".to_string(),
            ));
        }
        validate_exit_endpoint_protocol_config(outbound_type, &input.outbound_config)?;
        let outbound_config = input.outbound_config;
        let stream_config = input.stream_config;
        let probe_config = input.probe_config;
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_endpoints (
                exit_resource_id, name, outbound_type, host, port,
                outbound_config, stream_config, probe_config, enabled
            )
            VALUES ($1, $2, $3::endpoint_type, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
        )
        .bind(input.exit_resource_id)
        .bind(name)
        .bind(outbound_type)
        .bind(host)
        .bind(i32::from(input.port))
        .bind(outbound_config)
        .bind(stream_config)
        .bind(probe_config)
        .bind(input.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_admin_exit_endpoint(
        &self,
        exit_endpoint_id: Uuid,
        input: AdminExitEndpointUpdate,
    ) -> Result<(), DbError> {
        let current = sqlx::query_as::<_, ExitEndpointUpdateRow>(
            r#"
            SELECT exit_resource_id, outbound_type::text AS outbound_type, host, port, outbound_config
            FROM exit_endpoints
            WHERE id = $1
            "#,
        )
        .bind(exit_endpoint_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            DbError::InvalidAgentPayload(format!("出口端点不存在: {exit_endpoint_id}"))
        })?;

        let outbound_type = input
            .outbound_type
            .as_deref()
            .map(validate_endpoint_type_name)
            .transpose()?
            .unwrap_or(current.outbound_type.as_str());
        let host = input
            .host
            .as_deref()
            .unwrap_or(current.host.as_str())
            .trim();
        let port = input.port.unwrap_or(db_port_to_u16(
            current.port,
            "exit_endpoints.port",
            current.outbound_type == "direct",
        )?);
        if outbound_type != "direct" && host.is_empty() {
            return Err(DbError::InvalidAgentPayload("出口地址不能为空".to_string()));
        }
        if outbound_type != "direct" && port == 0 {
            return Err(DbError::InvalidAgentPayload(
                "出口端口必须大于 0".to_string(),
            ));
        }
        let exit_resource_id = input.exit_resource_id.unwrap_or(current.exit_resource_id);
        self.ensure_exit_endpoint_resource_compatible(exit_resource_id, outbound_type)
            .await?;
        let current_outbound_config = current.outbound_config;
        let next_outbound_config = input
            .outbound_config
            .clone()
            .unwrap_or(current_outbound_config);
        validate_exit_endpoint_protocol_config(outbound_type, &next_outbound_config)?;
        let name = input.name.map(|value| optional_admin_text(&value, 128));
        let host = if outbound_type == "direct" {
            optional_admin_text(host, 255)
        } else {
            required_admin_text(host, "出口地址", 255)?
        };
        let outbound_config = if input.outbound_config.is_some() {
            Some(next_outbound_config)
        } else {
            None
        };
        let stream_config = input.stream_config;
        let probe_config = input.probe_config;

        let mut tx = self.pool.begin().await?;
        ensure_exit_resource_exists_in_tx(&mut tx, exit_resource_id).await?;
        mark_nodes_dirty_for_endpoint_in_tx(
            &mut tx,
            exit_endpoint_id,
            "admin_updated_exit_endpoint",
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE exit_endpoints
            SET exit_resource_id = $2,
                name = COALESCE($3, name),
                outbound_type = $4::endpoint_type,
                host = $5,
                port = $6,
                outbound_config = COALESCE($7::jsonb, outbound_config),
                stream_config = COALESCE($8::jsonb, stream_config),
                probe_config = COALESCE($9::jsonb, probe_config),
                enabled = COALESCE($10, enabled)
            WHERE id = $1
            "#,
        )
        .bind(exit_endpoint_id)
        .bind(exit_resource_id)
        .bind(name)
        .bind(outbound_type)
        .bind(host)
        .bind(i32::from(port))
        .bind(outbound_config)
        .bind(stream_config)
        .bind(probe_config)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
        let affected_pool_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_pool_id FROM exit_pool_members WHERE exit_endpoint_id = $1",
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut *tx)
        .await?;
        ensure_exit_endpoint_compatible_with_bound_lines_in_tx(&mut tx, exit_endpoint_id).await?;
        prune_unusable_exit_pool_lines_in_tx(&mut tx, &affected_pool_ids).await?;
        mark_nodes_dirty_for_endpoint_in_tx(
            &mut tx,
            exit_endpoint_id,
            "admin_updated_exit_endpoint",
        )
        .await?;
        for pool_id in affected_pool_ids {
            mark_nodes_dirty_for_pool_in_tx(&mut tx, pool_id, "admin_updated_exit_endpoint")
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_admin_exit_endpoint(&self, exit_endpoint_id: Uuid) -> Result<u64, DbError> {
        let mut tx = self.pool.begin().await?;
        let resource_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_resource_id FROM exit_endpoints WHERE id = $1",
        )
        .bind(exit_endpoint_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            DbError::InvalidAgentPayload(format!("出口端点不存在: {exit_endpoint_id}"))
        })?;
        let affected_pool_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT exit_pool_id FROM exit_pool_members WHERE exit_endpoint_id = $1",
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut *tx)
        .await?;
        let deleted_access_line_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM access_lines WHERE exit_endpoint_id = $1 ORDER BY id",
        )
        .bind(exit_endpoint_id)
        .fetch_all(&mut *tx)
        .await?;

        // 删除出口会改变运行出口集合成员和用户出口映射，必须先脏标记再让外键级联清理。
        mark_nodes_dirty_for_endpoint_in_tx(
            &mut tx,
            exit_endpoint_id,
            "admin_deleted_exit_endpoint",
        )
        .await?;
        detach_usage_for_access_lines_in_tx(&mut tx, &deleted_access_line_ids).await?;
        sqlx::query("DELETE FROM user_exit_assignments WHERE exit_endpoint_id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM access_lines WHERE exit_endpoint_id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM exit_endpoints WHERE id = $1")
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
        for pool_id in &affected_pool_ids {
            mark_nodes_dirty_for_pool_in_tx(&mut tx, *pool_id, "admin_deleted_exit_endpoint")
                .await?;
        }
        if !affected_pool_ids.is_empty() {
            prune_unusable_exit_pool_lines_in_tx(&mut tx, &affected_pool_ids).await?;
        }
        sqlx::query(
            r#"
            DELETE FROM exit_resources r
            WHERE r.id = $1
              AND r.ownership = 'third_party'
              AND NOT EXISTS (
                  SELECT 1 FROM exit_endpoints e WHERE e.exit_resource_id = r.id
              )
            "#,
        )
        .bind(resource_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    pub(crate) async fn ensure_exit_endpoint_resource_compatible(
        &self,
        exit_resource_id: Uuid,
        outbound_type: &str,
    ) -> Result<(), DbError> {
        let (ownership, region_code) = sqlx::query_as::<_, (String, String)>(
            "SELECT ownership, region_code FROM exit_resources WHERE id = $1",
        )
        .bind(exit_resource_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            DbError::InvalidAgentPayload(format!("出口资源不存在: {exit_resource_id}"))
        })?;
        validate_hong_kong_hy2(outbound_type, &region_code)?;

        if outbound_type == "direct" && ownership != "local_direct" {
            return Err(DbError::InvalidAgentPayload(
                "当前 V2 不支持新增 direct/local_direct 出口端点".to_string(),
            ));
        }
        if ownership == "local_direct" && outbound_type != "direct" {
            return Err(DbError::InvalidAgentPayload(
                "历史 local_direct 资源只允许保留遗留 direct 记录，不能新增普通上游端点"
                    .to_string(),
            ));
        }

        Ok(())
    }
}
