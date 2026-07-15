//! 数据库查询行模型 topology。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use crate::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub(crate) struct AccessNodeRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) public_host: String,
    pub(crate) public_port: i32,
    pub(crate) ssh_host: String,
    pub(crate) remark: String,
    pub(crate) agent_token_hash: String,
    pub(crate) status: String,
    pub(crate) agent_version: String,
    pub(crate) config_dirty: bool,
    pub(crate) desired_config_hash: Option<String>,
    pub(crate) applied_config_hash: Option<String>,
    pub(crate) last_heartbeat_at: Option<DateTime<Utc>>,
    pub(crate) last_traffic_report_at: Option<DateTime<Utc>>,
    pub(crate) last_traffic_success_at: Option<DateTime<Utc>>,
    pub(crate) config_dirty_at: Option<DateTime<Utc>>,
    pub(crate) config_dirty_reason: String,
    pub(crate) tls_certificates: Value,
    pub(crate) tls_cert_last_report_at: Option<DateTime<Utc>>,
    pub(crate) tls_renew_request_id: Option<Uuid>,
    pub(crate) tls_renew_requested_at: Option<DateTime<Utc>>,
    pub(crate) tls_renew_completed_at: Option<DateTime<Utc>>,
    pub(crate) tls_renew_status: String,
    pub(crate) tls_renew_message: String,
    // 多模式 CF 字段(nullable 列用 Option,cf_enabled/cf_cert_mode 有 NOT NULL 默认)。
    pub(crate) cert_domain: Option<String>,
    pub(crate) acme_email: Option<String>,
    pub(crate) cf_enabled: bool,
    pub(crate) cf_domain: Option<String>,
    pub(crate) cf_cert_mode: String,
    // IP 直连地址(节点公网 IP),给 Reality/Shadowsocks 直连用;可空。
    pub(crate) ip_direct_address: Option<String>,
    // 内核能力软状态(§7.7.1):connmark 可加载性 + 是否已装新内核待重启(NOT NULL 默认列)。
    pub(crate) kernel_connmark_available: bool,
    pub(crate) kernel_upgrade_pending: bool,
    // 整机重启请求软状态:reboot_status/message 有 NOT NULL 默认;请求时间戳/ID 可空。
    pub(crate) reboot_status: String,
    pub(crate) reboot_message: String,
}

/// 多域名读模型行:供 load_store_data 把 node_domains 回填到各节点 domains[]。
/// 只取读侧需要回显的列(id/domain/kind/is_primary/cert_status),不含写侧内部派生项。
#[derive(Debug, FromRow)]
pub(crate) struct NodeDomainViewRow {
    pub(crate) access_node_id: Uuid,
    pub(crate) id: Uuid,
    pub(crate) domain: String,
    pub(crate) kind: String,
    pub(crate) is_primary: bool,
    pub(crate) cert_status: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct AccessNodeCfFieldsRow {
    pub(crate) cert_domain: Option<String>,
    pub(crate) acme_email: Option<String>,
    // cf_enabled 不再从 access_nodes 列读取,改由 node_has_domain_kind 派生(见 routing_read.rs)。
    pub(crate) cf_domain: Option<String>,
    pub(crate) cf_cert_mode: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct AccessNodeSummaryRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) config_dirty: bool,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AuditLogListRow {
    pub(crate) id: Uuid,
    pub(crate) actor_email: String,
    pub(crate) action: String,
    pub(crate) resource_type: String,
    pub(crate) resource_id: Option<Uuid>,
    pub(crate) request_summary: Value,
    pub(crate) result: String,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct ExitPoolRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) region_code: String,
    pub(crate) strategy: String,
    pub(crate) enabled: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct ExitEndpointRow {
    pub(crate) exit_pool_id: Uuid,
    pub(crate) id: Uuid,
    pub(crate) resource_name: String,
    pub(crate) ownership: String,
    pub(crate) owner_access_node_id: Option<Uuid>,
    pub(crate) outbound_type: String,
    pub(crate) host: String,
    pub(crate) port: i32,
    pub(crate) outbound_config: Value,
    pub(crate) stream_config: Value,
    pub(crate) weight: i32,
    pub(crate) priority: i32,
    pub(crate) allow_new_assignments: bool,
    pub(crate) status: String,
    pub(crate) healthy: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct LocalExitEndpointRow {
    pub(crate) id: Uuid,
    pub(crate) resource_name: String,
    pub(crate) ownership: String,
    pub(crate) owner_access_node_id: Option<Uuid>,
    pub(crate) outbound_type: String,
    pub(crate) host: String,
    pub(crate) port: i32,
    pub(crate) outbound_config: Value,
    pub(crate) stream_config: Value,
    pub(crate) status: String,
    pub(crate) healthy: bool,
}

#[derive(Debug, FromRow)]
pub(crate) struct ExitEndpointUpdateRow {
    pub(crate) exit_resource_id: Uuid,
    pub(crate) outbound_type: String,
    pub(crate) host: String,
    pub(crate) port: i32,
    pub(crate) outbound_config: Value,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminExitResourceListRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) region_code: String,
    pub(crate) provider_name: String,
    pub(crate) ownership: String,
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) last_probe_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_status: String,
    pub(crate) access_node_id: Option<Uuid>,
    pub(crate) access_node_name: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AdminExitEndpointListRow {
    pub(crate) id: Uuid,
    pub(crate) exit_resource_id: Uuid,
    pub(crate) exit_resource_name: String,
    pub(crate) name: String,
    pub(crate) outbound_type: String,
    pub(crate) host: String,
    pub(crate) port: i32,
    pub(crate) outbound_config: Value,
    pub(crate) stream_config: Value,
    pub(crate) probe_config: Value,
    pub(crate) enabled: bool,
    pub(crate) exit_resource_enabled: bool,
    pub(crate) last_probe_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_status: String,
    pub(crate) created_at: DateTime<Utc>,
    // 多域名读模型:出口选中域名引用与 LEFT JOIN 解析出的 domain/kind(未选为 None)。
    pub(crate) node_domain_id: Option<Uuid>,
    pub(crate) node_domain: Option<String>,
    pub(crate) node_domain_kind: Option<String>,
}
