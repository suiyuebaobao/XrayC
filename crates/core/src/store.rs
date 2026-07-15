//! 本文件实现 XrayC V2 的内存开发存储。
//! 内存 Store 供本地开发、单元测试和无数据库回退路径使用。
//! 结构布局尽量贴近 PostgreSQL 模型，降低两套存储的行为差异。
//! 这里也放置只依赖内存快照的轻量解析 helper。
//! 套餐线路组授权优先按单层分组直接读取线路。
//! 订阅、计费和读模型复用同一展开逻辑，避免各层 SQL 语义漂移。
//! 新增字段应同步 seeded 数据，保证默认测试快照可用。
//! 文件只维护内存结构和同步方法，不访问网络或数据库。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use crate::model::*;
use chrono::{Duration, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<StoreData>>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreData {
    pub access_nodes: HashMap<Uuid, AccessNode>,
    pub access_entries: HashMap<Uuid, AccessEntry>,
    pub access_entry_exit_bindings: HashMap<Uuid, AccessEntryExitBinding>,
    pub access_lines: HashMap<Uuid, AccessLine>,
    pub exit_pools: HashMap<Uuid, ExitPool>,
    pub local_exit_endpoints: Vec<ExitEndpoint>,
    pub line_groups: HashMap<Uuid, LineGroup>,
    pub plans: HashMap<Uuid, Plan>,
    pub users: HashMap<Uuid, User>,
    pub subscriptions: HashMap<Uuid, UserSubscription>,
    pub tokens: HashMap<String, SubscriptionToken>,
    pub user_access_line_assignments: Vec<UserAccessLineAssignment>,
    pub user_access_line_assignments_authoritative: bool,
    pub user_exit_assignments: Vec<UserExitAssignment>,
    pub access_exit_probe_states: HashMap<(Uuid, Uuid), AccessExitProbeState>,
    pub snapshots: HashMap<(Uuid, String), TrafficSnapshot>,
    pub ledgers: Vec<UsageLedger>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPlanLineGroup {
    pub section_group_id: Uuid,
    pub section_group_name: String,
    pub line_group_id: Uuid,
    pub line_group_name: String,
    pub line_ids: Vec<Uuid>,
    pub binding_node_ids: Vec<Uuid>,
    pub dedicated_rules: Vec<String>,
    pub billing_multiplier: f64,
}

impl StoreData {
    pub fn resolved_plan_line_groups(&self, plan: &Plan) -> Vec<ResolvedPlanLineGroup> {
        let mut resolved = Vec::new();
        let mut seen = HashSet::new();
        for binding in plan.authorized_line_groups() {
            let Some(group) = self.line_groups.get(&binding.line_group_id) else {
                continue;
            };
            if !group.enabled {
                continue;
            }
            if seen.insert(group.id) {
                resolved.push(resolved_line_group_from_binding(&binding, group));
            }
        }
        resolved
    }
}

fn resolved_line_group_from_binding(
    binding: &PlanLineGroup,
    group: &LineGroup,
) -> ResolvedPlanLineGroup {
    ResolvedPlanLineGroup {
        section_group_id: group.id,
        section_group_name: group.name.clone(),
        line_group_id: group.id,
        line_group_name: group.name.clone(),
        line_ids: group.line_ids.clone(),
        binding_node_ids: group.binding_node_ids.clone(),
        dedicated_rules: rules_for_line_group(group),
        billing_multiplier: binding.billing_multiplier,
    }
}

fn rules_for_line_group(group: &LineGroup) -> Vec<String> {
    if group.rule_set_bindings.is_empty() {
        return group.dedicated_rules.clone();
    }

    let mut bindings = group
        .rule_set_bindings
        .iter()
        .filter(|binding| binding.enabled)
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.rule_set_name.cmp(&right.rule_set_name))
            .then_with(|| left.rule_set_id.cmp(&right.rule_set_id))
    });
    let rules = bindings
        .into_iter()
        .flat_map(|binding| binding.rules.iter().cloned())
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect::<Vec<_>>();
    rules
}

