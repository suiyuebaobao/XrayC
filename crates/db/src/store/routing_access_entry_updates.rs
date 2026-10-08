//! 入口管理更新和删除逻辑。
//! 本模块从入口创建模块拆出，避免单文件继续膨胀。
//! 更新入口时同步刷新所有绑定节点对应的 access_lines。
//! 删除入口或绑定时同步移除运行时 access_lines 与用户分配。
//! usage_ledgers 通过既有外键规则保留历史账本并置空入口引用。
//! 分组绑定节点依赖 access_entry_exit_bindings 外键级联清理。
//! 所有写操作都会标记相关中转节点待同步。
//! 本模块只访问 PostgreSQL，不访问远端服务器。
//! 错误文案保持中文，便于管理后台直接展示。
//! 本头部满足前十行中文注释约束。

use super::dirty::mark_access_node_dirty_in_tx;
use super::routing_access_entries::{
    ensure_listen_port_available_in_tx, prepare_access_entry,
    reseal_access_entry_inbound_config_for_line, StoredAccessEntry,
};
use super::routing_access_line_cleanup::detach_usage_for_access_lines_in_tx;
use super::routing_entries::{selected_exit_endpoint_in_tx, sync_single_endpoint_exit_pool_in_tx};
use super::routing_entry_cert::NodeCertAnchor;
use super::routing_entry_selected_domain::{
    resolve_selected_entry_domain_in_tx, SelectedEntryDomainHolder,
};
use crate::*;
use uuid::Uuid;

