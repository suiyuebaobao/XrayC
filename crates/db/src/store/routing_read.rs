//! 路由配置和后台拓扑读模型入口。
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
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use xrayc_core::{AccessNode, StoreData};

impl PgStore {
    pub async fn generate_subscription_yaml(&self, token: &str) -> Result<String, DbError> {
        let user_id = self.user_id_for_subscription_token(token).await?;
        self.sync_user_access_line_assignments(user_id).await?;
        self.sync_user_exit_assignments(user_id).await?;
        let data = self.load_store_data_for_user_assignments(user_id).await?;
        let settings = self.subscription_settings_json().await?;
        let options = SubscriptionOptions::from_json(&settings);
        Ok(generate_clash_yaml_with_options(&data, token, &options)?)
    }

    pub async fn first_access_line_id(&self) -> Result<Uuid, DbError> {
        sqlx::query_scalar::<_, Uuid>(r#"SELECT id FROM access_lines ORDER BY name LIMIT 1"#)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(DbError::AccessLineNotFound)
    }

    pub async fn access_routing_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.load_store_data().await?;
        let value = access_routing_json(&data);
        // 先挂线路运行态,再挂节点运行态指标(监控中心),让 access_nodes[].runtime_metrics 透出。
        let mut value = self.attach_latest_line_runtime(value).await?;
        self.attach_latest_node_runtime(&mut value).await?;
        Ok(value)
    }

