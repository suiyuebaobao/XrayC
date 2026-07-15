//! 本文件实现 access-agent 控制面 HTTP 客户端。
//! 本模块封装心跳、配置结果和流量上报请求，让运行时只关注本地
//! 本地 Xray 状态，不把 HTTP 细节散落到主循环中。

use std::time::Duration;

use reqwest::{Client as HttpClient, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use xrayc_xray_config::AccessConfig;

use crate::config::RuntimeCore;

#[derive(Debug, Error)]
pub enum AgentClientError {
    #[error("control plane request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("control plane returned status {status}: {body}")]
    Status { status: StatusCode, body: String },
}

#[derive(Debug, Clone)]
pub struct AgentClient {
    base_url: String,
    token: String,
    http: HttpClient,
}

impl AgentClient {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, reqwest::Error> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()?;

        Ok(Self {
            base_url: trim_base_url(base_url.into()),
            token: token.into(),
            http,
        })
    }

    pub async fn heartbeat(
        &self,
        request: &HeartbeatRequest,
    ) -> Result<HeartbeatResponse, AgentClientError> {
        self.post_json("/api/agent/access/heartbeat", request).await
    }

    pub async fn report_traffic(
        &self,
        request: &TrafficReportRequest,
    ) -> Result<TrafficReportResponse, AgentClientError> {
        self.post_json("/api/agent/access/traffic", request).await
    }

    pub async fn report_metrics(
        &self,
        request: &MetricsReportRequest,
    ) -> Result<MetricsReportResponse, AgentClientError> {
        self.post_json("/api/agent/access/metrics", request).await
    }

    pub async fn report_sessions(
        &self,
        request: &SessionsReportRequest,
    ) -> Result<SessionsReportResponse, AgentClientError> {
        self.post_json("/api/agent/access/sessions", request).await
    }

    pub async fn report_probes(
        &self,
        request: &ProbesReportRequest,
    ) -> Result<ProbesReportResponse, AgentClientError> {
        self.post_json("/api/agent/access/probes", request).await
    }

    pub async fn report_config_result(
        &self,
        request: &ConfigResultRequest,
    ) -> Result<ConfigResultResponse, AgentClientError> {
        self.post_json("/api/agent/access/config-result", request)
            .await
    }

    async fn post_json<T, R>(&self, path: &str, body: &T) -> Result<R, AgentClientError>
    where
        T: Serialize + ?Sized,
        R: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body_len = response
                .text()
                .await
                .map(|body| body.len())
                .unwrap_or_default();
            let body = format!("redacted response body ({body_len} bytes)");
            return Err(AgentClientError::Status { status, body });
        }

        Ok(response.json::<R>().await?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub agent_version: String,
    pub hostname: String,
    #[serde(default)]
    pub core_type: RuntimeCore,
    pub xray_version: Option<String>,
    pub applied_config_version: Option<String>,
    pub uptime_seconds: u64,
    #[serde(default)]
    pub tls_certificates: Vec<TlsCertificateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_renew_result: Option<TlsRenewResult>,
    #[serde(default)]
    pub runtime_core_results: Vec<RuntimeCoreControlResult>,
    // 内核能力软信号(§7.7.1):connmark 是否可用、是否已装新内核待重启。
    // Option 缺省即旧 agent 不上报,控制面按可用/无待重启兜底,向后兼容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_connmark_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_upgrade_pending: Option<bool>,
    // 重启请求执行结果:agent 上轮收到 reboot 命令、做完安全自检后回报,
    // 控制面据 request_id 清理待执行重启请求。无结果时不发。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reboot_result: Option<RebootResult>,
    // 监控中心宿主指标软信号(阶段B):本心跳采集到的 CPU/内存/磁盘用量。
    // Option 缺省即旧 agent 不上报或本轮采集失败(软失败),控制面按缺省兜底,向后兼容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_metrics: Option<NodeMetricsReport>,
}

/// 监控中心宿主指标上报体(阶段B):一次心跳采集到的整机资源用量快照。
/// CPU 为占用千分比(0..=100000),内存/磁盘 used/total 为字节,collected_at_unix 为采集时刻。
/// 任一采集子项失败时 agent 整体不上报本结构(软失败),不构造假数据。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct NodeMetricsReport {
    pub cpu_pct_milli: u32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub collected_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct HeartbeatResponse {
    pub accepted: bool,
    #[serde(default)]
    pub core_type: RuntimeCore,
    pub desired_config_version: Option<String>,
    #[serde(default)]
    pub config_status: ConfigStatus,
    pub config: Option<AccessConfig>,
    #[serde(default)]
    pub probe_tasks: Vec<ProbeTask>,
    #[serde(default)]
    pub tls_renew_task: Option<TlsRenewTask>,
    #[serde(default)]
    pub runtime_core_tasks: Vec<RuntimeCoreTask>,
    // 节点多域名清单:控制面在心跳里下发,agent 据此遍历逐域名签证书。
    // 缺省为空(向后兼容旧单域名心跳),为空时 agent 沿用 env 单域名路径。
    #[serde(default)]
    pub node_domains: Vec<NodeDomain>,
    // 管理员从面板触发的整机重启请求:仅有 request_id,agent 收到后先做安全自检再 reboot。
    // 缺省 None 即无待执行重启;绝不无命令自动重启(§7.7.1)。
    #[serde(default)]
    pub reboot_task: Option<RebootTask>,
}

