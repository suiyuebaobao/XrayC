//! 中转节点管理写接口。
//! 本文件承载节点编辑、删除和批量删除。
//! 删除只处理控制台数据和从属入口，远端清理必须走部署或运维脚本。
//! 所有写操作保持事务边界，新增字段需同步前端和接口文档。
use super::routing_access_line_cleanup::detach_usage_for_access_lines_in_tx;
use crate::*;
use uuid::Uuid;
impl PgStore {
    pub async fn update_admin_access_node(
        &self,
        access_node_id: Uuid,
        input: AdminAccessNodeUpdate,
    ) -> Result<(), DbError> {
        let name = optional_required_admin_text(input.name, "中转节点名称", 128)?;
        let public_host = optional_required_admin_text(input.public_host, "中转节点地址", 255)?;
        let ssh_host = input.ssh_host.map(|value| optional_admin_text(&value, 255));
        if input.public_port == Some(0) {
            return Err(DbError::InvalidInput(
                "中转节点端口必须在 1-65535 之间".to_string(),
            ));
        }
        let remark = input.remark.map(|value| optional_admin_text(&value, 512));
        // 节点多模式 CF 字段:None 表示本次不更新该列;有值时裁剪空白后写入(空串归一为 NULL)。
        // 内层 Option 区分 Some("")→清空(NULL)与具体域名;外层 Option 区分是否提供本字段。
        let cert_domain = input
            .cert_domain
            .map(|value| node_text_to_nullable(&value, 255));
        let acme_email = input
            .acme_email
            .map(|value| node_text_to_nullable(&value, 255));
        let cf_domain = input
            .cf_domain
            .map(|value| node_text_to_nullable(&value, 255));
        let ip_direct_address = input
            .ip_direct_address
            .map(|value| node_text_to_nullable(&value, 255));
        // 本次提供了 cf_domain 时,cf_enabled 与 cf_cert_mode 按 cf_domain 派生(忽略入参 cf_enabled);
        // 未提供 cf_domain 时沿用入参 cf_enabled(兼容只切开关的旧路径)。
        let cf_enabled = match cf_domain.as_ref() {
            Some(value) => Some(value.is_some()),
            None => input.cf_enabled,
        };
        if name.is_none()
            && public_host.is_none()
            && input.public_port.is_none()
            && ssh_host.is_none()
            && remark.is_none()
            && cert_domain.is_none()
            && acme_email.is_none()
            && cf_domain.is_none()
            && ip_direct_address.is_none()
            && cf_enabled.is_none()
        {
            return Err(DbError::InvalidInput(
                "没有可更新的中转节点字段".to_string(),
            ));
        }
        // 留存「本次是否提供了域名列」,下方 .bind() 会消费 cert_domain/cf_domain。
        let provided_domain_columns = cert_domain.is_some() || cf_domain.is_some();
        let affected = sqlx::query(
            r#"
            UPDATE access_nodes
            SET name = COALESCE($2, name),
                public_host = COALESCE($3, public_host),
                public_port = COALESCE($4, public_port),
                ssh_host = COALESCE($5, ssh_host),
                remark = COALESCE($6, remark),
                cert_domain = CASE WHEN $7::boolean THEN $8 ELSE cert_domain END,
                acme_email = CASE WHEN $9::boolean THEN $10 ELSE acme_email END,
                cf_domain = CASE WHEN $11::boolean THEN $12 ELSE cf_domain END,
                cf_enabled = COALESCE($13, cf_enabled),
                ip_direct_address = CASE WHEN $14::boolean THEN $15 ELSE ip_direct_address END,
                -- CF 证书模式:本次提供了 cf_domain($11)就一律写 reuse_direct(token-less 免 token,与
                -- create/rebind 的 derived_cf_cert_mode、node_domains cf 主行、agent 默认完全一致);不提供则保留原值。
                -- 关键修正:acme 是直连证书 HTTP-01 也要用的邮箱、不是 DNS-01 凭据信号(DNS-01 给 cf_domain 签证书需
                -- CF API token),旧实现「有 cf_domain(且有 acme)→dns01」会把有直连证书的 token-less CF 节点误判成
                -- dns01 → 入口证书锚到签不出的 cf_domain DNS-01 路径、被 BUG-D 跳过。要 dns01 由运维在 cf 域名上显式设。
                cf_cert_mode = CASE WHEN $11::boolean THEN 'reuse_direct' ELSE cf_cert_mode END,
                config_dirty = TRUE,
                config_dirty_at = now(),
                desired_config_hash = NULL,
                config_dirty_reason = 'admin_updated_access_node'
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .bind(name)
        .bind(public_host)
        .bind(input.public_port.map(i32::from))
        .bind(ssh_host)
        .bind(remark)
        // 用「是否提供」布尔位区分 None(不更新)与 Some(写入,含清空为 NULL)。
        .bind(cert_domain.is_some())
        .bind(cert_domain.flatten())
        .bind(acme_email.is_some())
        .bind(acme_email.flatten())
        .bind(cf_domain.is_some())
        .bind(cf_domain.flatten())
        .bind(cf_enabled)
        .bind(ip_direct_address.is_some())
        .bind(ip_direct_address.flatten())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(DbError::InvalidAgentPayload(format!(
                "中转节点不存在: {access_node_id}"
            )));
        }
        // 更新涉及域名列时,按更新后的实际单字段值同步 node_domains 主行(多域名表为准)。
        // 直接读回 access_nodes 已应用的列值,避免在此重算 CASE 内/外层 Option 语义。
        if provided_domain_columns {
            let applied = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
                "SELECT cert_domain, cf_domain, acme_email FROM access_nodes WHERE id = $1",
            )
            .bind(access_node_id)
            .fetch_one(&self.pool)
            .await?;
            self.sync_primary_node_domains(
                access_node_id,
                applied.0.as_deref(),
                applied.1.as_deref(),
                applied.2.as_deref(),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn rotate_admin_access_node_agent_token(
        &self,
        access_node_id: Uuid,
        agent_token: &str,
    ) -> Result<(), DbError> {
        let agent_token = required_admin_text(agent_token, "Agent Token", 255)?;
        let affected = sqlx::query(
            r#"
            UPDATE access_nodes
                SET agent_token_hash = $2,
                    config_dirty = TRUE,
                    config_dirty_at = now(),
                    desired_config_hash = NULL,
                    config_dirty_reason = 'admin_redeploy_rotated_agent_token'
                WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .bind(agent_token_hash(&agent_token))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::InvalidAgentPayload(format!(
                "中转节点不存在: {access_node_id}"
            )));
        }
        Ok(())
    }

    pub async fn delete_admin_access_nodes(
        &self,
        access_node_ids: Vec<Uuid>,
    ) -> Result<AdminAccessNodeDeleteResult, DbError> {
        if access_node_ids.is_empty() {
            return Err(DbError::InvalidInput("请选择要删除的中转节点".to_string()));
        }

        let mut ids = access_node_ids;
        ids.sort_unstable();
        ids.dedup();

        let mut tx = self.pool.begin().await?;
        let existing_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM access_nodes
            WHERE id = ANY($1)
            ORDER BY id
            FOR UPDATE
            "#,
        )
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;

        if existing_ids.is_empty() {
            return Err(DbError::InvalidAgentPayload("中转节点不存在".to_string()));
        }

        let deleted_access_line_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT DISTINCT line_id
            FROM (
                SELECT l.id AS line_id
                FROM access_lines l
                WHERE l.access_node_id = ANY($1)
                UNION
                SELECT l.id AS line_id
                FROM access_lines l
                JOIN exit_endpoints e ON e.id = l.exit_endpoint_id
                JOIN exit_resources r ON r.id = e.exit_resource_id
                WHERE r.access_node_id = ANY($1)
            ) deleted_lines
            ORDER BY line_id
            "#,
        )
        .bind(&existing_ids)
        .fetch_all(&mut *tx)
        .await?;
        let deleted_access_line_count = deleted_access_line_ids.len() as i64;

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

