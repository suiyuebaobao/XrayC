//! 内存 StoreData 装载逻辑。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用；不引入 API、前端或脚本层行为改动。
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
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn load_store_data(&self) -> Result<StoreData, DbError> {
        let mut data = StoreData::default();

        for row in sqlx::query_as::<_, AccessNodeRow>(
            r#"
            SELECT id, name, public_host, public_port, ssh_host, remark, agent_token_hash, status, agent_version,
                   config_dirty, desired_config_hash, applied_config_hash,
                   last_heartbeat_at, last_traffic_report_at, last_traffic_success_at,
                   config_dirty_at, config_dirty_reason,
                   tls_certificates, tls_cert_last_report_at,
                   tls_renew_request_id, tls_renew_requested_at, tls_renew_completed_at,
                   tls_renew_status, tls_renew_message,
                   cert_domain, acme_email, cf_enabled, cf_domain, cf_cert_mode,
                   ip_direct_address,
                   kernel_connmark_available, kernel_upgrade_pending,
                   reboot_status, reboot_message
            FROM access_nodes
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.access_nodes.insert(
                row.id,
                AccessNode {
                    id: row.id,
                    name: row.name,
                    public_host: row.public_host,
                    public_port: db_port_to_u16(row.public_port, "access_nodes.public_port", false)?,
                    ssh_host: row.ssh_host,
                    remark: row.remark,
                    agent_token_hash: row.agent_token_hash,
                    status: row.status,
                    agent_version: row.agent_version,
                    config_dirty: row.config_dirty,
                    desired_config_hash: row.desired_config_hash,
                    applied_config_hash: row.applied_config_hash,
                    last_heartbeat_at: row.last_heartbeat_at,
                    last_traffic_report_at: row.last_traffic_report_at,
                    last_traffic_success_at: row.last_traffic_success_at,
                    config_dirty_at: row.config_dirty_at,
                    config_dirty_reason: row.config_dirty_reason,
                    tls_certificates: row.tls_certificates,
                    tls_cert_last_report_at: row.tls_cert_last_report_at,
                    tls_renew_request_id: row.tls_renew_request_id,
                    tls_renew_requested_at: row.tls_renew_requested_at,
                    tls_renew_completed_at: row.tls_renew_completed_at,
                    tls_renew_status: row.tls_renew_status,
                    tls_renew_message: row.tls_renew_message,
                    // 多模式 CF 字段:nullable 列归一为空串,布尔/模式列直接取。
                    cert_domain: row.cert_domain.unwrap_or_default(),
                    acme_email: row.acme_email.unwrap_or_default(),
                    cf_enabled: row.cf_enabled,
                    cf_domain: row.cf_domain.unwrap_or_default(),
                    cf_cert_mode: row.cf_cert_mode,
                    ip_direct_address: row.ip_direct_address.unwrap_or_default(),
                    // 内核能力软状态(§7.7.1):NOT NULL 默认列直接取,供面板显示降级/待重启。
                    kernel_connmark_available: row.kernel_connmark_available,
                    kernel_upgrade_pending: row.kernel_upgrade_pending,
                    reboot_status: row.reboot_status,
                    reboot_message: row.reboot_message,
                    // 多域名读模型:先建空数组,随后一次性查 node_domains 回填各节点。
                    domains: Vec::new(),
                },
            );
        }

        // 多域名 Phase 5b:一次性读全部 node_domains,按节点回填 domains[](保证读侧必含,空则空数组)。
        self.populate_node_domains_into(&mut data).await?;

        for row in sqlx::query_as::<_, ExitPoolRow>(
            r#"
            SELECT id, name, region_code, strategy, enabled
            FROM exit_pools
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.exit_pools.insert(
                row.id,
                ExitPool {
                    id: row.id,
                    name: row.name,
                    region_code: row.region_code,
                    strategy: row.strategy,
                    enabled: row.enabled,
                    members: Vec::new(),
                },
            );
        }

        for row in sqlx::query_as::<_, ExitEndpointRow>(
            r#"
            SELECT p.id AS exit_pool_id, e.id, r.name AS resource_name,
                   r.ownership, r.access_node_id AS owner_access_node_id,
                   e.outbound_type::text AS outbound_type, e.host, e.port,
                   e.outbound_config,
                   e.stream_config,
                   m.weight, m.priority, m.allow_new_assignments, m.status,
                   (
                       p.enabled
                       AND m.status IN ('healthy', 'degraded', 'draining')
                       AND e.enabled
                       AND r.enabled
                       AND (r.ownership = 'self_hosted' OR lower(COALESCE(NULLIF(trim(r.status), ''), 'healthy')) <> 'offline')
                   ) AS healthy
            FROM exit_pool_members m
            JOIN exit_pools p ON p.id = m.exit_pool_id
            JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
            JOIN exit_resources r ON r.id = e.exit_resource_id
            ORDER BY p.name, r.name, e.host
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            let outbound_type = endpoint_type(&row.outbound_type);
            if let Some(pool) = data.exit_pools.get_mut(&row.exit_pool_id) {
                pool.members.push(ExitEndpoint {
                    id: row.id,
                    resource_name: row.resource_name,
                    ownership: row.ownership,
                    owner_access_node_id: row.owner_access_node_id,
                    outbound_type,
                    host: row.host,
                    port: db_port_to_u16(
                        row.port,
                        "exit_endpoints.port",
                        matches!(outbound_type, EndpointType::Direct),
                    )?,
                    outbound_config: row.outbound_config,
                    stream_config: row.stream_config,
                    weight: row.weight.max(0) as u32,
                    priority: row.priority.max(0) as u32,
                    allow_new_assignments: row.allow_new_assignments,
                    healthy: row.healthy,
                    status: row.status,
                });
            }
        }

        for row in sqlx::query_as::<_, LocalExitEndpointRow>(
            r#"
            SELECT e.id, r.name AS resource_name,
                   r.ownership, r.access_node_id AS owner_access_node_id,
                   e.outbound_type::text AS outbound_type, e.host, e.port,
                   e.outbound_config, e.stream_config,
                   COALESCE(NULLIF(e.last_probe_status, ''), r.status, 'unknown') AS status,
                   (e.enabled AND r.enabled AND r.access_node_id IS NOT NULL) AS healthy
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE r.ownership = 'self_hosted'
              AND r.access_node_id IS NOT NULL
            ORDER BY r.name, e.host, e.port
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            let outbound_type = endpoint_type(&row.outbound_type);
            data.local_exit_endpoints.push(ExitEndpoint {
                id: row.id,
                resource_name: row.resource_name,
                ownership: row.ownership,
                owner_access_node_id: row.owner_access_node_id,
                outbound_type,
                host: row.host,
                port: db_port_to_u16(
                    row.port,
                    "exit_endpoints.port",
                    matches!(outbound_type, EndpointType::Direct),
                )?,
                outbound_config: row.outbound_config,
                stream_config: row.stream_config,
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: row.healthy,
                status: row.status,
            });
        }

        for row in sqlx::query_as::<_, AccessLineRow>(
            r#"
            SELECT id, name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port,
                   protocol, transport, user_uuid, server_name, public_key, short_id,
                   flow, inbound_config, udp_enabled, udp_packet_encoding,
                   xhttp_path, xhttp_host, xhttp_mode,
                   region_code, region_name, region_flag, enabled
            FROM access_lines
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.access_lines.insert(
                row.id,
                AccessLine {
                    id: row.id,
                    name: row.name,
                    access_node_id: row.access_node_id,
                    line_group_id: row.line_group_id,
                    exit_endpoint_id: row.exit_endpoint_id,
                    exit_pool_id: row.exit_pool_id,
                    listen_host: row.listen_host,
                    listen_port: db_port_to_u16(
                        row.listen_port,
                        "access_lines.listen_port",
                        false,
                    )?,
                    protocol: row.protocol,
                    transport: row.transport,
                    uuid: row.user_uuid,
                    server_name: row.server_name,
                    public_key: row.public_key,
                    short_id: row.short_id,
                    flow: row.flow,
                    inbound_config: row.inbound_config,
                    udp_enabled: row.udp_enabled,
                    udp_packet_encoding: row.udp_packet_encoding,
                    xhttp_path: row.xhttp_path,
                    xhttp_host: row.xhttp_host,
                    xhttp_mode: row.xhttp_mode,
                    region_code: row.region_code,
                    region_name: row.region_name,
                    region_flag: row.region_flag,
                    enabled: row.enabled,
                },
            );
        }

        for row in sqlx::query_as::<_, LineGroupRow>(
            r#"
            SELECT id, name, country_code, icon, exit_pool_id,
                   sort_weight, billing_multiplier::float8 AS billing_multiplier, enabled,
                   dedicated_rules
            FROM line_groups
            ORDER BY sort_weight, name, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.line_groups.insert(
                row.id,
                LineGroup {
                    id: row.id,
                    name: row.name,
                    country_code: row.country_code,
                    icon: row.icon,
                    sort_weight: row.sort_weight,
                    billing_multiplier: row.billing_multiplier,
                    enabled: row.enabled,
                    exit_pool_id: row.exit_pool_id,
                    line_ids: Vec::new(),
                    binding_node_ids: Vec::new(),
                    dedicated_rules: string_array_from_json(row.dedicated_rules),
                    rule_set_bindings: Vec::new(),
                },
            );
        }

        for row in sqlx::query_as::<_, LineGroupRuleSetBindingRow>(
            r#"
            SELECT b.line_group_id,
                   b.rule_set_id,
                   rs.name AS rule_set_name,
                   b.enabled AS binding_enabled,
                   rs.enabled AS rule_set_enabled,
                   b.position,
                   rs.rules
            FROM line_group_rule_set_bindings b
            JOIN subscription_rule_sets rs ON rs.id = b.rule_set_id
            ORDER BY b.line_group_id, b.position, rs.name, rs.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            if let Some(group) = data.line_groups.get_mut(&row.line_group_id) {
                group.rule_set_bindings.push(LineGroupRuleSetBinding {
                    rule_set_id: row.rule_set_id,
                    rule_set_name: row.rule_set_name,
                    enabled: row.binding_enabled && row.rule_set_enabled,
                    position: row.position,
                    rules: string_array_from_json(row.rules),
                });
            }
        }

        for row in sqlx::query_as::<_, LineGroupLineRow>(
            r#"
            SELECT gl.line_group_id, gl.exit_endpoint_id
            FROM line_group_exit_endpoints gl
            JOIN exit_endpoints e ON e.id = gl.exit_endpoint_id
            JOIN exit_resources r ON r.id = e.exit_resource_id
            ORDER BY gl.line_group_id, r.name, e.name, e.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            if let Some(group) = data.line_groups.get_mut(&row.line_group_id) {
                group.line_ids.push(row.exit_endpoint_id);
            }
        }

        // 分组绑定节点回填:抽到 populate_line_group_binding_nodes_into,
        // 在读模型源头用 EXISTS 过滤掉缺运行态 access_line 的悬挂 id(防订阅整单 422)。
        self.populate_line_group_binding_nodes_into(&mut data)
            .await?;

        let mut plan_groups: HashMap<Uuid, Vec<PlanLineGroup>> = HashMap::new();
        for row in sqlx::query_as::<_, PlanLineGroupRow>(
            r#"
            SELECT plg.plan_id, plg.line_group_id,
                   plg.billing_multiplier::float8 AS billing_multiplier
            FROM plan_line_groups plg
            JOIN line_groups lg ON lg.id = plg.line_group_id
            ORDER BY plg.plan_id, lg.sort_weight, lg.name, plg.line_group_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            plan_groups
                .entry(row.plan_id)
                .or_default()
                .push(PlanLineGroup {
                    line_group_id: row.line_group_id,
                    billing_multiplier: row.billing_multiplier,
                });
        }

        for row in sqlx::query_as::<_, PlanRow>(
            r#"
            SELECT id, name, is_default, traffic_limit_bytes, rate_limit_bps,
                   rate_limit_up_bps, rate_limit_down_bps,
                   billing_multiplier::float8 AS billing_multiplier,
                   price_cents, currency, duration_days, sort_weight,
                   default_line_group_id
            FROM plans
            WHERE enabled = TRUE AND is_deleted = FALSE
            ORDER BY is_default DESC, sort_weight, name
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            let line_groups = plan_groups.remove(&row.id).unwrap_or_default();
            let line_group_ids = line_groups
                .iter()
                .map(|binding| binding.line_group_id)
                .collect();
            data.plans.insert(
                row.id,
                Plan {
                    id: row.id,
                    name: row.name,
                    is_default: row.is_default,
                    traffic_limit_bytes: to_u64(row.traffic_limit_bytes),
                    rate_limit_bps: to_u64(row.rate_limit_bps),
                    rate_limit_up_bps: row.rate_limit_up_bps.map(to_u64),
                    rate_limit_down_bps: row.rate_limit_down_bps.map(to_u64),
                    billing_multiplier: row.billing_multiplier,
                    price_cents: row.price_cents,
                    currency: row.currency,
                    duration_days: row.duration_days,
                    sort_weight: row.sort_weight,
                    line_group_ids,
                    line_groups,
                    default_line_group_id: row.default_line_group_id,
                },
            );
        }

        for row in sqlx::query_as::<_, UserRow>(
            r#"SELECT id, email, xray_user_key, access_credential, disabled, rate_limit_bps, rate_limit_up_bps, rate_limit_down_bps FROM users"#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.users.insert(
                row.id,
                User {
                    id: row.id,
                    email: row.email,
                    xray_user_key: row.xray_user_key,
                    access_credential: row.access_credential,
                    disabled: row.disabled,
                    rate_limit_bps: row.rate_limit_bps.map(to_u64),
                    rate_limit_up_bps: row.rate_limit_up_bps.map(to_u64),
                    rate_limit_down_bps: row.rate_limit_down_bps.map(to_u64),
                },
            );
        }

        for row in sqlx::query_as::<_, SubscriptionRow>(
            r#"
            SELECT user_id, plan_id, active, expires_at, used_bytes,
                   limit_bytes
            FROM user_subscriptions
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.subscriptions.insert(
                row.user_id,
                UserSubscription {
                    user_id: row.user_id,
                    plan_id: row.plan_id,
                    active: row.active,
                    expires_at: row.expires_at,
                    used_bytes: to_u64(row.used_bytes),
                    limit_bytes: to_u64(row.limit_bytes),
                },
            );
        }

        for row in sqlx::query_as::<_, TokenRow>(
            r#"
                SELECT token, user_id
                FROM subscription_tokens
                WHERE revoked_at IS NULL
                  AND expires_at > now()
                "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.tokens.insert(
                row.token.clone(),
                SubscriptionToken {
                    token: row.token,
                    user_id: row.user_id,
                },
            );
        }

        for row in sqlx::query_as::<_, UserExitAssignmentRow>(
            r#"
            SELECT user_id, access_line_id, exit_pool_id, exit_endpoint_id,
                   assigned_at, failover_reason
            FROM user_exit_assignments
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.user_exit_assignments.push(UserExitAssignment {
                user_id: row.user_id,
                access_line_id: row.access_line_id,
                exit_pool_id: row.exit_pool_id,
                exit_endpoint_id: row.exit_endpoint_id,
                assigned_at: row.assigned_at,
                failover_reason: row.failover_reason,
            });
        }

        for row in sqlx::query_as::<_, AccessExitProbeStateRow>(
            r#"
            SELECT access_node_id, exit_endpoint_id, effective_status
            FROM access_exit_probe_states
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            data.access_exit_probe_states.insert(
                (row.access_node_id, row.exit_endpoint_id),
                AccessExitProbeState {
                    access_node_id: row.access_node_id,
                    exit_endpoint_id: row.exit_endpoint_id,
                    effective_status: row.effective_status,
                },
            );
        }

        for row in sqlx::query_as::<_, SnapshotRow>(
            r#"
            SELECT access_line_id, xray_user_key, uplink_total, downlink_total, collected_at
            FROM access_traffic_snapshots
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            let snapshot = TrafficSnapshot {
                access_line_id: row.access_line_id,
                xray_user_key: row.xray_user_key,
                uplink_total: to_u64(row.uplink_total),
                downlink_total: to_u64(row.downlink_total),
                collected_at: row.collected_at,
            };
            data.snapshots.insert(
                (snapshot.access_line_id, snapshot.xray_user_key.clone()),
                snapshot,
            );
        }

        Ok(data)
    }
}

fn string_array_from_json(value: Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