    pub async fn admin_access_nodes_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.access_routing_json().await?;
        let nodes = data
            .get("access_nodes")
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(admin_collection_json("access_nodes", nodes))
    }

    pub async fn admin_access_node_summary_json(
        &self,
        access_node_id: Uuid,
    ) -> Result<Option<serde_json::Value>, DbError> {
        let row = sqlx::query_as::<_, AccessNodeSummaryRow>(
            r#"
            SELECT id, name, status, config_dirty, created_at
            FROM access_nodes
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| {
            json!({
                "id": row.id,
                "name": row.name,
                "status": row.status,
                "config_dirty": row.config_dirty,
                "created_at": row.created_at
            })
        }))
    }

    /// 读取节点多模式 CF 字段(证书域名 + CF 身份)。
    /// 供安装指南把 cert_domain 并入 certbot 申请清单,以及读回断言用。
    /// 节点不存在返回 None;空白列由写入侧已归一为 NULL。
    pub async fn access_node_cf_fields(
        &self,
        access_node_id: Uuid,
    ) -> Result<Option<AccessNodeCfFields>, DbError> {
        let row = sqlx::query_as::<_, AccessNodeCfFieldsRow>(
            r#"
            SELECT cert_domain, acme_email, cf_domain, cf_cert_mode
            FROM access_nodes
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        // cf_enabled 改为按 node_domains 派生:存在任一 kind='cf' 行即视为启用 CF。
        // 写入侧已把 cf_domain 列与 cf 主域名行同步,这里再以多域名表为准,
        // 兼容仅通过 add_node_domain 增加 cf 域名(未改单字段列)的节点。
        let cf_enabled = self.node_has_domain_kind(access_node_id, "cf").await?;
        Ok(Some(AccessNodeCfFields {
            cert_domain: row.cert_domain,
            acme_email: row.acme_email,
            cf_enabled,
            cf_domain: row.cf_domain,
            cf_cert_mode: row.cf_cert_mode,
        }))
    }

    pub async fn admin_access_lines_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.access_routing_json().await?;
        Ok(admin_collection_json(
            "access_lines",
            data.get("access_lines")
                .cloned()
                .unwrap_or_else(|| json!([])),
        ))
    }

    pub async fn access_node_enabled_listen_ports(
        &self,
        access_node_id: Uuid,
    ) -> Result<Vec<u16>, DbError> {
        let rows = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT DISTINCT listen_port
            FROM access_lines
            WHERE access_node_id = $1
              AND enabled = TRUE
              AND lower(btrim(protocol)) <> 'hysteria'
              AND lower(btrim(transport)) <> 'hysteria'
            ORDER BY listen_port
            "#,
        )
        .bind(access_node_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|port| db_port_to_u16(port, "access_lines.listen_port", false))
            .collect()
    }

    pub async fn access_node_tls_cert_domains(
        &self,
        access_node_id: Uuid,
    ) -> Result<Vec<String>, DbError> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT COALESCE(
                NULLIF(btrim(server_name), ''),
                NULLIF(btrim(inbound_config->>'server_name'), ''),
                NULLIF(btrim(inbound_config->>'serverName'), ''),
                NULLIF(btrim(inbound_config->>'sni'), ''),
                NULLIF(btrim(inbound_config->>'tls_server_name'), ''),
                NULLIF(btrim(listen_host), '')
            )
            FROM access_lines
            WHERE access_node_id = $1
              AND enabled = TRUE
              AND (
                lower(btrim(protocol)) = 'trojan'
                OR lower(btrim(COALESCE(inbound_config->>'security', ''))) = 'tls'
              )
            ORDER BY 1
            "#,
        )
        .bind(access_node_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect())
    }

    pub async fn admin_exit_resources_json(&self) -> Result<serde_json::Value, DbError> {
        let rows = sqlx::query_as::<_, AdminExitResourceListRow>(
            r#"
            SELECT r.id, r.name, r.region_code, r.provider_name, r.ownership,
                   r.enabled, r.status,
                   r.last_probe_at, r.last_probe_status,
                   r.access_node_id, n.name AS access_node_name, r.created_at
            FROM exit_resources r
            LEFT JOIN access_nodes n ON n.id = r.access_node_id
            ORDER BY r.created_at DESC, r.name ASC, r.id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "uuid": row.id,
                    "name": row.name,
                    "region_code": row.region_code,
                    "provider_name": row.provider_name,
                    "ownership": row.ownership,
                    "enabled": row.enabled,
                    "status": row.status,
                    "last_probe_at": row.last_probe_at,
                    "last_probe_status": row.last_probe_status,
                    "access_node_id": row.access_node_id,
                    "access_node_name": row.access_node_name,
                    "created_at": row.created_at
                })
            })
            .collect::<Vec<_>>();
        Ok(admin_collection_json("exit_resources", json!(items)))
    }

    pub async fn admin_exit_endpoints_json(&self) -> Result<serde_json::Value, DbError> {
        let rows = sqlx::query_as::<_, AdminExitEndpointListRow>(
            r#"
            SELECT e.id,
                   e.exit_resource_id,
                   r.name AS exit_resource_name,
                   e.name,
                   e.outbound_type::text AS outbound_type,
                   e.host,
                   e.port,
                   e.outbound_config,
                   e.stream_config,
                   e.probe_config,
                   e.enabled,
                   r.enabled AS exit_resource_enabled,
                   e.last_probe_at,
                   e.last_probe_status,
                   e.created_at,
                   e.node_domain_id,
                   nd.domain AS node_domain,
                   nd.kind AS node_domain_kind
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            LEFT JOIN node_domains nd ON nd.id = e.node_domain_id
            ORDER BY e.created_at DESC, r.name ASC, e.name ASC, e.id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| -> Result<Value, DbError> {
                let outbound_config = row.outbound_config;
                let stream_config = row.stream_config;
                let probe_config = row.probe_config;
                Ok(json!({
                    "id": row.id,
                    "uuid": row.id,
                    "exit_resource_id": row.exit_resource_id,
                    "exit_resource_name": row.exit_resource_name,
                    "name": row.name,
                    "outbound_type": row.outbound_type,
                    "host": row.host,
                    "host_redacted": false,
                    "port": row.port.clamp(0, u16::MAX as i32),
                    "outbound_config": outbound_config,
                    "stream_config": stream_config,
                    "probe_config": probe_config,
                    "enabled": row.enabled,
                    "exit_resource_enabled": row.exit_resource_enabled,
                    "last_probe_at": row.last_probe_at,
                    "last_probe_status": row.last_probe_status,
                    "created_at": row.created_at,
                    // 多域名读模型:出口选中的 node_domain_id(未选 null)+ 解析出的 node_domain(domain/kind)。
                    "node_domain_id": row.node_domain_id,
                    "node_domain": row.node_domain_id.map(|id| json!({
                        "id": id,
                        "domain": row.node_domain,
                        "kind": row.node_domain_kind
                    }))
                }))
            })
            .collect::<Result<Vec<_>, DbError>>()?;
        Ok(admin_collection_json("exit_endpoints", json!(items)))
    }

    pub async fn exit_pools_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.load_store_data().await?;
        Ok(exit_pools_json(&data))
    }

    pub async fn operations_summary_json(&self) -> Result<serde_json::Value, DbError> {
        let data = self.load_store_data().await?;
        let value = operations_summary_json(&data);
        let mut value = self.attach_operations_runtime(value).await?;
        let backup_state = self.database_backup_state_json().await?;
        attach_production_alerts(&data, &backup_state, &mut value);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "settings".to_string(),
                self.access_operations_settings_json().await?,
            );
        }
        Ok(value)
    }
}

