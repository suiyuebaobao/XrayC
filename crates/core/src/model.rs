//! 本文件定义 XrayC 共享领域模型。
//! 这些结构体对应迁移中的 PostgreSQL 表，同时刻意保持和 Web 框架解耦。
//! 核心模型只表达业务字段，不包含数据库连接、HTTP 路由或渲染流程。
//! 套餐、线路组、订阅和计费服务都复用这里的稳定数据结构。
//! 当前分组模型只有一层，套餐授权分组后直接展开组内线路。
//! 历史层级字段不进入核心业务模型，避免订阅路径重新引入父子分组。
//! 新增字段应优先保证 serde 兼容，避免破坏内存 Store 测试。
//! 注释使用中文，便于后续任务按仓库规则继续维护。
//! 文件保持短小，只承载领域结构和少量轻量 helper。
//! 本头部满足前十行中文注释约束。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    Direct,
    Socks,
    Http,
    Vless,
    Trojan,
    Shadowsocks,
    Hysteria,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessNode {
    pub id: Uuid,
    pub name: String,
    pub public_host: String,
    pub public_port: u16,
    #[serde(default)]
    pub ssh_host: String,
    pub remark: String,
    pub agent_token_hash: String,
    pub status: String,
    pub agent_version: String,
    pub config_dirty: bool,
    pub desired_config_hash: Option<String>,
    pub applied_config_hash: Option<String>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub last_traffic_report_at: Option<DateTime<Utc>>,
    pub last_traffic_success_at: Option<DateTime<Utc>>,
    pub config_dirty_at: Option<DateTime<Utc>>,
    pub config_dirty_reason: String,
    #[serde(default)]
    pub tls_certificates: Value,
    pub tls_cert_last_report_at: Option<DateTime<Utc>>,
    pub tls_renew_request_id: Option<Uuid>,
    pub tls_renew_requested_at: Option<DateTime<Utc>>,
    pub tls_renew_completed_at: Option<DateTime<Utc>>,
    pub tls_renew_status: String,
    pub tls_renew_message: String,
    // 多模式 CF 身份(读模型透传给控制面/前端回显;空串表示未配置)。
    // cert_domain:直连灰云证书域名;cf_domain:CF 橙云对外域名;cf_cert_mode:回源证书模式。
    #[serde(default)]
    pub cert_domain: String,
    #[serde(default)]
    pub acme_email: String,
    #[serde(default)]
    pub cf_enabled: bool,
    #[serde(default)]
    pub cf_domain: String,
    #[serde(default)]
    pub cf_cert_mode: String,
    // IP 直连地址(节点公网 IP),给 Reality/Shadowsocks 直连;空串表示未配置。
    #[serde(default)]
    pub ip_direct_address: String,
    // 内核能力软状态(§7.7.1):agent 自检 act_connmark 可加载性 + 是否已装新内核待重启。
    // connmark 不可用即限速跳过下行整形(优雅降级);upgrade_pending 即面板提示"内核待升级·需重启"。
    // 默认 connmark 可用、无待重启,向后兼容旧 agent 不上报。
    #[serde(default = "default_true")]
    pub kernel_connmark_available: bool,
    #[serde(default)]
    pub kernel_upgrade_pending: bool,
    // 整机重启请求软状态(§7.7.1):管理员从面板触发,经 agent 安全自检后执行。
    // reboot_status∈{'','queued','running','success','failed'};message 为脱敏短摘要。
    #[serde(default)]
    pub reboot_status: String,
    #[serde(default)]
    pub reboot_message: String,
    // 多域名 Phase 5b:本节点的全部域名视图(direct/cf 各成行)。
    // 读侧契约「写侧有读侧无」红线:节点读模型必含 domains,空则空数组,不缺键不为 null。
    #[serde(default)]
    pub domains: Vec<NodeDomainView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitEndpoint {
    pub id: Uuid,
    pub resource_name: String,
    pub ownership: String,
    pub owner_access_node_id: Option<Uuid>,
    pub outbound_type: EndpointType,
    pub host: String,
    pub port: u16,
    pub outbound_config: Value,
    #[serde(default)]
    pub stream_config: Value,
    pub weight: u32,
    pub priority: u32,
    pub allow_new_assignments: bool,
    pub healthy: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitPool {
    pub id: Uuid,
    pub name: String,
    pub region_code: String,
    pub strategy: String,
    pub enabled: bool,
    pub members: Vec<ExitEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLine {
    pub id: Uuid,
    pub name: String,
    pub access_node_id: Uuid,
    #[serde(default)]
    pub line_group_id: Option<Uuid>,
    #[serde(default)]
    pub exit_endpoint_id: Option<Uuid>,
    pub exit_pool_id: Uuid,
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol: String,
    pub transport: String,
    pub uuid: String,
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    pub flow: String,
    pub inbound_config: Value,
    pub udp_enabled: bool,
    pub udp_packet_encoding: String,
    pub xhttp_path: String,
    pub xhttp_host: String,
    pub xhttp_mode: String,
    pub region_code: String,
    pub region_name: String,
    pub region_flag: String,
    pub enabled: bool,
}

/// 节点单个域名的读模型视图(多域名 Phase 5)。
/// 对应 node_domains 一行,供节点视图 domains[] 数组与入口/出口选中域名解析复用。
/// kind 取 direct/cf;is_primary 标记该 kind 下默认域名;cert_status 为证书状态。
/// 只携带读侧需要回显的字段,不含 acme/cf_cert_mode 等写入侧内部派生项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeDomainView {
    pub id: Uuid,
    pub domain: String,
    pub kind: String,
    pub is_primary: bool,
    pub cert_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEntry {
    pub id: Uuid,
    pub access_node_id: Uuid,
    pub name: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol: String,
    pub transport: String,
    pub security: String,
    pub user_uuid: String,
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    pub flow: String,
    pub udp_enabled: bool,
    pub udp_packet_encoding: String,
    pub ws_path: String,
    pub ws_host: String,
    pub xhttp_path: String,
    pub xhttp_host: String,
    pub xhttp_mode: String,
    pub cdn_enabled: bool,
    pub cdn_provider: String,
    pub cdn_hostname: String,
    pub cdn_server: String,
    pub inbound_config: Value,
    pub enabled: bool,
    pub sort_weight: i32,
    // 多域名 Phase 5:入口选中的 node_domain 引用与解析结果(读侧契约)。
    // node_domain_id 永远出现在读模型(未选为 null),满足「写侧有读侧无」红线;
    // node_domain 为解析出的 domain/kind 视图,供前端回显与订阅默认域名复用,未选为 None。
    #[serde(default)]
    pub node_domain_id: Option<Uuid>,
    #[serde(default)]
    pub node_domain: Option<NodeDomainView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEntryExitBinding {
    pub id: Uuid,
    pub access_entry_id: Uuid,
    pub exit_endpoint_id: Uuid,
    pub exit_pool_id: Option<Uuid>,
    pub name: String,
    pub enabled: bool,
    pub sort_weight: i32,
    pub remark: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineGroup {
    pub id: Uuid,
    pub name: String,
    pub country_code: String,
    pub icon: String,
    #[serde(default = "default_line_group_sort_weight")]
    pub sort_weight: i32,
    #[serde(default = "default_billing_multiplier")]
    pub billing_multiplier: f64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub exit_pool_id: Option<Uuid>,
    pub line_ids: Vec<Uuid>,
    #[serde(default)]
    pub binding_node_ids: Vec<Uuid>,
    #[serde(default)]
    pub dedicated_rules: Vec<String>,
    #[serde(default)]
    pub rule_set_bindings: Vec<LineGroupRuleSetBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineGroupRuleSetBinding {
    pub rule_set_id: Uuid,
    pub rule_set_name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_line_group_sort_weight")]
    pub position: i32,
    #[serde(default)]
    pub rules: Vec<String>,
}

fn default_line_group_sort_weight() -> i32 {
    100
}

fn default_billing_multiplier() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLineGroup {
    pub line_group_id: Uuid,
    #[serde(default = "default_billing_multiplier")]
    pub billing_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub name: String,
    pub is_default: bool,
    pub traffic_limit_bytes: u64,
    pub rate_limit_bps: u64,
    // 方向限速覆盖（bps，None=沿用 rate_limit_bps 对称值）。用户 2026-06-30 定：上下行可分别限速。
    #[serde(default)]
    pub rate_limit_up_bps: Option<u64>,
    #[serde(default)]
    pub rate_limit_down_bps: Option<u64>,
    pub billing_multiplier: f64,
    pub price_cents: i64,
    pub currency: String,
    pub duration_days: i32,
    pub sort_weight: i32,
    pub line_group_ids: Vec<Uuid>,
    pub line_groups: Vec<PlanLineGroup>,
    #[serde(default)]
    pub default_line_group_id: Option<Uuid>,
}

impl Plan {
    pub fn authorized_line_groups(&self) -> Vec<PlanLineGroup> {
        if !self.line_groups.is_empty() {
            return self.line_groups.clone();
        }

        self.line_group_ids
            .iter()
            .copied()
            .map(|line_group_id| PlanLineGroup {
                line_group_id,
                billing_multiplier: 1.0,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub xray_user_key: String,
    pub access_credential: String,
    pub disabled: bool,
    pub rate_limit_bps: Option<u64>,
    // 方向限速覆盖（bps，None=该方向沿用 rate_limit_bps / 套餐值）。
    #[serde(default)]
    pub rate_limit_up_bps: Option<u64>,
    #[serde(default)]
    pub rate_limit_down_bps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscription {
    pub user_id: Uuid,
    pub plan_id: Uuid,
    pub active: bool,
    pub expires_at: DateTime<Utc>,
    pub used_bytes: u64,
    pub limit_bytes: u64,
}

impl UserSubscription {
    pub fn remaining_bytes(&self) -> u64 {
        self.limit_bytes.saturating_sub(self.used_bytes)
    }

    pub fn add_billed(&mut self, bytes: u64) {
        self.used_bytes = self.used_bytes.saturating_add(bytes);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionToken {
    pub token: String,
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserExitAssignment {
    pub user_id: Uuid,
    pub access_line_id: Uuid,
    pub exit_pool_id: Uuid,
    pub exit_endpoint_id: Uuid,
    pub assigned_at: DateTime<Utc>,
    pub failover_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccessLineAssignment {
    pub user_id: Uuid,
    pub line_group_id: Uuid,
    pub access_line_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessExitProbeState {
    pub access_node_id: Uuid,
    pub exit_endpoint_id: Uuid,
    pub effective_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSnapshot {
    pub access_line_id: Uuid,
    pub xray_user_key: String,
    pub uplink_total: u64,
    pub downlink_total: u64,
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLedger {
    pub access_line_id: Uuid,
    pub user_id: Uuid,
    pub xray_user_key: String,
    pub traffic_source: String,
    pub delta_uplink: u64,
    pub delta_downlink: u64,
    pub billing_multiplier: f64,
    pub billed_bytes: u64,
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedUser {
    pub user_id: Uuid,
    pub xray_user_key: String,
    pub uuid: String,
}

#[cfg(test)]
mod node_domain_read_model_tests {
    //! 多域名 Phase 5.1:核对读模型携带 node_domains 与入口选中域名解析。
    //! 读侧契约:入口读模型必须含 node_domain_id 键(即便 None),并能解析出 domain/kind。
    use super::*;
    use serde_json::json;

    #[test]
    fn test_node_domain_view_carries_domain_kind_primary_cert_status() {
        // 节点域名读模型每项必须带 domain + kind + is_primary + cert_status。
        let view = NodeDomainView {
            id: Uuid::nil(),
            domain: "domain-b.example.test".to_string(),
            kind: "cf".to_string(),
            is_primary: true,
            cert_status: "issued".to_string(),
        };
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(value["domain"], "domain-b.example.test");
        assert_eq!(value["kind"], "cf");
        assert_eq!(value["is_primary"], true);
        assert_eq!(value["cert_status"], "issued");
    }

    #[test]
    fn test_access_entry_read_model_always_exposes_node_domain_id_key() {
        // 红线「写侧有读侧无」:入口读模型必须始终含 node_domain_id 键,未选时为 null。
        let entry = AccessEntry {
            id: Uuid::nil(),
            access_node_id: Uuid::nil(),
            name: "e".to_string(),
            listen_host: String::new(),
            listen_port: 443,
            protocol: "vless".to_string(),
            transport: "ws".to_string(),
            security: "tls".to_string(),
            user_uuid: String::new(),
            server_name: String::new(),
            public_key: String::new(),
            short_id: String::new(),
            flow: String::new(),
            udp_enabled: false,
            udp_packet_encoding: String::new(),
            ws_path: String::new(),
            ws_host: String::new(),
            xhttp_path: String::new(),
            xhttp_host: String::new(),
            xhttp_mode: String::new(),
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            inbound_config: json!({}),
            enabled: true,
            sort_weight: 0,
            node_domain_id: None,
            node_domain: None,
        };
        let value = serde_json::to_value(&entry).unwrap();
        // 键必须存在(读侧不缺字段),未选域名时为 null。
        assert!(value.as_object().unwrap().contains_key("node_domain_id"));
        assert!(value["node_domain_id"].is_null());
        assert!(value["node_domain"].is_null());
    }

    #[test]
    fn test_access_entry_read_model_resolves_selected_node_domain() {
        // 入口选了 node_domain → 读模型解析出 node_domain(domain+kind),供前端回显/订阅默认值。
        let domain_id = Uuid::from_u128(7);
        let entry = AccessEntry {
            id: Uuid::nil(),
            access_node_id: Uuid::nil(),
            name: "e".to_string(),
            listen_host: String::new(),
            listen_port: 443,
            protocol: "trojan".to_string(),
            transport: "tcp".to_string(),
            security: "tls".to_string(),
            user_uuid: String::new(),
            server_name: "domain-b.example.test".to_string(),
            public_key: String::new(),
            short_id: String::new(),
            flow: String::new(),
            udp_enabled: false,
            udp_packet_encoding: String::new(),
            ws_path: String::new(),
            ws_host: String::new(),
            xhttp_path: String::new(),
            xhttp_host: String::new(),
            xhttp_mode: String::new(),
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            inbound_config: json!({}),
            enabled: true,
            sort_weight: 0,
            node_domain_id: Some(domain_id),
            node_domain: Some(NodeDomainView {
                id: domain_id,
                domain: "domain-b.example.test".to_string(),
                kind: "direct".to_string(),
                is_primary: false,
                cert_status: "issued".to_string(),
            }),
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["node_domain_id"], domain_id.to_string());
        assert_eq!(value["node_domain"]["domain"], "domain-b.example.test");
        assert_eq!(value["node_domain"]["kind"], "direct");
    }
}