impl PgStore {
    pub async fn update_admin_access_entry(
        &self,
        access_entry_id: Uuid,
        input: AdminAccessEntryInput,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        // 一并读节点 cert_domain/cf_domain:更新时同样把直连 TLS 入口证书与 SNI 锚定灰云 cert_domain,
        // CF 入口证书一律锚 cf_domain 自己的证书(per-domain,不复用直连灰云;用户定:不复用别的域名证书)。
        let current = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>)>(
            r#"
            SELECT e.access_node_id, n.public_host, n.cert_domain, n.cf_domain
            FROM access_entries e
            JOIN access_nodes n ON n.id = e.access_node_id
            WHERE e.id = $1
            FOR UPDATE OF e
            "#,
        )
        .bind(access_entry_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidInput("入口不存在".to_string()))?;
        if input.access_node_id != current.0 {
            return Err(DbError::InvalidInput(
                "入口暂不支持跨中转节点移动，请新建入口后重新绑定出口".to_string(),
            ));
        }
        // 解析选中域名(多域名 Phase 2/3):护栏据 kind 判定,证书锚定据 domain/kind;不选则回退。
        let mut selected_holder = SelectedEntryDomainHolder::default();
        let selected = resolve_selected_entry_domain_in_tx(
            &mut tx,
            input.access_node_id,
            input.node_domain_id,
            &mut selected_holder,
        )
        .await?;
        let prepared = prepare_access_entry(
            &input,
            current.1,
            NodeCertAnchor {
                cert_domain: current.2.as_deref(),
                cf_domain: current.3.as_deref(),
            },
            selected,
        )?;
        // 更新换端口同样校验,但排除自身 id:原地不动或停用入口不应误报冲突。
        ensure_listen_port_available_in_tx(
            &mut tx,
            input.access_node_id,
            i32::from(input.listen_port),
            input.enabled,
            Some(access_entry_id),
        )
        .await?;
        let inbound_config = prepared.inbound_config;
        let name = required_admin_text(&input.name, "入口名称", 128)?;
        sqlx::query(
            r#"
            UPDATE access_entries
            SET name = $2,
                listen_host = $3,
                listen_port = $4,
                protocol = $5,
                transport = $6,
                security = $7,
                server_name = $8,
                public_key = $9,
                short_id = $10,
                flow = $11,
                udp_enabled = $12,
                udp_packet_encoding = $13,
                ws_path = $14,
                ws_host = $15,
                xhttp_path = $16,
                xhttp_host = $17,
                xhttp_mode = $18,
                cdn_enabled = $19,
                cdn_provider = $20,
                cdn_hostname = $21,
                cdn_server = $22,
                inbound_config = $23,
                enabled = $24,
                sort_weight = $25,
                node_domain_id = $26,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(access_entry_id)
        .bind(name)
        .bind(prepared.listen_host)
        .bind(i32::from(input.listen_port))
        .bind(prepared.protocol)
        .bind(prepared.transport)
        .bind(prepared.security)
        .bind(prepared.server_name)
        .bind(prepared.public_key)
        .bind(prepared.short_id)
        .bind(prepared.flow)
        .bind(prepared.udp_enabled)
        .bind(prepared.udp_packet_encoding)
        .bind(prepared.ws_path)
        .bind(prepared.ws_host)
        .bind(prepared.xhttp_path)
        .bind(prepared.xhttp_host)
        .bind(prepared.xhttp_mode)
        .bind(input.cdn_enabled)
        .bind(optional_admin_text(&input.cdn_provider, 32))
        .bind(optional_admin_text(&input.cdn_hostname, 255))
        .bind(optional_admin_text(&input.cdn_server, 255))
        .bind(inbound_config)
        .bind(input.enabled)
        .bind(input.sort_weight.clamp(0, 1_000_000))
        .bind(input.node_domain_id)
        .execute(&mut *tx)
        .await?;
        sync_access_lines_for_entry_in_tx(&mut tx, access_entry_id).await?;
        mark_access_node_dirty_in_tx(&mut tx, current.0, "admin_updated_access_entry").await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_admin_access_entry(&self, access_entry_id: Uuid) -> Result<u64, DbError> {
        let mut tx = self.pool.begin().await?;
        let access_node_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT access_node_id FROM access_entries WHERE id = $1 FOR UPDATE",
        )
        .bind(access_entry_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidInput("入口不存在".to_string()))?;
        let binding_ids = binding_ids_for_entry_in_tx(&mut tx, access_entry_id).await?;
        delete_runtime_lines_in_tx(&mut tx, &binding_ids).await?;
        let deleted = sqlx::query("DELETE FROM access_entries WHERE id = $1")
            .bind(access_entry_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        mark_access_node_dirty_in_tx(&mut tx, access_node_id, "admin_deleted_access_entry").await?;
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn update_admin_access_entry_exit_binding(
        &self,
        binding_id: Uuid,
        input: AdminAccessEntryExitBindingInput,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let entry = stored_entry_for_binding_in_tx(&mut tx, binding_id).await?;
        let exit_endpoint =
            selected_exit_endpoint_in_tx(&mut tx, entry.access_node_id, input.exit_endpoint_id)
                .await?;
        validate_hong_kong_hy2(&entry.protocol, &exit_endpoint.region_code)?;
        let exit_pool_id = sync_single_endpoint_exit_pool_in_tx(
            &mut tx,
            exit_endpoint.id,
            &exit_endpoint.label,
            &exit_endpoint.region_code,
        )
        .await?;
        let name = if input.name.trim().is_empty() {
            format!("{} / {}", entry.name, exit_endpoint.label)
        } else {
            required_admin_text(&input.name, "绑定节点名称", 128)?
        };
        sqlx::query(
            r#"
            UPDATE access_entry_exit_bindings
            SET exit_endpoint_id = $2,
                exit_pool_id = $3,
                name = $4,
                enabled = $5,
                sort_weight = $6,
                remark = $7,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(binding_id)
        .bind(exit_endpoint.id)
        .bind(exit_pool_id)
        .bind(&name)
        .bind(input.enabled)
        .bind(input.sort_weight.clamp(0, 1_000_000))
        .bind(optional_admin_text(&input.remark, 512))
        .execute(&mut *tx)
        .await?;
        let access_entry_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT access_entry_id FROM access_entry_exit_bindings WHERE id = $1",
        )
        .bind(binding_id)
        .fetch_one(&mut *tx)
        .await?;
        sync_access_lines_for_entry_in_tx(&mut tx, access_entry_id).await?;
        mark_access_node_dirty_in_tx(
            &mut tx,
            entry.access_node_id,
            "admin_updated_access_entry_exit_binding",
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_admin_access_entry_exit_binding(
        &self,
        binding_id: Uuid,
    ) -> Result<u64, DbError> {
        let mut tx = self.pool.begin().await?;
        let entry = stored_entry_for_binding_in_tx(&mut tx, binding_id).await?;
        delete_runtime_lines_in_tx(&mut tx, &[binding_id]).await?;
        let deleted = sqlx::query("DELETE FROM access_entry_exit_bindings WHERE id = $1")
            .bind(binding_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        mark_access_node_dirty_in_tx(
            &mut tx,
            entry.access_node_id,
            "admin_deleted_access_entry_exit_binding",
        )
        .await?;
        tx.commit().await?;
        Ok(deleted)
    }
}

async fn binding_ids_for_entry_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_entry_id: Uuid,
) -> Result<Vec<Uuid>, DbError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM access_entry_exit_bindings WHERE access_entry_id = $1",
    )
    .bind(access_entry_id)
    .fetch_all(&mut **tx)
    .await?)
}

async fn stored_entry_for_binding_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding_id: Uuid,
) -> Result<StoredAccessEntry, DbError> {
    sqlx::query_as::<_, StoredAccessEntry>(
        r#"
        SELECT e.access_node_id, e.name, e.listen_host, e.listen_port, e.protocol, e.transport,
               e.user_uuid, e.server_name, e.public_key, e.short_id, e.flow, e.udp_enabled,
               e.udp_packet_encoding, e.xhttp_path, e.xhttp_host, e.xhttp_mode,
               e.inbound_config, e.enabled, e.sort_weight
        FROM access_entry_exit_bindings b
        JOIN access_entries e ON e.id = b.access_entry_id
        WHERE b.id = $1
        FOR UPDATE OF b, e
        "#,
    )
    .bind(binding_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::InvalidInput("绑定节点不存在".to_string()))
}

