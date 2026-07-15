//! 本模块集中放置 db crate 对外暴露的数据传输类型。
//! 这些类型主要用于管理后台输入、认证输出和 worker 维护结果。
//! 模块只包含轻量结构体定义，不包含 SQL 查询和持久化流程。
//! PgStore 的业务方法仍保留在 crate root，避免拆散事务上下文。
//! 字段加密状态与轮转报告属于公开查询结果，也统一放在这里。
//! 管理端创建和更新输入保持原字段名，确保现有调用方兼容。
//! 支付回调输入只描述入参，不处理签名和数据库写入；维护结果用于 worker 返回清理统计。
//! 新增公开 DTO 应优先放入本模块，避免 lib.rs 再次膨胀。
//! 本模块通过 crate root 重新导出，保持历史 public API 不变。

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct AdminOrderFilters {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub user: Option<String>,
    pub email: Option<String>,
    pub keyword: Option<String>,
    pub order_no: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AdminUserFilters {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
    pub email: Option<String>,
    pub status: Option<String>,
    pub role: Option<String>,
    pub plan_id: Option<Uuid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentTlsCertificateReport {
    pub domain: String,
    pub status: String,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub days_remaining: Option<i64>,
    pub error_summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentTlsRenewResult {
    pub request_id: Uuid,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct AccessNodeTlsRenewalResult {
    pub request_id: Uuid,
    pub status: String,
    pub domains: Vec<String>,
}

/// 管理员触发整机重启的下发结果(§7.7.1):只回请求 ID 与状态,不含任何凭据/SSH。
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct AccessNodeRebootResult {
    pub request_id: Uuid,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone)]
pub struct AuditLogInput {
    pub actor_user_id: Uuid,
    pub actor_email: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub client_ip: String,
    pub request_summary: Value,
    pub result: String,
}

#[derive(Debug, Clone)]
pub struct CreateDeploymentTaskInput {
    pub kind: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub title: String,
    pub summary: String,
    pub safe_metadata: Value,
    pub steps: Value,
    pub created_by_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct DeploymentTaskReportInput {
    pub task_id: Uuid,
    pub report_token: String,
    pub status: Option<String>,
    pub step: Option<String>,
    pub message: Option<String>,
    pub progress_percent: Option<i32>,
    pub result: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RegisteredUser {
    pub user: AuthenticatedUser,
    pub subscription_token: String,
}

#[derive(Debug, Clone, Default)]
pub struct AdminAccessNodeInput {
    pub name: String,
    pub public_host: String,
    pub public_port: u16,
    pub agent_token: String,
    pub remark: String,
    // 直连证书域名(灰云),用于 certbot HTTP-01 申请直连证书;可空。
    pub cert_domain: Option<String>,
    // ACME 邮箱,签发/续期通知用;可空。
    pub acme_email: Option<String>,
    // 是否启用 Cloudflare 接入;写入侧以 cf_domain 派生为准,不信任该入参。
    pub cf_enabled: bool,
    // CF 橙云域名,启用 CF 时对外公布;可空。
    pub cf_domain: Option<String>,
    // IP 直连地址(节点公网 IP),给 Reality/Shadowsocks 直连用;可空。
    pub ip_direct_address: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AdminAccessNodeUpdate {
    pub name: Option<String>,
    pub public_host: Option<String>,
    pub public_port: Option<u16>,
    pub ssh_host: Option<String>,
    pub remark: Option<String>,
    // 以下为节点多模式 CF 字段,None 表示本次不更新该列。
    pub cert_domain: Option<String>,
    pub acme_email: Option<String>,
    pub cf_enabled: Option<bool>,
    pub cf_domain: Option<String>,
    pub ip_direct_address: Option<String>,
}

/// 节点多模式 CF 字段读模型。
/// 仅承载证书/CF 身份列,供安装指南与读回断言使用。
#[derive(Debug, Clone)]
pub struct AccessNodeCfFields {
    pub cert_domain: Option<String>,
    pub acme_email: Option<String>,
    pub cf_enabled: bool,
    pub cf_domain: Option<String>,
    pub cf_cert_mode: String,
}

/// 节点多域名行读模型。
/// 对应 node_domains 一行:某节点的一个直连/CF 域名及其证书锚定信息。
/// kind 仅取 direct/cf;is_primary 标记该 kind 下默认域名(订阅/入口默认值)。
/// cf_cert_mode 仅 kind=cf 有意义(dns01/reuse_direct);direct 行为 None。
#[derive(Debug, Clone)]
pub struct NodeDomain {
    pub id: Uuid,
    pub access_node_id: Uuid,
    pub domain: String,
    pub kind: String,
    pub cf_cert_mode: Option<String>,
    pub acme_email: Option<String>,
    pub is_primary: bool,
    pub cert_status: String,
}

/// 新增节点域名入参。
/// domain 非空、kind∈{direct,cf};cf 域名的 cf_cert_mode 由写入侧按是否给 token/acme 派生。
/// is_primary=true 时该 kind 下其余行的 is_primary 会被清掉,保证同 kind 唯一主域名。
#[derive(Debug, Clone, Default)]
pub struct AddNodeDomainInput {
    pub domain: String,
    pub kind: String,
    pub cf_cert_mode: Option<String>,
    pub acme_email: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct AdminAccessEntryInput {
    pub access_node_id: Uuid,
    pub name: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol: String,
    pub transport: String,
    pub security: String,
    pub server_name: String,
    pub ws_path: String,
    pub ws_host: String,
    pub cdn_enabled: bool,
    pub cdn_provider: String,
    pub cdn_hostname: String,
    pub cdn_server: String,
    pub enabled: bool,
    pub sort_weight: i32,
    /// 选中的 node_domain id(多域名 Phase 2):None 时回退节点单域名行为。
    pub node_domain_id: Option<Uuid>,
    /// 量子加密(VLESS native encryption,后量子)开关:仅非 Reality 的 VLESS 生效;
    /// true 时入口创建/编辑生成 mlkem768x25519plus 密钥串入 inbound_config(详见 routing_entry_reality)。
    pub vless_quantum_encryption: bool,
}

#[derive(Debug, Clone)]
pub struct AdminAccessEntryExitBindingInput {
    pub exit_endpoint_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub sort_weight: i32,
    pub remark: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminAccessNodeDeleteResult {
    pub deleted_node_ids: Vec<Uuid>,
    pub deleted_node_count: i64,
    pub deleted_access_line_count: i64,
    pub retained_usage_ledger_count: i64,
    pub deleted_local_resource_count: i64,
    pub deleted_local_pool_count: i64,
}

#[derive(Debug, Clone)]
pub struct AdminExitResourceInput {
    pub name: String,
    pub region_code: String,
    pub provider_name: String,
    pub ownership: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AdminExitResourceUpdate {
    pub name: Option<String>,
    pub region_code: Option<String>,
    pub provider_name: Option<String>,
    pub ownership: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct AdminExitEndpointInput {
    pub exit_resource_id: Uuid,
    pub name: String,
    pub outbound_type: String,
    pub host: String,
    pub port: u16,
    pub outbound_config: Value,
    pub stream_config: Value,
    pub probe_config: Value,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AdminExitEndpointUpdate {
    pub exit_resource_id: Option<Uuid>,
    pub name: Option<String>,
    pub outbound_type: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub outbound_config: Option<Value>,
    pub stream_config: Option<Value>,
    pub probe_config: Option<Value>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct AdminExitPoolInput {
    pub name: String,
    pub region_code: String,
    pub strategy: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminExitPoolDeleteResult {
    pub deleted: bool,
    pub deleted_access_line_count: i64,
    pub deleted_member_count: i64,
    pub deleted_assignment_count: i64,
    pub retained_usage_ledger_count: i64,
    pub affected_access_node_count: i64,
}

#[derive(Debug, Clone)]
pub struct AdminExitPoolMemberInput {
    pub exit_endpoint_id: Uuid,
    pub weight: i32,
    pub priority: i32,
    pub status: String,
    pub allow_new_assignments: bool,
}

#[derive(Debug, Clone)]
pub struct AdminExitPoolExitInput {
    pub resource_name: String,
    pub endpoint_name: String,
    pub region_code: String,
    pub provider_name: String,
    pub ownership: String,
    pub outbound_type: String,
    pub host: String,
    pub port: u16,
    pub outbound_config: Value,
    pub stream_config: Value,
    pub probe_config: Value,
    pub enabled: bool,
    pub weight: i32,
    pub priority: i32,
    pub status: String,
    pub allow_new_assignments: bool,
}

#[derive(Debug, Clone)]
pub struct AdminExitPoolExitCreated {
    pub exit_resource_id: Uuid,
    pub exit_endpoint_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct AdminAccessLineInput {
    pub name: String,
    pub access_node_id: Uuid,
    pub exit_pool_id: Uuid,
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol: String,
    pub transport: String,
    pub user_uuid: String,
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    pub enabled: bool,
    pub region_code: String,
    pub region_name: String,
    pub region_flag: String,
    pub flow: String,
    pub udp_enabled: bool,
    pub udp_packet_encoding: String,
    pub xhttp_path: String,
    pub xhttp_host: String,
    pub xhttp_mode: String,
    pub identity_mode: String,
    pub user_key_source: String,
    pub inbound_config: Value,
    pub visibility_weight: i32,
}

#[derive(Debug, Clone)]
pub struct AdminAccessNodeGroupEntryInput {
    pub name: String,
    pub exit_endpoint_id: Uuid,
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol: String,
    pub network_mode: String,
    pub inbound_config: Value,
    pub xhttp_mode: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminAccessNodeGroupEntryResult {
    pub created_line_ids: Vec<Uuid>,
    pub exit_endpoint_ids: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct AdminLineGroupInput {
    pub name: String,
    pub country_code: String,
    pub icon: String,
    pub group_level: Option<String>,
    pub parent_group_id: Option<Uuid>,
    pub sort_weight: Option<i32>,
    pub billing_multiplier: Option<f64>,
    pub enabled: Option<bool>,
    pub dedicated_rules: Option<Vec<String>>,
    pub rule_set_bindings: Option<Vec<LineGroupRuleSetBindingInput>>,
}

#[derive(Debug, Clone)]
pub struct LineGroupRuleSetBindingInput {
    pub rule_set_id: Uuid,
    pub position: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AdminSubscriptionRuleSetInput {
    pub name: String,
    pub description: String,
    pub enabled: Option<bool>,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubscriptionRuleSetSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub rules: Vec<String>,
    pub binding_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdminPlanLineGroupInput {
    pub line_group_id: Uuid,
    pub billing_multiplier: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct AdminPlanInput {
    pub name: String,
    pub traffic_limit_bytes: i64,
    pub rate_limit_bps: i64,
    pub rate_limit_up_bps: Option<i64>,
    pub rate_limit_down_bps: Option<i64>,
    pub billing_multiplier: f64,
    pub enabled: bool,
    pub price_cents: i64,
    pub currency: String,
    pub duration_days: i32,
    pub sort_weight: i32,
}

#[derive(Debug, Clone, Default)]
pub struct AdminPlanUpdate {
    pub name: Option<String>,
    pub traffic_limit_bytes: Option<i64>,
    pub rate_limit_bps: Option<i64>,
    pub rate_limit_up_bps: Option<Option<i64>>,
    pub rate_limit_down_bps: Option<Option<i64>>,
    pub billing_multiplier: Option<f64>,
    pub enabled: Option<bool>,
    pub price_cents: Option<i64>,
    pub currency: Option<String>,
    pub duration_days: Option<i32>,
    pub sort_weight: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct AdminUserUpdate {
    pub email: Option<String>,
    pub disabled: Option<bool>,
    pub is_admin: Option<bool>,
    pub plan_id: Option<Uuid>,
    pub rate_limit_bps: Option<Option<i64>>,
    pub rate_limit_up_bps: Option<Option<i64>>,
    pub rate_limit_down_bps: Option<Option<i64>>,
}

#[derive(Debug, Clone)]
pub struct AdminUserCreate {
    pub email: String,
    pub password: String,
    pub disabled: bool,
    pub is_admin: bool,
    pub plan_id: Option<Uuid>,
    pub rate_limit_bps: Option<i64>,
    pub rate_limit_up_bps: Option<i64>,
    pub rate_limit_down_bps: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AdminLocalExitLinesInput {
    pub lines: Vec<AdminLocalExitLineInput>,
}

#[derive(Debug, Clone)]
pub struct AdminLocalExitLineInput {
    pub resource_name: String,
    pub endpoint_name: String,
    pub region_code: String,
    pub outbound_type: String,
    pub network_mode: String,
    pub host: String,
    pub port: u16,
    pub outbound_config: Value,
    pub stream_config: Value,
    pub probe_config: Value,
    pub enabled: bool,
    /// 选中的 node_domain id(多域名 Phase 2):None 时回退节点单域名护栏行为。
    pub node_domain_id: Option<Uuid>,
}

/// 本机出口就地编辑入参(多域名 Phase 4)。
/// 各字段 None 表示本次不改该列;限 ownership=self_hosted 的出口可改。
/// node_domain_id 用 Option<Option<Uuid>>:外层 None 不改,内层 None 显式清空选中域名。
/// outbound_config/stream_config 为 Some 时表示要改协议配置——改配置必须重新跑出口护栏,不绕校验。
#[derive(Debug, Clone, Default)]
pub struct AdminLocalExitLineUpdate {
    pub endpoint_name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub enabled: Option<bool>,
    pub node_domain_id: Option<Option<Uuid>>,
    // 协议连接配置:Some 时重跑 fill+护栏+证书域名+网络安全校验后持久化;None 保留原值。
    pub outbound_config: Option<Value>,
    // 网络模式/传输配置:Some 时按网络模式补默认值并持久化;None 保留原值。
    pub stream_config: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct PaymentCallbackInput {
    pub order_no: String,
    pub tx_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub payment_address: Option<String>,
    pub status: String,
    pub confirmations: Option<i32>,
    pub paid_at: Option<DateTime<Utc>>,
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TrafficLogRetentionPolicy {
    pub detail_retention_days: i64,
    pub prune_enabled: bool,
    pub delete_batch_size: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct DatabaseBackupPolicy {
    pub enabled: bool,
    pub interval_days: i64,
    pub retention_days: i64,
}