/// 控制面下发的整机重启请求(只带 request_id,不含任何凭据/SSH)。
/// agent 收到后必须先做安全自检(GRUB 默认引导最高内核、容器与 docker 开机自启)
/// 才 reboot 宿主;自检不过只回报失败、不重启。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RebootTask {
    pub request_id: String,
}

/// 重启请求执行结果:status∈{success,running,failed},success 表示已通过自检并已下达 reboot。
/// message 为脱敏短摘要(自检未过的原因等),不回传宿主明文细节。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RebootResult {
    pub request_id: String,
    pub status: String,
    pub message: String,
}

/// agent 侧自定义的节点域名形状(不依赖 db,JSON 字段与控制面心跳对齐)。
/// 每项描述一个对外域名及其证书签发方式:direct→HTTP-01、cf→DNS-01。
/// cf_cert_mode/acme_email 可选,缺省时由节点级回退值补全(向后兼容单域名)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct NodeDomain {
    pub domain: String,
    pub kind: NodeDomainKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cf_cert_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acme_email: Option<String>,
}

/// 域名证书签发模式:direct 为灰云直连(HTTP-01),cf 为橙云回源(DNS-01)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeDomainKind {
    Direct,
    Cf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TlsCertificateReport {
    pub domain: String,
    pub status: String,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub days_remaining: Option<i64>,
    pub error_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TlsRenewTask {
    pub request_id: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TlsRenewResult {
    pub request_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoreAction {
    Start,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeCoreTask {
    pub request_id: String,
    pub core_type: RuntimeCore,
    pub action: RuntimeCoreAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeCoreControlResult {
    pub request_id: String,
    pub core_type: RuntimeCore,
    pub action: RuntimeCoreAction,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ConfigStatus {
    pub required: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProbeTask {
    pub exit_endpoint_id: String,
    pub requested_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ProbeTaskTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProbeTaskTarget {
    pub protocol: String,
    pub address: String,
    pub port: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TrafficReportRequest {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_config_version: Option<String>,
    pub snapshots: Vec<TrafficSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TrafficSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_line_id: Option<String>,
    pub xray_user_key: String,
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
    pub captured_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TrafficReportResponse {
    pub accepted: bool,
    #[serde(default)]
    pub core_type: RuntimeCore,
    pub desired_config_version: Option<String>,
    #[serde(default)]
    pub config_status: ConfigStatus,
    pub config: Option<AccessConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MetricsReportRequest {
    pub access_node_id: String,
    pub metrics: Vec<AccessLineMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessLineMetric {
    pub access_node_id: String,
    pub access_line_id: String,
    pub online_users: u64,
    pub active_connections: u64,
    pub unique_client_ips: u64,
    pub uplink_rate_bps: u64,
    pub downlink_rate_bps: u64,
    pub collected_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MetricsReportResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SessionsReportRequest {
    pub access_node_id: String,
    pub sessions: Vec<AccessUserSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessUserSession {
    pub access_line_id: String,
    pub xray_user_key: String,
    #[serde(default)]
    pub client_ip: String,
    pub client_ip_hash: String,
    pub active_connection_count: u64,
    pub started_at_unix: i64,
    pub last_seen_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SessionsReportResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProbesReportRequest {
    pub access_node_id: String,
    pub line_probes: Vec<AccessLineProbe>,
    pub exit_probes: Vec<AccessExitProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessLineProbe {
    pub access_line_id: String,
    pub status: String,
    pub latency_ms: Option<u64>,
    pub error_summary: String,
    pub probed_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessExitProbe {
    pub exit_endpoint_id: String,
    pub status: String,
    pub latency_ms: Option<u64>,
    pub error_summary: String,
    pub probed_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProbesReportResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ConfigResultRequest {
    pub node_id: String,
    pub config_version: String,
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ConfigResultResponse {
    pub accepted: bool,
}

fn trim_base_url(mut base_url: String) -> String {
    while base_url.ends_with('/') {
        base_url.pop();
    }
    base_url
}
