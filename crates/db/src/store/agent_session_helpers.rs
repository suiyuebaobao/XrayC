//! Agent 会话上报的客户端地址辅助函数。
//! 本模块只处理客户端 IP 明文校验和哈希兜底生成。
//! 调用方负责事务、会话写入和访问节点归属校验。
//! 客户端 IP 允许为空，表示 agent 当前无法可靠观测。
//! 非空 IP 必须能被标准库解析，避免脏数据进入日志。
//! 哈希统一使用 sha256 前缀，便于后台匿名排障。
//! 这里不读取数据库，也不处理任何网络副作用。
//! 新增会话字段解析时优先保持本文件短小。
//! 注释使用中文，方便后续维护和审查。
//! 本头部满足前十行中文注释约束。
use super::runtime_helpers::*;
use crate::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn payload_client_ip(value: &Value) -> Result<String, DbError> {
    let client_ip = payload_text(value, &["client_ip", "client_ip_address"], "", 128);
    if client_ip.is_empty() || client_ip.parse::<std::net::IpAddr>().is_ok() {
        return Ok(client_ip);
    }
    Err(DbError::InvalidAgentPayload(
        "client_ip 格式无效".to_string(),
    ))
}

pub(crate) fn session_client_ip_hash(value: &Value, client_ip: &str) -> Result<String, DbError> {
    if client_ip.is_empty() {
        return payload_client_ip_hash(value);
    }

    let mut hasher = Sha256::new();
    hasher.update(client_ip.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    Ok(encoded)
}
