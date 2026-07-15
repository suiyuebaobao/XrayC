//! 本模块定义 Xray 配置编译器的输入数据结构。
//! 这些结构承载控制面下发的入口、出口和路由描述。
//! 序列化名称保持原有约定，避免影响现有配置格式。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessConfig {
    pub node_id: String,
    pub log_level: LogLevel,
    pub stats_enabled: bool,
    pub access_lines: Vec<AccessLine>,
    #[serde(default)]
    pub local_exit_services: Vec<LocalExitService>,
    pub exit_endpoints: Vec<ExitEndpoint>,
    pub routing_rules: Vec<RoutingRule>,
    #[serde(default)]
    pub rate_limits: Vec<UserRateLimit>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessLine {
    pub id: String,
    #[serde(default)]
    pub source_line_id: String,
    #[serde(default = "default_runtime_core")]
    pub runtime_core: String,
    pub listen_host: String,
    pub listen_port: u16,
    #[serde(default = "default_tcp_transport")]
    pub transport: String,
    #[serde(default)]
    pub xhttp_path: String,
    #[serde(default)]
    pub xhttp_host: String,
    #[serde(default = "default_xhttp_mode")]
    pub xhttp_mode: String,
    #[serde(default)]
    pub inbound_security: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub reality_dest: Option<String>,
    #[serde(default)]
    pub reality_private_key: Option<String>,
    #[serde(default)]
    pub reality_short_ids: Vec<String>,
    #[serde(default)]
    pub tls_certificate_file: Option<String>,
    #[serde(default)]
    pub tls_key_file: Option<String>,
    pub protocol: AccessProtocol,
    pub users: Vec<AccessUser>,
    pub default_exit_tag: String,
}

impl AccessLine {
    pub fn report_line_id(&self) -> &str {
        let source_line_id = self.source_line_id.trim();
        if source_line_id.is_empty() {
            &self.id
        } else {
            source_line_id
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AccessProtocol {
    Vless {
        flow: Option<String>,
        decryption: Option<String>,
    },
    Trojan,
    Hysteria2,
    Shadowsocks {
        method: String,
        server_password: String,
        network: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AccessUser {
    pub xray_user_key: String,
    pub credential: String,
    pub email: String,
    pub level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct LocalExitService {
    pub id: String,
    pub tag: String,
    pub listen_port: u16,
    pub protocol: LocalExitProtocol,
    #[serde(default)]
    pub network: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub security: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub reality_dest: Option<String>,
    #[serde(default)]
    pub reality_private_key: Option<String>,
    #[serde(default)]
    pub reality_short_ids: Vec<String>,
    #[serde(default)]
    pub tls_certificate_file: Option<String>,
    #[serde(default)]
    pub tls_key_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocalExitProtocol {
    Socks,
    Http,
    Vless,
    Trojan,
    Shadowsocks,
    Hysteria2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ExitEndpoint {
    pub id: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sockopt_mark: Option<u32>,
    pub protocol: ExitProtocol,
}

// The serialized shape is the control-plane contract. VLESS carries many
// protocol/security/transport fields, and boxing the variant would make this
// externally shaped enum harder to read without measurable runtime benefit.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExitProtocol {
    Direct,
    Socks {
        address: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
    Http {
        address: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
    Vless {
        address: String,
        port: u16,
        uuid: String,
        security: Option<String>,
        flow: Option<String>,
        server_name: Option<String>,
        public_key: Option<String>,
        short_id: Option<String>,
        fingerprint: Option<String>,
        network: Option<String>,
        xhttp_path: Option<String>,
        xhttp_host: Option<String>,
        xhttp_mode: Option<String>,
        udp_packet_encoding: Option<String>,
    },
    Trojan {
        address: String,
        port: u16,
        password: String,
        security: Option<String>,
        server_name: Option<String>,
    },
    Shadowsocks {
        address: String,
        port: u16,
        method: String,
        password: String,
    },
    Hysteria2 {
        address: String,
        port: u16,
        password: String,
        server_name: Option<String>,
        allow_insecure: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RoutingRule {
    pub inbound_tag: Option<String>,
    pub user_email: Option<String>,
    pub outbound_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct UserRateLimit {
    pub user_id: String,
    pub xray_user_key: String,
    /// 是否限速的门控值（= max(上行, 下行)，>0 即该用户需要分配 fwmark 与限速类）。向后兼容旧的对称口径。
    pub rate_limit_bps: u64,
    /// 上行（用户上传＝节点→出口 egress）限速 bps，0 表示该方向不限（回退 root）。
    /// agent 按该用户 fwmark 喂物理网卡 egress 上行类 1:N（出口侧整形，不再按端口）。
    #[serde(default)]
    pub rate_limit_up_bps: u64,
    /// 下行（用户下载＝出口→节点 ingress）限速 bps，0 表示该方向不限（回退 root）。
    /// agent 用 act_connmark 还原 fwmark 后喂 ifb 下行类 2:N。
    #[serde(default)]
    pub rate_limit_down_bps: u64,
    pub mark: u32,
    pub class_id: u16,
}

fn default_tcp_transport() -> String {
    "tcp".to_owned()
}

fn default_runtime_core() -> String {
    "xray".to_owned()
}

fn default_xhttp_mode() -> String {
    "stream-one".to_owned()
}