        let deleted_local_resource_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM exit_resources WHERE access_node_id = ANY($1)",
        )
        .bind(&existing_ids)
        .fetch_one(&mut *tx)
        .await?;

        let local_pool_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT DISTINCT m.exit_pool_id
            FROM exit_pool_members m
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE r.access_node_id = ANY($1)
            "#,
        )
        .bind(&existing_ids)
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM access_nodes WHERE id = ANY($1)")
            .bind(&existing_ids)
            .execute(&mut *tx)
            .await?;

        let deleted_local_pool_count = if local_pool_ids.is_empty() {
            0
        } else {
            sqlx::query(
                r#"
                DELETE FROM exit_pools p
                WHERE p.id = ANY($1)
                  AND NOT EXISTS (
                      SELECT 1 FROM exit_pool_members m WHERE m.exit_pool_id = p.id
                  )
                "#,
            )
            .bind(&local_pool_ids)
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64
        };

        tx.commit().await?;
        Ok(AdminAccessNodeDeleteResult {
            deleted_node_count: existing_ids.len() as i64,
            deleted_node_ids: existing_ids,
            deleted_access_line_count,
            retained_usage_ledger_count,
            deleted_local_resource_count,
            deleted_local_pool_count,
        })
    }
}

/// 节点文本字段归一为可空值:裁剪并按 max 截断,空串转 None(写入 NULL)。
fn node_text_to_nullable(value: &str, max: usize) -> Option<String> {
    let normalized = optional_admin_text(value, max);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
