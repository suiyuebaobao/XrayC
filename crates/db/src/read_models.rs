//! 本模块集中承载 StoreData 读模型 JSON 组装。
//! 这些函数只读取内存快照，不访问数据库或修改状态。
//! 管理端路由、出口池和运营摘要统一在这里生成。
//! 订阅与心跳读模型拆到相邻模块，避免单文件过长。
//! Xray 协议可用性判断委托给渲染协议 helper。
//! 字节展示复用 helpers 中的稳定换算函数。
//! 函数签名保持 crate root 重导出后的历史兼容。
//! 新增 StoreData 聚合视图应优先放在本模块。
//! 本模块不得引入 SQL、PgStore 或事务流程。
//! 文件行数需保持低于 500 行，便于后续继续拆分。

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;
use xrayc_core::StoreData;

use crate::helpers::bytes_to_gb;
use crate::xray_protocol::xray_exit_protocol_for_endpoint;

fn access_node_health_status(
    last_heartbeat_at: Option<DateTime<Utc>>,
    config_dirty: bool,
    config_synced: bool,
) -> (&'static str, &'static str) {
    let Some(last_heartbeat_at) = last_heartbeat_at else {
        return ("offline", "节点尚未上报心跳");
    };
    let heartbeat_age = Utc::now().signed_duration_since(last_heartbeat_at);
    if heartbeat_age > Duration::minutes(5) {
        return ("offline", "心跳超过 5 分钟未刷新");
    }
    if heartbeat_age > Duration::seconds(90) {
        return ("stale", "心跳已超过 90 秒");
    }
    if config_dirty || !config_synced {
        return ("syncing", "配置等待节点应用");
    }
    ("healthy", "心跳和配置同步正常")
}

fn access_line_protocol_label(line: &xrayc_core::AccessLine) -> String {
    let base = format!("{}+{}", line.protocol, line.transport);
    if line.udp_enabled && line.udp_packet_encoding.eq_ignore_ascii_case("xudp") {
        format!("{base}+xudp")
    } else {
        base
    }
}

