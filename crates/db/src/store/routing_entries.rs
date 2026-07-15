//! 中转节点入口行和线路绑定写接口。
//! 本文件承载“入口协议/网络/端口 + 出口线路”的原子创建。
//! 入口只在这里生成，线路分组本身只保存线路池成员。
//! 每个输入行会生成一条 access_lines 记录。
//! 出口池仅作为内部运行集合，由后端按具体出口线路自动创建或复用。
//! 创建后标记所属中转节点 dirty，由 access-agent 心跳应用新配置。
//! 失败时事务回滚，避免只有入口没有运行集合。
//! 本模块不处理 SSH 部署，不访问服务器 Shell。
//! 后续扩展入口协议时应同步 validation 和前端选项。
//! 本头部满足前十行中文注释约束。

use super::dirty::*;
use super::local_access_defaults::local_access_line_inbound_config;
use super::routing_entry_reality::*;
use crate::*;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) struct EntryNetworkMode {
    pub(crate) transport: &'static str,
    pub(crate) udp_enabled: bool,
    pub(crate) udp_packet_encoding: &'static str,
}

pub(crate) struct SelectedExitEndpoint {
    pub(crate) id: Uuid,
    pub(crate) label: String,
    pub(crate) region_code: String,
}

impl PgStore {
    pub async fn create_admin_access_node_group_entries(
        &self,
        access_node_id: Uuid,
        entries: Vec<AdminAccessNodeGroupEntryInput>,
    ) -> Result<AdminAccessNodeGroupEntryResult, DbError> {
        if entries.is_empty() {
            return Err(DbError::InvalidAgentPayload(
                "请至少添加一个中转入口".to_string(),
            ));
        }
        if entries.len() > 50 {
            return Err(DbError::InvalidAgentPayload(
                "单次最多创建 50 个中转入口".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        let public_host = sqlx::query_scalar::<_, String>(
            "SELECT public_host FROM access_nodes WHERE id = $1 FOR UPDATE",
        )
        .bind(access_node_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("中转节点不存在: {access_node_id}")))?;

        let mut created_line_ids = Vec::with_capacity(entries.len());
        let mut exit_endpoint_ids = Vec::new();
        let mut seen_ports = HashSet::new();

        for entry in entries {
            if entry.listen_port == 0 {
                return Err(DbError::InvalidAgentPayload(
                    "入口监听端口必须大于 0".to_string(),
                ));
            }
            if !seen_ports.insert(entry.listen_port) {
                return Err(DbError::InvalidAgentPayload(format!(
                    "本次提交里入口端口重复: {}",
                    entry.listen_port
                )));
            }

            let exit_endpoint =
                selected_exit_endpoint_in_tx(&mut tx, access_node_id, entry.exit_endpoint_id)
                    .await?;
            let exit_pool_id = sync_single_endpoint_exit_pool_in_tx(
                &mut tx,
                exit_endpoint.id,
                &exit_endpoint.label,
                &exit_endpoint.region_code,
            )
            .await?;

            let protocol = validate_access_protocol(&entry.protocol)?;
            let mode = access_entry_network_mode(protocol, &entry.network_mode)?;
            validate_access_protocol_transport(protocol, mode.transport)?;
            let xhttp_mode = if mode.transport == "xhttp" {
                validate_xhttp_mode(&entry.xhttp_mode)?
            } else {
                "auto"
            };
            let listen_host = optional_admin_text(&entry.listen_host, 255);
            let listen_host = if listen_host.is_empty() {
                public_host.clone()
            } else {
                listen_host
            };
            let server_name = server_name_for_entry(protocol, &listen_host, &entry.inbound_config);
            let mut inbound_config = if entry
                .inbound_config
                .as_object()
                .is_some_and(|object| object.is_empty())
            {
                local_access_line_inbound_config(protocol, &server_name)?
            } else {
                entry.inbound_config
            };
            inbound_config =
                normalize_access_inbound_config(protocol, inbound_config, &server_name)?;
            inbound_config =
                fill_vless_reality_entry_config(protocol, &server_name, inbound_config)?;
            inbound_config = fill_vless_quantum_entry_config(protocol, inbound_config)?;
            validate_vless_reality_entry_config(protocol, &inbound_config)?;
            validate_vless_reality_entry_network_mode(protocol, &inbound_config, &mode)?;
            validate_access_inbound_config(protocol, &inbound_config, &server_name)?;
            let (public_key, short_id) =
                vless_reality_entry_public_fields(protocol, &inbound_config)?;
            let flow = config_text(&inbound_config, &["flow"]).unwrap_or_default();
            let line_name = if entry.name.trim().is_empty() {
                default_entry_line_name(
                    &public_host,
                    &exit_endpoint.label,
                    protocol,
                    &entry.network_mode,
                    entry.listen_port,
                )
            } else {
                required_admin_text(&entry.name, "中转入口名称", 128)?
            };
            validate_hong_kong_hy2(protocol, &exit_endpoint.region_code)?;

            let access_line_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO access_lines (
                    name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port,
                    protocol, transport, user_uuid, server_name, public_key, short_id,
                    enabled, region_code, region_name, region_flag,
                    flow, udp_enabled, udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode,
                    identity_mode, user_key_source, inbound_config, visibility_weight
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                    $14, $15, $16, '',
                    $17, $18, $19, '', '', $20,
                    'credential', 'xray_email', $21, 100
                )
                RETURNING id
                "#,
            )
            .bind(line_name)
            .bind(access_node_id)
            .bind(None::<Uuid>)
            .bind(exit_endpoint.id)
            .bind(exit_pool_id)
            .bind(listen_host)
            .bind(i32::from(entry.listen_port))
            .bind(protocol)
            .bind(mode.transport)
            .bind(Uuid::new_v4().to_string())
            .bind(&server_name)
            .bind(public_key)
            .bind(short_id)
            .bind(entry.enabled)
            .bind(&exit_endpoint.region_code)
            .bind(&exit_endpoint.label)
            .bind(flow)
            .bind(mode.udp_enabled)
            .bind(mode.udp_packet_encoding)
            .bind(xhttp_mode)
            .bind(inbound_config)
            .fetch_one(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO access_entries (
                    id, access_node_id, name, listen_host, listen_port, protocol, transport,
                    security, user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                    udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode, inbound_config,
                    enabled, sort_weight
                )
                SELECT
                    id, access_node_id, name, listen_host, listen_port, protocol, transport,
                    COALESCE(NULLIF(inbound_config->>'security', ''), ''),
                    user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                    udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode, inbound_config,
                    enabled, visibility_weight
                FROM access_lines
                WHERE id = $1
                ON CONFLICT (id) DO UPDATE SET
                    access_node_id = EXCLUDED.access_node_id,
                    name = EXCLUDED.name,
                    listen_host = EXCLUDED.listen_host,
                    listen_port = EXCLUDED.listen_port,
                    protocol = EXCLUDED.protocol,
                    transport = EXCLUDED.transport,
                    security = EXCLUDED.security,
                    user_uuid = EXCLUDED.user_uuid,
                    server_name = EXCLUDED.server_name,
                    public_key = EXCLUDED.public_key,
                    short_id = EXCLUDED.short_id,
                    flow = EXCLUDED.flow,
                    udp_enabled = EXCLUDED.udp_enabled,
                    udp_packet_encoding = EXCLUDED.udp_packet_encoding,
                    xhttp_path = EXCLUDED.xhttp_path,
                    xhttp_host = EXCLUDED.xhttp_host,
                    xhttp_mode = EXCLUDED.xhttp_mode,
                    inbound_config = EXCLUDED.inbound_config,
                    enabled = EXCLUDED.enabled,
                    sort_weight = EXCLUDED.sort_weight,
                    updated_at = now()
                "#,
            )
            .bind(access_line_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO access_entry_exit_bindings (
                    id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight
                )
                SELECT id, id, exit_endpoint_id, exit_pool_id, name, enabled, visibility_weight
                FROM access_lines
                WHERE id = $1 AND exit_endpoint_id IS NOT NULL
                ON CONFLICT (id) DO UPDATE SET
                    access_entry_id = EXCLUDED.access_entry_id,
                    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                    exit_pool_id = EXCLUDED.exit_pool_id,
                    name = EXCLUDED.name,
                    enabled = EXCLUDED.enabled,
                    sort_weight = EXCLUDED.sort_weight,
                    updated_at = now()
                "#,
            )
            .bind(access_line_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO line_group_binding_nodes (
                    line_group_id, entry_exit_binding_id, position
                )
                SELECT line_group_id, $1, 100
                FROM line_group_exit_endpoints
                WHERE exit_endpoint_id = $2
                ON CONFLICT (line_group_id, entry_exit_binding_id) DO NOTHING
                "#,
            )
            .bind(access_line_id)
            .bind(exit_endpoint.id)
            .execute(&mut *tx)
            .await?;

            created_line_ids.push(access_line_id);
            if !exit_endpoint_ids.contains(&exit_endpoint.id) {
                exit_endpoint_ids.push(exit_endpoint.id);
            }
        }

        mark_access_node_dirty_in_tx(&mut tx, access_node_id, "admin_bound_access_node_lines")
            .await?;
        tx.commit().await?;

        Ok(AdminAccessNodeGroupEntryResult {
            created_line_ids,
            exit_endpoint_ids,
        })
    }
}

pub(crate) async fn selected_exit_endpoint_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    exit_endpoint_id: Uuid,
) -> Result<SelectedExitEndpoint, DbError> {
    let row = sqlx::query_as::<_, (Uuid, String, String, bool, bool, String, Option<Uuid>)>(
        r#"
        SELECT e.id,
               COALESCE(NULLIF(e.name, ''), NULLIF(r.name, ''), e.id::text) AS label,
               COALESCE(NULLIF(r.region_code, ''), 'GLOBAL') AS region_code,
               e.enabled,
               r.enabled,
               r.ownership,
               r.access_node_id
        FROM exit_endpoints e
        JOIN exit_resources r ON r.id = e.exit_resource_id
        WHERE e.id = $1
        "#,
    )
    .bind(exit_endpoint_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::InvalidAgentPayload(format!("线路不存在: {exit_endpoint_id}")))?;

    if !row.3 || !row.4 {
        return Err(DbError::InvalidAgentPayload(
            "请选择已启用的出口线路".to_string(),
        ));
    }
    if row.5 == "local_direct" && row.6 != Some(access_node_id) {
        return Err(DbError::InvalidAgentPayload(
            "本机 direct 出口只能绑定所属中转节点".to_string(),
        ));
    }
    Ok(SelectedExitEndpoint {
        id: row.0,
        label: row.1,
        region_code: row.2,
    })
}

