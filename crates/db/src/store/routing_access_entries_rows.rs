//! 入口写入主路径用到的两个纯数据行结构体定义。
//! 从 routing_access_entries 拆出,避免主写入文件超过 550 行硬上限。
//! 本模块只承载结构体定义,不含任何逻辑函数与 SQL 访问。
//! PreparedAccessEntry 是 prepare_access_entry 物化后的入口参数快照。
//! StoredAccessEntry 是从 access_entries 读回的行(sqlx::FromRow 映射)。
//! 两者均为 pub(crate),由 routing_access_entries 以 use 引回再对外暴露。
//! 引回口径不变,其它模块仍按 routing_access_entries:: 路径引用这两个结构。
//! 仅依赖 serde_json::Value、uuid::Uuid 与 sqlx 派生,无业务副作用。
//! 不读写远端服务器,不处理前端展示,不触碰出口敏感字段。
//! 本头部满足前十行中文注释约束。

use serde_json::Value;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct PreparedAccessEntry {
    pub(crate) listen_host: String,
    pub(crate) protocol: &'static str,
    pub(crate) transport: &'static str,
    pub(crate) security: String,
    pub(crate) server_name: String,
    pub(crate) public_key: String,
    pub(crate) short_id: String,
    pub(crate) flow: String,
    pub(crate) udp_enabled: bool,
    pub(crate) udp_packet_encoding: String,
    pub(crate) ws_path: String,
    pub(crate) ws_host: String,
    pub(crate) xhttp_path: String,
    pub(crate) xhttp_host: String,
    pub(crate) xhttp_mode: &'static str,
    pub(crate) inbound_config: Value,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct StoredAccessEntry {
    pub(crate) access_node_id: Uuid,
    pub(crate) name: String,
    pub(crate) listen_host: String,
    pub(crate) listen_port: i32,
    pub(crate) protocol: String,
    pub(crate) transport: String,
    pub(crate) user_uuid: String,
    pub(crate) server_name: String,
    pub(crate) public_key: String,
    pub(crate) short_id: String,
    pub(crate) flow: String,
    pub(crate) udp_enabled: bool,
    pub(crate) udp_packet_encoding: String,
    pub(crate) xhttp_path: String,
    pub(crate) xhttp_host: String,
    pub(crate) xhttp_mode: String,
    pub(crate) inbound_config: Value,
    pub(crate) enabled: bool,
    pub(crate) sort_weight: i32,
}
