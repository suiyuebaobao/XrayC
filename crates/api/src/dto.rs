//! 本模块集中存放 HTTP handler 使用的请求和查询 DTO。
//! 这些结构体只承载反序列化字段和默认值。
//! 数据库访问、路由注册和业务处理器不得放在这里。
//! DTO 字段保持 snake_case 兼容前端和脚本。
//! 默认值函数统一来自 request_defaults。
//! 线路分组 DTO 保留历史层级字段，当前请求默认按普通分组处理。
//! 旧客户端未传层级时按扁平分组处理。
//! 新增请求字段应优先可选，避免破坏历史接口。
//! 注释必须使用中文，便于维护规则追踪。
//! 文件接近行数上限，新增大块 DTO 应拆分模块。

use crate::request_defaults::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use uuid::Uuid;
#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    pub(crate) account: String,
    pub(crate) password: String,
    pub(crate) captcha_id: Option<Uuid>,
    pub(crate) captcha_answer: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterRequest {
    // 与登录口径统一:注册也接受 `account` 字段作为邮箱别名(login 用 account)。
    #[serde(alias = "account")]
    pub(crate) email: String,
    pub(crate) password: String,
    pub(crate) invite_code: Option<String>,
    pub(crate) captcha_id: Option<Uuid>,
    pub(crate) captcha_answer: Option<String>,
    pub(crate) email_code_id: Option<Uuid>,
    pub(crate) email_code: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CaptchaQuery {
    pub(crate) scene: String,
    pub(crate) target: String,
}
#[derive(Debug, Deserialize)]
pub(crate) struct EmailCodeRequest {
    pub(crate) email: String,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateOrderRequest {
    pub(crate) plan_id: Uuid,
    #[serde(default)]
    pub(crate) channel: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct AdminOrdersQuery {
    pub(crate) page: Option<i64>,
    pub(crate) page_size: Option<i64>,
    pub(crate) status: Option<String>,
    pub(crate) user: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) keyword: Option<String>,
    pub(crate) order_no: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentCallbackRequest {
    pub(crate) order_no: String,
    pub(crate) tx_id: String,
    pub(crate) amount_cents: i64,
    #[serde(default = "default_usdt")]
    pub(crate) currency: String,
    pub(crate) payment_address: Option<String>,
    pub(crate) status: String,
    pub(crate) confirmations: Option<i32>,
    pub(crate) paid_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct RedeemCodeRequest {
    pub(crate) code: String,
}
// 已登录用户改密：提交邮箱验证码 + 新密码；邮箱取自 JWT，不在请求体内。
#[derive(Debug, Deserialize)]
pub(crate) struct ChangePasswordRequest {
    pub(crate) email_code_id: Option<Uuid>,
    pub(crate) email_code: Option<String>,
    pub(crate) new_password: String,
}
// 管理员重置用户密码：admin 本身即授权，直接给目标用户设新密码，无需邮箱验证码。
#[derive(Debug, Deserialize)]
pub(crate) struct AdminResetPasswordRequest {
    pub(crate) new_password: String,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateRedeemCodesRequest {
    pub(crate) plan_id: Uuid,
    #[serde(default = "default_redeem_count")]
    pub(crate) count: i32,
    pub(crate) duration_days: Option<i32>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateAccessNodeRequest {
    pub(crate) name: String,
    pub(crate) public_host: String,
    pub(crate) public_port: Option<u16>,
    pub(crate) ssh_host: Option<String>,
    pub(crate) agent_token: Option<String>,
    pub(crate) remark: Option<String>,
    // 节点多模式 CF 字段:直连证书域名 + ACME 邮箱 + CF 身份;均可缺省。
    pub(crate) cert_domain: Option<String>,
    pub(crate) acme_email: Option<String>,
    #[serde(default)]
    pub(crate) cf_enabled: bool,
    pub(crate) cf_domain: Option<String>,
    // IP 直连地址(节点公网 IP),给 Reality/Shadowsocks 直连;可缺省。
    pub(crate) ip_direct_address: Option<String>,
    // CF 证书模式:reuse_direct/dns01;缺省时由 store 按 cf_domain 派生(有则 dns01)。
    pub(crate) cf_cert_mode: Option<String>,
    // 多域名清单(多域名 Phase 6):每条 {domain,kind,is_primary?};缺省空表示只用单字段主域名。
    #[serde(default)]
    pub(crate) domains: Vec<NodeDomainInput>,
}

/// 节点多域名请求项(多域名 Phase 6)。
/// domain 非空、kind∈{direct,cf};is_primary 默认 false;acme_email/cf_cert_mode 可选透传给 store 派生。
/// 主域名(cert_domain/cf_domain)已由节点单字段同步进 node_domains,这里只对账新增/移除的额外域名。
#[derive(Debug, Deserialize)]
pub(crate) struct NodeDomainInput {
    pub(crate) domain: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) is_primary: bool,
    pub(crate) acme_email: Option<String>,
    pub(crate) cf_cert_mode: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct AgentInstallGuideRequest {
    pub(crate) access_node_id: Option<Uuid>,
    #[serde(alias = "panel_url")]
    pub(crate) control_plane_url: Option<String>,
    pub(crate) install_dir: Option<String>,
    #[serde(alias = "compose_project_name")]
    pub(crate) compose_project: Option<String>,
    pub(crate) xray_api_server: Option<String>,
    pub(crate) xray_api_listen_host: Option<String>,
    #[serde(alias = "xray_api_port")]
    pub(crate) xray_api_listen_port: Option<u16>,
    pub(crate) heartbeat_interval_seconds: Option<u64>,
    pub(crate) traffic_interval_seconds: Option<u64>,
    pub(crate) session_idle_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) expected_listen_ports: Vec<u16>,
    #[serde(default)]
    pub(crate) tls_cert_domains: Vec<String>,
    pub(crate) acme_email: Option<String>,
    // CF API Token:仅安装/续期期用,注入 agent env 给 certbot DNS-01,绝不落库/不回显。
    pub(crate) cf_api_token: Option<String>,
    #[serde(default)]
    pub(crate) disable_legacy_systemd_units: bool,
    #[serde(default)]
    pub(crate) force_reinstall: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OneClickAgentInstallRequest {
    pub(crate) name: String,
    pub(crate) public_host: String,
    pub(crate) public_port: Option<u16>,
    pub(crate) remark: Option<String>,
    pub(crate) ssh_host: String,
    pub(crate) ssh_port: Option<u16>,
    pub(crate) ssh_user: String,
    pub(crate) ssh_password: Option<String>,
    pub(crate) ssh_private_key: Option<String>,
    #[serde(alias = "panel_url")]
    pub(crate) control_plane_url: Option<String>,
    pub(crate) install_dir: Option<String>,
    #[serde(alias = "compose_project_name")]
    pub(crate) compose_project: Option<String>,
    pub(crate) xray_api_server: Option<String>,
    pub(crate) xray_api_listen_host: Option<String>,
    #[serde(alias = "xray_api_port")]
    pub(crate) xray_api_listen_port: Option<u16>,
    pub(crate) heartbeat_interval_seconds: Option<u64>,
    pub(crate) traffic_interval_seconds: Option<u64>,
    pub(crate) session_idle_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) expected_listen_ports: Vec<u16>,
    #[serde(default)]
    pub(crate) tls_cert_domains: Vec<String>,
    pub(crate) acme_email: Option<String>,
    // 多模式 CF 字段:一键安装直接把节点建成支持 CF + 直连。
    // cert_domain=灰云直连证书域名;cf_enabled 开则 cf_domain=橙云对外域名。
    pub(crate) cert_domain: Option<String>,
    #[serde(default)]
    pub(crate) cf_enabled: bool,
    pub(crate) cf_domain: Option<String>,
    // IP 直连地址(节点公网 IP):一键安装直接透传进节点创建,纯 IP 节点装完无需再单独 PUT 补。
    pub(crate) ip_direct_address: Option<String>,
    // CF API Token:仅安装/续期期用,注入 agent env 给 certbot DNS-01,绝不落库/不回显。
    pub(crate) cf_api_token: Option<String>,
    #[serde(default)]
    pub(crate) disable_legacy_systemd_units: bool,
    #[serde(default)]
    pub(crate) force_reinstall: bool,
}

pub(crate) struct AgentInstallGuideResolved {
    pub(crate) guide_id: Uuid,
    pub(crate) access_node_id: Option<Uuid>,
    pub(crate) access_node_name: String,
    pub(crate) control_plane_url: String,
    pub(crate) deploy_artifact_token: String,
    pub(crate) acme_email: String,
    pub(crate) install_dir: String,
    pub(crate) compose_project: String,
    pub(crate) xray_api_server: String,
    pub(crate) xray_api_listen_host: String,
    pub(crate) xray_api_listen_port: u16,
    pub(crate) tls_cert_domains: Vec<String>,
    pub(crate) heartbeat_interval_seconds: u64,
    pub(crate) traffic_interval_seconds: u64,
    pub(crate) session_idle_seconds: u64,
    pub(crate) expected_listen_ports: Vec<u16>,
    pub(crate) disable_legacy_systemd_units: bool,
    pub(crate) force_reinstall: bool,
    // CF API Token:解析后只用于注入 agent env,不进 AdminAccessNodeInput、不落库、不入审计/回显。
    pub(crate) cf_api_token: Option<String>,
    // CF 域名与证书模式:注入 agent env(XRAYC_CF_DOMAIN/XRAYC_CF_CERT_MODE),
    // 部署脚本据此触发 DNS-01 给 CF 域名签自己的证书;非敏感,可入 env 文件。
    pub(crate) cf_domain: Option<String>,
    pub(crate) cf_cert_mode: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeploymentTaskReportRequest {
    pub(crate) report_token: String,
    pub(crate) status: Option<String>,
    pub(crate) step: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) progress_percent: Option<i32>,
    pub(crate) result: Option<serde_json::Value>,
}

pub(crate) struct AdminAuditRecord<'a> {
    pub(crate) action: &'a str,
    pub(crate) resource_type: &'a str,
    pub(crate) resource_id: Option<Uuid>,
    pub(crate) request_summary: serde_json::Value,
    pub(crate) result: &'a str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateExitResourceRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) region_code: String,
    #[serde(default)]
    pub(crate) provider_name: String,
    #[serde(default = "default_third_party")]
    pub(crate) ownership: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateExitResourceRequest {
    pub(crate) name: Option<String>,
    pub(crate) region_code: Option<String>,
    pub(crate) provider_name: Option<String>,
    pub(crate) ownership: Option<String>,
    pub(crate) enabled: Option<bool>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateExitEndpointRequest {
    pub(crate) exit_resource_id: Uuid,
    #[serde(default)]
    pub(crate) name: String,
    pub(crate) outbound_type: String,
    #[serde(default)]
    pub(crate) host: String,
    #[serde(default)]
    pub(crate) port: u16,
    #[serde(default = "empty_object")]
    pub(crate) outbound_config: serde_json::Value,
    #[serde(default = "empty_object")]
    pub(crate) stream_config: serde_json::Value,
    #[serde(default = "empty_object")]
    pub(crate) probe_config: serde_json::Value,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateExitEndpointRequest {
    pub(crate) exit_resource_id: Option<Uuid>,
    pub(crate) name: Option<String>,
    pub(crate) outbound_type: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) outbound_config: Option<serde_json::Value>,
    pub(crate) stream_config: Option<serde_json::Value>,
    pub(crate) probe_config: Option<serde_json::Value>,
    pub(crate) enabled: Option<bool>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct TriggerAccessOperationsProbeRequest {
    pub(crate) exit_endpoint_id: Uuid,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateLocalExitLinesRequest {
    #[serde(default)]
    pub(crate) lines: Vec<CreateLocalExitLineRequest>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct CreateLocalExitLineRequest {
    #[serde(default)]
    pub(crate) resource_name: String,
    #[serde(default)]
    pub(crate) endpoint_name: String,
    #[serde(default)]
    pub(crate) region_code: String,
    #[serde(default)]
    pub(crate) outbound_type: String,
    #[serde(default)]
    pub(crate) network_mode: String,
    #[serde(default)]
    pub(crate) host: String,
    #[serde(default)]
    pub(crate) port: u16,
    #[serde(default = "empty_object")]
    pub(crate) outbound_config: serde_json::Value,
    #[serde(default = "empty_object")]
    pub(crate) stream_config: serde_json::Value,
    #[serde(default = "empty_object")]
    pub(crate) probe_config: serde_json::Value,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    // 选中的节点域名 id(多域名 Phase 4):None 表示免证书直连(回退节点单域名护栏)。
    #[serde(default)]
    pub(crate) node_domain_id: Option<Uuid>,
}
/// 本机出口就地编辑请求(多域名 Phase 4)。
/// 各字段缺省表示本次不改该列;node_domain_id 用 NullableUuidPatch 区分「不改/清空/设值」。
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateLocalExitLineRequest {
    pub(crate) endpoint_name: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) outbound_config: Option<serde_json::Value>,
    pub(crate) stream_config: Option<serde_json::Value>,
    pub(crate) enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_nullable_uuid_patch")]
    pub(crate) node_domain_id: NullableUuidPatch,
}
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAccessNodeRequest {
    pub(crate) name: Option<String>,
    pub(crate) public_host: Option<String>,
    pub(crate) public_port: Option<u16>,
    pub(crate) ssh_host: Option<String>,
    pub(crate) remark: Option<String>,
    // 节点多模式 CF 字段:None 表示本次不更新对应列。
    pub(crate) cert_domain: Option<String>,
    pub(crate) acme_email: Option<String>,
    pub(crate) cf_enabled: Option<bool>,
    pub(crate) cf_domain: Option<String>,
    // IP 直连地址:None 表示本次不更新;cf_cert_mode 由 store 派生(DTO 字段仅兼容入参)。
    pub(crate) ip_direct_address: Option<String>,
    pub(crate) cf_cert_mode: Option<String>,
    // 多域名清单:节点域名的唯一真值源,后端一律以此为准对账 node_domains(新增缺失、删除多余;
    // 传空清单即删光全部未被入口/出口引用的域名)。前端每次保存都回传完整清单,缺省按空清单处理。
    // (历史曾用「空=不改」兼容旧前端,但 Vec 区分不了「没传」与「传了空[]」,导致删最后一个域名永远失败,已废弃该兼容。)
    #[serde(default)]
    pub(crate) domains: Vec<NodeDomainInput>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct BatchDeleteAccessNodesRequest {
    #[serde(default, alias = "ids")]
    pub(crate) access_node_ids: Vec<Uuid>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct AdminPlanRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) traffic_limit_bytes: i64,
    #[serde(default)]
    pub(crate) rate_limit_bps: i64,
    #[serde(default)]
    pub(crate) rate_limit_up_bps: Option<i64>,
    #[serde(default)]
    pub(crate) rate_limit_down_bps: Option<i64>,
    #[serde(default = "default_plan_multiplier")]
    pub(crate) billing_multiplier: f64,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) price_cents: i64,
    #[serde(default = "default_currency")]
    pub(crate) currency: String,
    #[serde(default = "default_duration_days")]
    pub(crate) duration_days: i32,
    #[serde(default = "default_plan_sort_weight")]
    pub(crate) sort_weight: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminPlanUpdateRequest {
    pub(crate) name: Option<String>,
    pub(crate) traffic_limit_bytes: Option<i64>,
    pub(crate) rate_limit_bps: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_nullable_i64_patch")]
    pub(crate) rate_limit_up_bps: NullableI64Patch,
    #[serde(default, deserialize_with = "deserialize_nullable_i64_patch")]
    pub(crate) rate_limit_down_bps: NullableI64Patch,
    pub(crate) billing_multiplier: Option<f64>,
    pub(crate) enabled: Option<bool>,
    pub(crate) price_cents: Option<i64>,
    pub(crate) currency: Option<String>,
    pub(crate) duration_days: Option<i32>,
    pub(crate) sort_weight: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUserUpdateRequest {
    pub(crate) email: Option<String>,
    pub(crate) disabled: Option<bool>,
    pub(crate) is_admin: Option<bool>,
    pub(crate) plan_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_nullable_i64_patch")]
    pub(crate) rate_limit_bps: NullableI64Patch,
    #[serde(default, deserialize_with = "deserialize_nullable_i64_patch")]
    pub(crate) rate_limit_up_bps: NullableI64Patch,
    #[serde(default, deserialize_with = "deserialize_nullable_i64_patch")]
    pub(crate) rate_limit_down_bps: NullableI64Patch,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUserCreateRequest {
    pub(crate) email: String,
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) disabled: bool,
    #[serde(default)]
    pub(crate) is_admin: bool,
    pub(crate) plan_id: Option<Uuid>,
    pub(crate) rate_limit_bps: Option<i64>,
    #[serde(default)]
    pub(crate) rate_limit_up_bps: Option<i64>,
    #[serde(default)]
    pub(crate) rate_limit_down_bps: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUsersBatchDeleteRequest {
    pub(crate) user_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum NullableI64Patch {
    #[default]
    Unset,
    Null,
    Value(i64),
}

impl NullableI64Patch {
    pub(crate) fn into_option_option(self) -> Option<Option<i64>> {
        match self {
            Self::Unset => None,
            Self::Null => Some(None),
            Self::Value(value) => Some(Some(value)),
        }
    }
}

fn deserialize_nullable_i64_patch<'de, D>(deserializer: D) -> Result<NullableI64Patch, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(NullableI64Patch::Null),
        Value::Number(number) => number
            .as_i64()
            .map(NullableI64Patch::Value)
            .ok_or_else(|| serde::de::Error::custom("rate_limit_bps must be an integer")),
        _ => Err(serde::de::Error::custom(
            "rate_limit_bps must be an integer or null",
        )),
    }
}

/// 本机出口编辑里 node_domain_id 的三态补丁:字段缺省=不改、显式 null=清空选中域名、字符串=设为该域名。
/// 与 store 的 Option<Option<Uuid>> 对齐:Unset→None、Null→Some(None)、Value→Some(Some(uuid))。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum NullableUuidPatch {
    #[default]
    Unset,
    Null,
    Value(Uuid),
}

impl NullableUuidPatch {
    pub(crate) fn into_option_option(self) -> Option<Option<Uuid>> {
        match self {
            Self::Unset => None,
            Self::Null => Some(None),
            Self::Value(value) => Some(Some(value)),
        }
    }
}

fn deserialize_nullable_uuid_patch<'de, D>(deserializer: D) -> Result<NullableUuidPatch, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(NullableUuidPatch::Null),
        Value::String(text) => Uuid::parse_str(&text)
            .map(NullableUuidPatch::Value)
            .map_err(|_| serde::de::Error::custom("node_domain_id must be a valid UUID")),
        _ => Err(serde::de::Error::custom(
            "node_domain_id must be a UUID string or null",
        )),
    }
}