pub(crate) async fn sync_single_endpoint_exit_pool_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
    label: &str,
    region_code: &str,
) -> Result<Uuid, DbError> {
    let pool_name = required_admin_text(
        &format!("线路入口出口：{} {}", label, exit_endpoint_id.simple()),
        "线路入口出口池名称",
        128,
    )?;
    let exit_pool_id = match sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT p.id
        FROM exit_pools p
        WHERE p.name = $1
          AND NOT EXISTS (
              SELECT 1
              FROM exit_pool_members other
              WHERE other.exit_pool_id = p.id
                AND other.exit_endpoint_id <> $2
          )
        ORDER BY p.id
        LIMIT 1
        "#,
    )
    .bind(&pool_name)
    .bind(exit_endpoint_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        Some(id) => id,
        None => {
            sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO exit_pools (name, region_code, strategy, enabled, updated_at)
                VALUES ($1, $2, 'priority', TRUE, now())
                RETURNING id
                "#,
            )
            .bind(&pool_name)
            .bind(region_code)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    sqlx::query(
        r#"
        DELETE FROM exit_pool_members
        WHERE exit_pool_id = $1 AND exit_endpoint_id <> $2
        "#,
    )
    .bind(exit_pool_id)
    .bind(exit_endpoint_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO exit_pool_members (
            exit_pool_id, exit_endpoint_id, weight, status,
            priority, allow_new_assignments, updated_at
        )
        VALUES ($1, $2, 100, 'healthy', 100, TRUE, now())
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
    .execute(&mut **tx)
    .await?;

    Ok(exit_pool_id)
}

pub(crate) fn access_entry_network_mode(
    protocol: &str,
    raw_mode: &str,
) -> Result<EntryNetworkMode, DbError> {
    match raw_mode.trim().to_ascii_lowercase().as_str() {
        "" | "tcp" => Ok(EntryNetworkMode {
            transport: "tcp",
            udp_enabled: false,
            udp_packet_encoding: "",
        }),
        "udp" => Ok(EntryNetworkMode {
            transport: if protocol == "hysteria" {
                "hysteria"
            } else {
                "tcp"
            },
            udp_enabled: true,
            udp_packet_encoding: "",
        }),
        "xhttp" => Ok(EntryNetworkMode {
            transport: "xhttp",
            udp_enabled: false,
            udp_packet_encoding: "",
        }),
        "ws" | "websocket" => Ok(EntryNetworkMode {
            transport: "ws",
            udp_enabled: false,
            udp_packet_encoding: "",
        }),
        "grpc" => Ok(EntryNetworkMode {
            transport: "grpc",
            udp_enabled: false,
            udp_packet_encoding: "",
        }),
        "xudp" if protocol == "vless" => Ok(EntryNetworkMode {
            transport: "tcp",
            udp_enabled: true,
            udp_packet_encoding: "xudp",
        }),
        "xudp" => Err(DbError::InvalidAgentPayload(
            "XUDP 入口模式当前仅支持 VLESS".to_string(),
        )),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的客户端网络模式: {other}"
        ))),
    }
}

fn default_entry_line_name(
    public_host: &str,
    line_label: &str,
    protocol: &str,
    network_mode: &str,
    listen_port: u16,
) -> String {
    let mode = if network_mode.trim().is_empty() {
        "tcp"
    } else {
        network_mode.trim()
    };
    optional_admin_text(
        &format!("{public_host}-{line_label}-{protocol}-{mode}-{listen_port}"),
        128,
    )
}