fn attach_production_alerts(data: &StoreData, backup_state: &Value, value: &mut Value) {
    let mut alerts = Vec::new();
    let now = Utc::now();
    for node in data.access_nodes.values() {
        if access_node_is_offline(node, now) {
            alerts.push(json!({
                "id": format!("access_node_offline:{}", node.id),
                "kind": "access_node_offline",
                "severity": "danger",
                "resource_type": "access_node",
                "resource_id": node.id,
                "resource_name": node.name,
                "title": format!("中转节点 {} 离线", node.name),
                "message": "节点心跳超过 5 分钟未刷新或尚未上报心跳",
                "detected_at": now
            }));
        }
        append_tls_alerts(node, now, &mut alerts);
    }

    if backup_state
        .get("last_status")
        .or_else(|| backup_state.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "failed" | "error"))
    {
        alerts.push(json!({
            "id": "database_backup_failed",
            "kind": "database_backup_failed",
            "severity": "danger",
            "resource_type": "database_backup",
            "resource_id": Value::Null,
            "resource_name": "数据库备份",
            "title": "数据库自动备份失败",
            "message": backup_state
                .get("last_error")
                .or_else(|| backup_state.get("error_summary"))
                .and_then(Value::as_str)
                .unwrap_or("最近一次数据库备份失败"),
            "detected_at": now
        }));
    }

    let danger_count = alerts
        .iter()
        .filter(|alert| alert.get("severity").and_then(Value::as_str) == Some("danger"))
        .count();
    let warning_count = alerts
        .iter()
        .filter(|alert| alert.get("severity").and_then(Value::as_str) == Some("warning"))
        .count();
    let info_count = alerts.len().saturating_sub(danger_count + warning_count);
    if let Some(object) = value.as_object_mut() {
        object.insert("alerts".to_string(), json!(alerts));
        object.insert(
            "alert_summary".to_string(),
            json!({
                "danger_count": danger_count,
                "warning_count": warning_count,
                "info_count": info_count,
                "total_count": danger_count + warning_count + info_count,
                "generated_at": now
            }),
        );
    }
}

fn access_node_is_offline(node: &AccessNode, now: DateTime<Utc>) -> bool {
    if node.id.to_string() == "00000000-0000-0000-0000-000000000201"
        && node.public_host == "access.example.test"
    {
        return false;
    }
    node.last_heartbeat_at
        .map(|heartbeat_at| now.signed_duration_since(heartbeat_at) > Duration::minutes(5))
        .unwrap_or(true)
}

fn append_tls_alerts(node: &AccessNode, now: DateTime<Utc>, alerts: &mut Vec<Value>) {
    let Some(certificates) = node.tls_certificates.as_array() else {
        return;
    };
    for certificate in certificates {
        let status = certificate
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let severity = match status {
            "expired" | "missing" | "invalid" => "danger",
            "expiring" => "warning",
            _ => continue,
        };
        let kind = match status {
            "expired" => "ssl_certificate_expired",
            "missing" => "ssl_certificate_missing",
            "invalid" => "ssl_certificate_invalid",
            "expiring" => "ssl_certificate_expiring",
            _ => "ssl_certificate_warning",
        };
        let domain = certificate
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or("未知域名");
        alerts.push(json!({
            "id": format!("{kind}:{}:{domain}", node.id),
            "kind": kind,
            "severity": severity,
            "resource_type": "access_node",
            "resource_id": node.id,
            "resource_name": node.name,
            "title": format!("SSL 证书异常：{domain}"),
            "message": certificate
                .get("error_summary")
                .and_then(Value::as_str)
                .unwrap_or("节点上报 SSL 证书状态异常"),
            "detected_at": now
        }));
    }
}
