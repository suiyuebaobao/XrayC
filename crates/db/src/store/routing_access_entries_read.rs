//! 入口与入口出口绑定的只读 JSON 聚合查询。
//! 从 routing_access_entries 拆出,避免主写入文件超过 550 行硬上限。
//! 这里只做 jsonb_agg 聚合读出,供管理后台列表展示,不写库、不访问远端。
//! 字段口径与 access_entries / access_entry_exit_bindings 表列对齐。
//! 出口名兜底取 endpoint/resource 名,绝不回显出口真实主机/凭据。
//! 多域名 Phase 5b:入口读模型 SELECT 出 node_domain_id(未选 null),并 LEFT JOIN 解析 node_domain。
//! 所有查询走 self.pool,与其他 store 一致的错误封装。
//! 由 store::mod 挂载,PgStore 方法对 API 层可见。
//! 注释保持中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use crate::*;
use serde_json::Value;

impl PgStore {
    pub async fn list_admin_access_entries_json(&self) -> Result<Value, DbError> {
        let value = sqlx::query_scalar::<_, Value>(
            r#"
            SELECT COALESCE(
                jsonb_agg(
                    jsonb_build_object(
                        'id', e.id,
                        'access_node_id', e.access_node_id,
                        'access_node_name', n.name,
                        'name', e.name,
                        'listen_host', e.listen_host,
                        'listen_port', e.listen_port,
                        'protocol', e.protocol,
                        'transport', e.transport,
                        'security', e.security,
                        'server_name', e.server_name,
                        'ws_path', e.ws_path,
                        'ws_host', e.ws_host,
                        'cdn_enabled', e.cdn_enabled,
                        'cdn_provider', e.cdn_provider,
                        'cdn_hostname', e.cdn_hostname,
                        'cdn_server', e.cdn_server,
                        'enabled', e.enabled,
                        'sort_weight', e.sort_weight,
                        'created_at', e.created_at,
                        'updated_at', e.updated_at,
                        'node_domain_id', e.node_domain_id,
                        'vless_quantum_encryption',
                            COALESCE((e.inbound_config->>'vless_quantum_encryption')::boolean, false),
                        'node_domain', CASE WHEN nd.id IS NULL THEN NULL ELSE jsonb_build_object(
                            'id', nd.id,
                            'domain', nd.domain,
                            'kind', nd.kind,
                            'is_primary', nd.is_primary,
                            'cert_status', nd.cert_status
                        ) END
                    )
                    ORDER BY n.name, e.sort_weight, e.name, e.id
                ),
                '[]'::jsonb
            )
            FROM access_entries e
            JOIN access_nodes n ON n.id = e.access_node_id
            LEFT JOIN node_domains nd ON nd.id = e.node_domain_id
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(value)
    }

    pub async fn list_admin_access_entry_exit_bindings_json(&self) -> Result<Value, DbError> {
        let value = sqlx::query_scalar::<_, Value>(
            r#"
            SELECT COALESCE(
                jsonb_agg(
                    jsonb_build_object(
                        'id', b.id,
                        'access_entry_id', b.access_entry_id,
                        'access_entry_name', e.name,
                        'access_node_id', e.access_node_id,
                        'access_node_name', n.name,
                        'exit_endpoint_id', b.exit_endpoint_id,
                        'exit_endpoint_name', COALESCE(NULLIF(ee.name, ''), er.name, ee.id::text),
                        'exit_pool_id', b.exit_pool_id,
                        'exit_pool_name', COALESCE(ep.name, ''),
                        'name', b.name,
                        'enabled', b.enabled,
                        'sort_weight', b.sort_weight,
                        'remark', b.remark,
                        'created_at', b.created_at,
                        'updated_at', b.updated_at
                    )
                    ORDER BY n.name, e.sort_weight, e.name, b.sort_weight, b.name, b.id
                ),
                '[]'::jsonb
            )
            FROM access_entry_exit_bindings b
            JOIN access_entries e ON e.id = b.access_entry_id
            JOIN access_nodes n ON n.id = e.access_node_id
            JOIN exit_endpoints ee ON ee.id = b.exit_endpoint_id
            JOIN exit_resources er ON er.id = ee.exit_resource_id
            LEFT JOIN exit_pools ep ON ep.id = b.exit_pool_id
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(value)
    }
}
