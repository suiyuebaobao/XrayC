//! 内部运行集合和运行成员写接口。
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
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn create_admin_exit_pool(&self, input: AdminExitPoolInput) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "出口池名称", 128)?;
        let region_code = optional_admin_text(&input.region_code, 32);
        let strategy = validate_pool_strategy(&input.strategy)?;
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_pools (
                name, region_code, strategy, enabled, updated_at
            )
            VALUES ($1, $2, $3, $4, now())
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(region_code)
        .bind(strategy)
        .bind(input.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_admin_exit_pool(
        &self,
        exit_pool_id: Uuid,
    ) -> Result<AdminExitPoolDeleteResult, DbError> {
        let mut tx = self.pool.begin().await?;
        let exists =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM exit_pools WHERE id = $1 FOR UPDATE")
                .bind(exit_pool_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(DbError::InvalidInput("内部运行集合不存在".to_string()));
        }

        let affected_node_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT DISTINCT node_id
            FROM (
                SELECT access_node_id AS node_id
                FROM access_lines
                WHERE exit_pool_id = $1
                UNION
                SELECT r.access_node_id AS node_id
                FROM exit_pool_members m
                JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
                JOIN exit_resources r ON r.id = e.exit_resource_id
                WHERE m.exit_pool_id = $1
                  AND r.ownership = 'self_hosted'
                  AND r.access_node_id IS NOT NULL
            ) affected_nodes
            WHERE node_id IS NOT NULL
            ORDER BY node_id
            "#,
        )
        .bind(exit_pool_id)
        .fetch_all(&mut *tx)
        .await?;
        let deleted_access_line_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM access_lines WHERE exit_pool_id = $1 ORDER BY id",
        )
        .bind(exit_pool_id)
        .fetch_all(&mut *tx)
        .await?;
        let retained_usage_ledger_count = if deleted_access_line_ids.is_empty() {
            0
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM usage_ledgers WHERE access_line_id = ANY($1)",
            )
            .bind(&deleted_access_line_ids)
            .fetch_one(&mut *tx)
            .await?
        };
        detach_usage_for_access_lines_in_tx(&mut tx, &deleted_access_line_ids).await?;

        let deleted_assignment_count =
            sqlx::query("DELETE FROM user_exit_assignments WHERE exit_pool_id = $1")
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
        let deleted_access_line_count =
            sqlx::query("DELETE FROM access_lines WHERE exit_pool_id = $1")
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
        let deleted_member_count =
            sqlx::query("DELETE FROM exit_pool_members WHERE exit_pool_id = $1")
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
        let deleted_pool_count = sqlx::query("DELETE FROM exit_pools WHERE id = $1")
            .bind(exit_pool_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted_pool_count == 0 {
            return Err(DbError::InvalidInput("内部运行集合不存在".to_string()));
        }
        for node_id in &affected_node_ids {
            mark_access_node_dirty_in_tx(&mut tx, *node_id, "admin_deleted_exit_pool").await?;
        }
        tx.commit().await?;
        Ok(AdminExitPoolDeleteResult {
            deleted: true,
            deleted_access_line_count,
            deleted_member_count,
            deleted_assignment_count,
            retained_usage_ledger_count,
            affected_access_node_count: affected_node_ids.len() as i64,
        })
    }

    pub async fn replace_admin_exit_pool_members(
        &self,
        exit_pool_id: Uuid,
        members: Vec<AdminExitPoolMemberInput>,
    ) -> Result<usize, DbError> {
        let mut tx = self.pool.begin().await?;
        ensure_exit_pool_exists_in_tx(&mut tx, exit_pool_id).await?;
        let endpoint_ids = members
            .iter()
            .map(|member| member.exit_endpoint_id)
            .collect::<Vec<_>>();
        ensure_unique_uuids(&endpoint_ids, "运行成员重复")?;

        if endpoint_ids.is_empty() {
            sqlx::query("DELETE FROM exit_pool_members WHERE exit_pool_id = $1")
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE exit_pool_id = $1")
                .bind(exit_pool_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query(
                r#"
                DELETE FROM exit_pool_members
                WHERE exit_pool_id = $1 AND NOT (exit_endpoint_id = ANY($2))
                "#,
            )
            .bind(exit_pool_id)
            .bind(&endpoint_ids)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                DELETE FROM user_exit_assignments
                WHERE exit_pool_id = $1 AND NOT (exit_endpoint_id = ANY($2))
                "#,
            )
            .bind(exit_pool_id)
            .bind(&endpoint_ids)
            .execute(&mut *tx)
            .await?;
        }
        ensure_exit_pool_member_set_compatible_with_bound_lines_in_tx(
            &mut tx,
            exit_pool_id,
            &endpoint_ids,
        )
        .await?;

        for member in members {
            ensure_exit_endpoint_exists_in_tx(&mut tx, member.exit_endpoint_id).await?;
            let status = validate_member_status(&member.status)?;
            sqlx::query(
                r#"
                INSERT INTO exit_pool_members (
                    exit_pool_id, exit_endpoint_id, weight, status,
                    priority, allow_new_assignments, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, now())
                ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                    weight = EXCLUDED.weight,
                    status = EXCLUDED.status,
                    priority = EXCLUDED.priority,
                    allow_new_assignments = EXCLUDED.allow_new_assignments,
                    updated_at = now()
                "#,
            )
            .bind(exit_pool_id)
            .bind(member.exit_endpoint_id)
            .bind(member.weight.max(1))
            .bind(status)
            .bind(member.priority.max(0))
            .bind(member.allow_new_assignments)
            .execute(&mut *tx)
            .await?;
        }
        prune_unusable_exit_pool_lines_in_tx(&mut tx, &[exit_pool_id]).await?;
        mark_nodes_dirty_for_pool_in_tx(&mut tx, exit_pool_id, "admin_updated_exit_pool_members")
            .await?;
        tx.commit().await?;
        Ok(endpoint_ids.len())
    }

    pub async fn create_admin_exit_pool_exit(
        &self,
        exit_pool_id: Uuid,
        input: AdminExitPoolExitInput,
    ) -> Result<AdminExitPoolExitCreated, DbError> {
        let resource_name = required_admin_text(&input.resource_name, "出口名称", 128)?;
        let endpoint_name = optional_admin_text(&input.endpoint_name, 128);
        let region_code = optional_admin_text(&input.region_code, 32);
        let provider_name = optional_admin_text(&input.provider_name, 128);
        let ownership = validate_ownership(&input.ownership)?;
        if ownership == "local_direct" {
            return Err(DbError::InvalidAgentPayload(
                "当前 V2 不支持新增 direct/local_direct 出口；请使用第三方或普通自建上游出口"
                    .to_string(),
            ));
        }
        let outbound_type = validate_endpoint_type_name(&input.outbound_type)?;
        if outbound_type == "direct" {
            return Err(DbError::InvalidAgentPayload(
                "当前 V2 不支持新增 direct 出口；请使用 SOCKS/HTTP/VLESS/Trojan/Shadowsocks/HY2 上游"
                    .to_string(),
            ));
        }
        let host = required_admin_text(&input.host, "出口地址", 255)?;
        if input.port == 0 {
            return Err(DbError::InvalidAgentPayload(
                "出口端口必须大于 0".to_string(),
            ));
        }
        validate_exit_endpoint_protocol_config(outbound_type, &input.outbound_config)?;
        let outbound_config = input.outbound_config;
        let stream_config = input.stream_config;
        let probe_config = input.probe_config;
        let status = validate_member_status(&input.status)?;

        let mut tx = self.pool.begin().await?;
        ensure_exit_pool_exists_in_tx(&mut tx, exit_pool_id).await?;
        let exit_resource_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_resources (
                name, region_code, provider_name, ownership, enabled, status
            )
            VALUES ($1, $2, $3, $4, $5, 'unknown')
            RETURNING id
            "#,
        )
        .bind(resource_name)
        .bind(region_code)
        .bind(provider_name)
        .bind(ownership)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await?;

        let exit_endpoint_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_endpoints (
                exit_resource_id, name, outbound_type, host, port,
                outbound_config, stream_config, probe_config, enabled
            )
            VALUES ($1, $2, $3::endpoint_type, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
        )
        .bind(exit_resource_id)
        .bind(endpoint_name)
        .bind(outbound_type)
        .bind(host)
        .bind(i32::from(input.port))
        .bind(outbound_config)
        .bind(stream_config)
        .bind(probe_config)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await?;

        ensure_exit_pool_member_set_compatible_with_bound_lines_in_tx(
            &mut tx,
            exit_pool_id,
            &[exit_endpoint_id],
        )
        .await?;
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status,
                priority, allow_new_assignments, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, now())
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                weight = EXCLUDED.weight,
                status = EXCLUDED.status,
                priority = EXCLUDED.priority,
                allow_new_assignments = EXCLUDED.allow_new_assignments,
                updated_at = now()
            "#,
        )
        .bind(exit_pool_id)
        .bind(exit_endpoint_id)
        .bind(input.weight.max(1))
        .bind(status)
        .bind(input.priority.max(0))
        .bind(input.allow_new_assignments)
        .execute(&mut *tx)
        .await?;
        mark_nodes_dirty_for_pool_in_tx(&mut tx, exit_pool_id, "admin_created_exit_pool_exit")
            .await?;
        tx.commit().await?;

        Ok(AdminExitPoolExitCreated {
            exit_resource_id,
            exit_endpoint_id,
        })
    }
}