async fn delete_runtime_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding_ids: &[Uuid],
) -> Result<(), DbError> {
    if binding_ids.is_empty() {
        return Ok(());
    }
    detach_usage_for_access_lines_in_tx(tx, binding_ids).await?;
    sqlx::query("DELETE FROM user_exit_assignments WHERE access_line_id = ANY($1)")
        .bind(binding_ids)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM user_access_line_assignments WHERE access_line_id = ANY($1)")
        .bind(binding_ids)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM access_lines WHERE id = ANY($1)")
        .bind(binding_ids)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn sync_access_lines_for_entry_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_entry_id: Uuid,
) -> Result<(), DbError> {
    let inbound_rows = sqlx::query_as::<_, (Uuid, serde_json::Value)>(
        r#"
        SELECT b.id, e.inbound_config
        FROM access_entry_exit_bindings b
        JOIN access_entries e ON e.id = b.access_entry_id
        WHERE b.access_entry_id = $1
        "#,
    )
    .bind(access_entry_id)
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE access_lines al
        SET name = b.name,
            access_node_id = e.access_node_id,
            exit_endpoint_id = b.exit_endpoint_id,
            exit_pool_id = b.exit_pool_id,
            listen_host = e.listen_host,
            listen_port = e.listen_port,
            protocol = e.protocol,
            transport = e.transport,
            user_uuid = e.user_uuid,
            server_name = e.server_name,
            public_key = e.public_key,
            short_id = e.short_id,
            enabled = e.enabled AND b.enabled,
            flow = e.flow,
            udp_enabled = e.udp_enabled,
            udp_packet_encoding = e.udp_packet_encoding,
            xhttp_path = e.xhttp_path,
            xhttp_host = e.xhttp_host,
            xhttp_mode = e.xhttp_mode,
            region_code = COALESCE(NULLIF(r.region_code, ''), 'GLOBAL'),
            region_name = COALESCE(NULLIF(ee.name, ''), r.name, ee.id::text),
            visibility_weight = e.sort_weight
        FROM access_entry_exit_bindings b
        JOIN access_entries e ON e.id = b.access_entry_id
        JOIN exit_endpoints ee ON ee.id = b.exit_endpoint_id
        JOIN exit_resources r ON r.id = ee.exit_resource_id
        WHERE al.id = b.id
          AND b.access_entry_id = $1
        "#,
    )
    .bind(access_entry_id)
    .execute(&mut **tx)
    .await?;
    for (binding_id, entry_inbound_config) in inbound_rows {
        let access_line_inbound_config =
            reseal_access_entry_inbound_config_for_line(entry_inbound_config)?;
        sqlx::query("UPDATE access_lines SET inbound_config = $2 WHERE id = $1")
            .bind(binding_id)
            .bind(access_line_inbound_config)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query(
        r#"
        DELETE FROM user_access_line_assignments
        WHERE access_line_id IN (
            SELECT b.id FROM access_entry_exit_bindings b WHERE b.access_entry_id = $1
        )
        "#,
    )
    .bind(access_entry_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        DELETE FROM user_exit_assignments
        WHERE access_line_id IN (
            SELECT b.id FROM access_entry_exit_bindings b WHERE b.access_entry_id = $1
        )
        "#,
    )
    .bind(access_entry_id)
    .execute(&mut **tx)
    .await?;
    let pool_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT DISTINCT exit_pool_id FROM access_entry_exit_bindings WHERE access_entry_id = $1",
    )
    .bind(access_entry_id)
    .fetch_all(&mut **tx)
    .await?;
    super::line_binding::prune_unusable_exit_pool_lines_in_tx(tx, &pool_ids).await?;
    Ok(())
}