impl MemoryStore {
    pub fn seeded() -> Self {
        let store = Self::default();
        let access_node_id = Uuid::new_v4();
        let access_line_id = Uuid::new_v4();
        let exit_pool_id = Uuid::new_v4();
        let line_group_id = Uuid::new_v4();
        let plan_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let user_uuid = Uuid::new_v4().to_string();
        let xray_user_key = format!("u-{}@xrayc.local", user_id.simple());

        let endpoint = ExitEndpoint {
            id: Uuid::new_v4(),
            resource_name: "seeded-socks-exit".to_string(),
            ownership: "third_party".to_string(),
            owner_access_node_id: None,
            outbound_type: EndpointType::Socks,
            host: "198.51.100.10".to_string(),
            port: 1080,
            outbound_config: serde_json::json!({}),
            stream_config: serde_json::json!({}),
            weight: 100,
            priority: 100,
            allow_new_assignments: true,
            healthy: true,
            status: "healthy".to_string(),
        };
        let endpoint_id = endpoint.id;

        let mut data = store.inner.write().expect("memory store poisoned");
        data.access_nodes.insert(
            access_node_id,
            AccessNode {
                id: access_node_id,
                name: "本机中转节点".to_string(),
                public_host: "access.example.test".to_string(),
                public_port: 443,
                ssh_host: String::new(),
                remark: String::new(),
                agent_token_hash:
                    "b83e3e191f2acfe2c3c27f2079c5c13ea8409e9abbf31b671d421d80bbc847c9".to_string(),
                status: "unknown".to_string(),
                agent_version: String::new(),
                config_dirty: true,
                desired_config_hash: Some("seeded-desired".to_string()),
                applied_config_hash: None,
                last_heartbeat_at: None,
                last_traffic_report_at: None,
                last_traffic_success_at: None,
                config_dirty_at: None,
                config_dirty_reason: "seed".to_string(),
                tls_certificates: serde_json::json!([]),
                tls_cert_last_report_at: None,
                tls_renew_request_id: None,
                tls_renew_requested_at: None,
                tls_renew_completed_at: None,
                tls_renew_status: String::new(),
                tls_renew_message: String::new(),
                cert_domain: String::new(),
                acme_email: String::new(),
                cf_enabled: false,
                cf_domain: String::new(),
                cf_cert_mode: "reuse_direct".to_string(),
                ip_direct_address: String::new(),
                // 内核能力软状态:种子默认按可用、无待重启、无重启请求。
                kernel_connmark_available: true,
                kernel_upgrade_pending: false,
                reboot_status: String::new(),
                reboot_message: String::new(),
                domains: Vec::new(),
            },
        );
        data.exit_pools.insert(
            exit_pool_id,
            ExitPool {
                id: exit_pool_id,
                name: "默认出口池".to_string(),
                region_code: "HK".to_string(),
                strategy: "priority".to_string(),
                enabled: true,
                members: vec![endpoint],
            },
        );
        data.access_lines.insert(
            access_line_id,
            AccessLine {
                id: access_line_id,
                name: "香港 01".to_string(),
                access_node_id,
                line_group_id: None,
                exit_endpoint_id: Some(endpoint_id),
                exit_pool_id,
                listen_host: "access.example.test".to_string(),
                listen_port: 443,
                protocol: "vless".to_string(),
                transport: "tcp".to_string(),
                uuid: user_uuid,
                server_name: "www.cloudflare.com".to_string(),
                public_key: "seeded-public-key".to_string(),
                short_id: "a1b2c3d4".to_string(),
                flow: String::new(),
                inbound_config: serde_json::json!({}),
                udp_enabled: true,
                udp_packet_encoding: String::new(),
                xhttp_path: String::new(),
                xhttp_host: String::new(),
                xhttp_mode: "auto".to_string(),
                region_code: "HK".to_string(),
                region_name: "香港".to_string(),
                region_flag: "🇭🇰".to_string(),
                enabled: true,
            },
        );
        data.line_groups.insert(
            line_group_id,
            LineGroup {
                id: line_group_id,
                name: "默认分组".to_string(),
                country_code: "HK".to_string(),
                icon: "🇭🇰".to_string(),
                sort_weight: 100,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: Some(exit_pool_id),
                line_ids: vec![endpoint_id],
                binding_node_ids: Vec::new(),
                dedicated_rules: Vec::new(),
                rule_set_bindings: Vec::new(),
            },
        );
        data.plans.insert(
            plan_id,
            Plan {
                id: plan_id,
                name: "基础套餐".to_string(),
                is_default: true,
                traffic_limit_bytes: 10 * 1024 * 1024 * 1024,
                rate_limit_bps: 0,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
                billing_multiplier: 1.0,
                price_cents: 0,
                currency: "USDT".to_string(),
                duration_days: 30,
                sort_weight: 1,
                line_group_ids: vec![line_group_id],
                line_groups: vec![PlanLineGroup {
                    line_group_id,
                    billing_multiplier: 1.0,
                }],
                default_line_group_id: Some(line_group_id),
            },
        );
        data.users.insert(
            user_id,
            User {
                id: user_id,
                email: "demo@example.test".to_string(),
                xray_user_key: xray_user_key.clone(),
                access_credential: user_id.to_string(),
                disabled: false,
                rate_limit_bps: None,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        );
        data.subscriptions.insert(
            user_id,
            UserSubscription {
                user_id,
                plan_id,
                active: true,
                expires_at: Utc::now() + Duration::days(30),
                used_bytes: 0,
                limit_bytes: 10 * 1024 * 1024 * 1024,
            },
        );
        data.tokens.insert(
            "demo-token".to_string(),
            SubscriptionToken {
                token: "demo-token".to_string(),
                user_id,
            },
        );
        drop(data);
        store
    }

    pub fn read<R>(&self, f: impl FnOnce(&StoreData) -> R) -> R {
        let data = self.inner.read().expect("memory store poisoned");
        f(&data)
    }

    pub fn write<R>(&self, f: impl FnOnce(&mut StoreData) -> R) -> R {
        let mut data = self.inner.write().expect("memory store poisoned");
        f(&mut data)
    }
}