pub fn access_routing_json(data: &StoreData) -> serde_json::Value {
    let access_lines = data
        .access_lines
        .values()
        .enumerate()
        .map(|(index, line)| {
            let access_node_name = data
                .access_nodes
                .get(&line.access_node_id)
                .map(|node| node.name.clone())
                .unwrap_or_default();
            let exit_pool_name = data
                .exit_pools
                .get(&line.exit_pool_id)
                .map(|pool| pool.name.clone())
                .unwrap_or_default();
            let line_group_name = line
                .line_group_id
                .and_then(|line_group_id| data.line_groups.get(&line_group_id))
                .map(|group| group.name.clone())
                .unwrap_or_default();
            let exit_endpoint = line.exit_endpoint_id.and_then(|exit_endpoint_id| {
                data.exit_pools
                    .values()
                    .flat_map(|pool| pool.members.iter())
                    .find(|member| member.id == exit_endpoint_id)
            });

            json!({
                "id": index + 1,
                "uuid": line.id,
                "name": line.name,
                "region_code": line.region_code,
                "region_name": line.region_name,
                "region_flag": line.region_flag,
                "access_node_id": line.access_node_id,
                "access_node_name": access_node_name,
                "line_group_id": line.line_group_id,
                "line_group_name": line_group_name,
                "exit_endpoint_id": line.exit_endpoint_id,
                "exit_endpoint_name": exit_endpoint
                    .map(|endpoint| endpoint.resource_name.clone())
                    .unwrap_or_default(),
                "listen_host": line.listen_host,
                "listen_port": line.listen_port,
                "protocol": access_line_protocol_label(line),
                "udp_enabled": line.udp_enabled,
                "udp_packet_encoding": line.udp_packet_encoding,
                "status": if line.enabled { "enabled" } else { "disabled" },
                "exit_pool_id": line.exit_pool_id,
                "exit_pool_name": exit_pool_name,
                "online_users": Value::Null,
                "active_connections": Value::Null,
                "unique_client_ips": Value::Null,
                "uplink_rate_bps": Value::Null,
                "downlink_rate_bps": Value::Null,
                "metric_status": "no_data",
                "latency_ms": Value::Null,
                "probe_status": "unknown"
            })
        })
        .collect::<Vec<_>>();

    let access_nodes = data
        .access_nodes
        .values()
        .map(|node| {
            let heartbeat_age_seconds = node.last_heartbeat_at.map(|heartbeat_at| {
                Utc::now()
                    .signed_duration_since(heartbeat_at)
                    .num_seconds()
                    .max(0)
            });
            let config_synced = !node.config_dirty
                && node.desired_config_hash.is_some()
                && node.desired_config_hash == node.applied_config_hash;
            let (health_status, health_reason) =
                access_node_health_status(node.last_heartbeat_at, node.config_dirty, config_synced);
            json!({
                "id": node.id,
                "name": node.name,
                "public_host": node.public_host,
                "public_port": node.public_port,
                "ssh_host": node.ssh_host,
                "remark": node.remark,
                "status": health_status,
                "agent_version": node.agent_version,
                "config_dirty": node.config_dirty,
                "config_dirty_at": node.config_dirty_at,
                "config_dirty_reason": node.config_dirty_reason,
                "desired_config_hash": node.desired_config_hash,
                "applied_config_hash": node.applied_config_hash,
                "config_synced": config_synced,
                "last_heartbeat_at": node.last_heartbeat_at,
                "last_traffic_report_at": node.last_traffic_report_at,
                "last_traffic_success_at": node.last_traffic_success_at,
                "heartbeat_age_seconds": heartbeat_age_seconds,
                "health_status": health_status,
                "health_reason": health_reason,
                "tls_certificates": node.tls_certificates,
                "tls_cert_last_report_at": node.tls_cert_last_report_at,
                "tls_renew_request_id": node.tls_renew_request_id,
                "tls_renew_requested_at": node.tls_renew_requested_at,
                "tls_renew_completed_at": node.tls_renew_completed_at,
                "tls_renew_status": node.tls_renew_status,
                "tls_renew_message": node.tls_renew_message,
                "cert_domain": node.cert_domain,
                "acme_email": node.acme_email,
                "cf_enabled": node.cf_enabled,
                "cf_domain": node.cf_domain,
                "cf_cert_mode": node.cf_cert_mode,
                "ip_direct_address": node.ip_direct_address,
                // 内核能力软状态(§7.7.1):供前端显示"内核待升级·需重启"与降级提示。
                "kernel_connmark_available": node.kernel_connmark_available,
                "kernel_upgrade_pending": node.kernel_upgrade_pending,
                // 整机重启请求软状态:reboot_status/message 供前端按钮置灰与提示。
                "reboot_status": node.reboot_status,
                "reboot_message": node.reboot_message,
                // 多域名读模型:节点必含 domains[](空则 []),每项 id/domain/kind/is_primary/cert_status。
                "domains": node.domains
            })
        })
        .collect::<Vec<_>>();

    let exit_pools = data
        .exit_pools
        .values()
        .enumerate()
        .map(|(index, pool)| {
            let healthy_members = pool
                .members
                .iter()
                .filter(|member| {
                    member.healthy && xray_exit_protocol_for_endpoint(member).is_some()
                })
                .count();
            let members = pool
                .members
                .iter()
                .map(|member| {
                    let status = if member.status.is_empty() {
                        member_status_from_health(member.healthy).to_string()
                    } else {
                        member.status.clone()
                    };
                    // 后台读模型只需要成员身份和调度状态，不能回显上游地址或凭据。
                    json!({
                        "id": member.id,
                        "resource_name": member.resource_name,
                        "outbound_type": member.outbound_type,
                        "status": status,
                        "host": "",
                        "port": 0,
                        "weight": member.weight,
                        "priority": member.priority,
                        "allow_new_assignments": member.allow_new_assignments,
                        "healthy": member.healthy
                    })
                })
                .collect::<Vec<_>>();

            json!({
                "id": index + 1,
                "uuid": pool.id,
                "name": pool.name,
                "region_code": pool.region_code,
                "region_name": pool.region_code,
                "status": if !pool.enabled {
                    "offline"
                } else if healthy_members > 0 {
                    "healthy"
                } else {
                    "offline"
                },
                "healthy_members": healthy_members,
                "total_members": pool.members.len(),
                "strategy": pool.strategy,
                "active_assignments": data
                    .user_exit_assignments
                    .iter()
                    .filter(|assignment| assignment.exit_pool_id == pool.id)
                    .count(),
                "members": members
            })
        })
        .collect::<Vec<_>>();

    let mut line_group_values = data.line_groups.values().collect::<Vec<_>>();
    line_group_values.sort_by(|left, right| {
        left.sort_weight
            .cmp(&right.sort_weight)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    let line_groups = line_group_values
        .into_iter()
        .map(|group| {
            json!({
                "id": group.id,
                "name": group.name,
                "country_code": group.country_code,
                "group_level": "group",
                "parent_group_id": null,
                "sort_weight": group.sort_weight,
                "billing_multiplier": group.billing_multiplier,
                "enabled": group.enabled,
                "line_ids": group.line_ids,
                "binding_node_ids": group.binding_node_ids,
                "dedicated_rules": group.dedicated_rules,
                "rule_set_bindings": group.rule_set_bindings.iter().map(|binding| {
                    json!({
                        "rule_set_id": binding.rule_set_id,
                        "rule_set_name": binding.rule_set_name,
                        "enabled": binding.enabled,
                        "position": binding.position,
                        "rules": binding.rules
                    })
                }).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();

    json!({
        "access_nodes": access_nodes,
        "access_lines": access_lines,
        "exit_pools": exit_pools,
        "line_groups": line_groups,
    })
}

pub fn exit_pools_json(data: &StoreData) -> serde_json::Value {
    let pools = data
        .exit_pools
        .values()
        .enumerate()
        .map(|(index, pool)| {
            let healthy_members = pool
                .members
                .iter()
                .filter(|member| {
                    member.healthy && xray_exit_protocol_for_endpoint(member).is_some()
                })
                .count();
            json!({
                "id": index + 1,
                "uuid": pool.id,
                "name": pool.name,
                "region_code": pool.region_code,
                "region_name": pool.region_code,
                "status": if !pool.enabled {
                    "offline"
                } else if healthy_members > 0 {
                    "healthy"
                } else {
                    "offline"
                },
                "healthy_members": healthy_members,
                "total_members": pool.members.len(),
                "strategy": pool.strategy,
                "active_assignments": data
                    .user_exit_assignments
                    .iter()
                    .filter(|assignment| assignment.exit_pool_id == pool.id)
                    .count()
            })
        })
        .collect::<Vec<_>>();
    json!({ "exit_pools": pools })
}

fn member_status_from_health(healthy: bool) -> &'static str {
    if healthy {
        "healthy"
    } else {
        "offline"
    }
}

pub fn operations_summary_json(data: &StoreData) -> serde_json::Value {
    let traffic_ledgers = data
        .ledgers
        .iter()
        .filter(|ledger| ledger.traffic_source == "access_line")
        .collect::<Vec<_>>();
    let traffic_real_total = traffic_ledgers
        .iter()
        .map(|ledger| ledger.delta_uplink + ledger.delta_downlink)
        .sum::<u64>();
    let traffic_billed_total = traffic_ledgers
        .iter()
        .map(|ledger| ledger.billed_bytes)
        .sum::<u64>();
    let healthy_exit_pool_count = data
        .exit_pools
        .values()
        .filter(|pool| {
            pool.members
                .iter()
                .any(|member| member.healthy && xray_exit_protocol_for_endpoint(member).is_some())
        })
        .count();
    let exit_probe_problem_count = data
        .access_exit_probe_states
        .values()
        .filter(|state| state.effective_status == "offline")
        .count();
    json!({
        "active_users": data.users.values().filter(|user| !user.disabled).count(),
        "active_access_lines": data.access_lines.values().filter(|line| line.enabled).count(),
        "healthy_exit_pools": healthy_exit_pool_count,
        "exit_probe_problem_count": exit_probe_problem_count,
        "exit_probe_state_count": data.access_exit_probe_states.len(),
        "monthly_billed_traffic_gb": bytes_to_gb(data.ledgers.iter().map(|ledger| ledger.billed_bytes).sum()),
        "traffic_health": {
            "generated_at": Utc::now(),
            "windows": {
                "today": {"real_bytes": traffic_real_total, "billed_bytes": traffic_billed_total},
                "week": {"real_bytes": traffic_real_total, "billed_bytes": traffic_billed_total},
                "month": {"real_bytes": traffic_real_total, "billed_bytes": traffic_billed_total},
                "total": {"real_bytes": traffic_real_total, "billed_bytes": traffic_billed_total}
            },
            "line_items": [],
            "node_items": [],
            "exit_items": []
        },
        "config_dirty_nodes": data.access_nodes.values().filter(|node| node.config_dirty).count(),
        "recent_events": [{
            "id": 1,
            "level": "info",
            "title": "Rust 控制面运行中",
            "time": Utc::now()
        }],
        "alerts": [],
        "alert_summary": {
            "danger_count": 0,
            "warning_count": 0,
            "info_count": 0,
            "total_count": 0,
            "generated_at": Utc::now()
        },
        "access_node_count": data.access_nodes.len(),
        "access_line_count": data.access_lines.len(),
        "line_group_count": data.line_groups.len(),
        "exit_pool_count": data.exit_pools.len(),
        "ledger_count": data.ledgers.len(),
        "generated_at": Utc::now(),
    })
}

pub fn operations_ledger_ranking_json(data: &StoreData, limit: usize) -> serde_json::Value {
    #[derive(Debug, Default)]
    struct Aggregate {
        ledger_count: u64,
        delta_uplink: u64,
        delta_downlink: u64,
        delta_total: u64,
        billed_bytes: u64,
        latest_collected_at: Option<DateTime<Utc>>,
    }

    let mut aggregates = HashMap::<Uuid, Aggregate>::new();
    let mut total = Aggregate::default();
    for ledger in data
        .ledgers
        .iter()
        .filter(|ledger| ledger.traffic_source == "access_line")
    {
        let delta_total = ledger.delta_uplink.saturating_add(ledger.delta_downlink);
        let entry = aggregates.entry(ledger.access_line_id).or_default();
        entry.ledger_count = entry.ledger_count.saturating_add(1);
        entry.delta_uplink = entry.delta_uplink.saturating_add(ledger.delta_uplink);
        entry.delta_downlink = entry.delta_downlink.saturating_add(ledger.delta_downlink);
        entry.delta_total = entry.delta_total.saturating_add(delta_total);
        entry.billed_bytes = entry.billed_bytes.saturating_add(ledger.billed_bytes);
        entry.latest_collected_at = Some(
            entry
                .latest_collected_at
                .map(|current| current.max(ledger.collected_at))
                .unwrap_or(ledger.collected_at),
        );

        total.ledger_count = total.ledger_count.saturating_add(1);
        total.delta_uplink = total.delta_uplink.saturating_add(ledger.delta_uplink);
        total.delta_downlink = total.delta_downlink.saturating_add(ledger.delta_downlink);
        total.delta_total = total.delta_total.saturating_add(delta_total);
        total.billed_bytes = total.billed_bytes.saturating_add(ledger.billed_bytes);
        total.latest_collected_at = Some(
            total
                .latest_collected_at
                .map(|current| current.max(ledger.collected_at))
                .unwrap_or(ledger.collected_at),
        );
    }

    let line_name = |access_line_id: &Uuid| {
        data.access_lines
            .get(access_line_id)
            .map(|line| line.name.as_str())
            .unwrap_or("")
    };
    let mut rows = aggregates.into_iter().collect::<Vec<_>>();
    rows.sort_by(|(left_id, left), (right_id, right)| {
        right
            .billed_bytes
            .cmp(&left.billed_bytes)
            .then_with(|| right.delta_total.cmp(&left.delta_total))
            .then_with(|| right.ledger_count.cmp(&left.ledger_count))
            .then_with(|| line_name(left_id).cmp(line_name(right_id)))
    });

    json!({
        "source": "usage_ledgers+usage_daily_rollups",
        "limit": limit.clamp(1, 100),
        "generated_at": Utc::now(),
        "totals": {
            "access_line_count": rows.len(),
            "ledger_count": total.ledger_count,
            "delta_uplink": total.delta_uplink,
            "delta_downlink": total.delta_downlink,
            "delta_total": total.delta_total,
            "real_bytes": total.delta_total,
            "billed_uplink": 0,
            "billed_downlink": 0,
            "billed_bytes": total.billed_bytes,
            "latest_collected_at": total.latest_collected_at
        },
        "items": rows
            .into_iter()
            .take(limit.clamp(1, 100))
            .enumerate()
            .map(|(index, (access_line_id, aggregate))| {
                let line = data.access_lines.get(&access_line_id);
                let access_node_name = line
                    .and_then(|line| data.access_nodes.get(&line.access_node_id))
                    .map(|node| node.name.as_str())
                    .unwrap_or("");
                json!({
                    "rank": index + 1,
                    "access_line_id": access_line_id,
                    "access_line_name": line.map(|line| line.name.as_str()).unwrap_or(""),
                    "access_node_name": access_node_name,
                    "region_code": line.map(|line| line.region_code.as_str()).unwrap_or(""),
                    "listen_host": line.map(|line| line.listen_host.as_str()).unwrap_or(""),
                    "listen_port": line.map(|line| line.listen_port).unwrap_or_default(),
                    "ledger_count": aggregate.ledger_count,
                    "delta_uplink": aggregate.delta_uplink,
                    "delta_downlink": aggregate.delta_downlink,
                    "delta_total": aggregate.delta_total,
                    "real_bytes": aggregate.delta_total,
                    "billed_uplink": 0,
                    "billed_downlink": 0,
                    "billed_bytes": aggregate.billed_bytes,
                    "latest_collected_at": aggregate.latest_collected_at
                })
            })
            .collect::<Vec<_>>()
    })
}
