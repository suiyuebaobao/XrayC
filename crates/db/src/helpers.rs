//! 本模块收纳数据库层的纯辅助函数。
//! 这里的函数不访问数据库，也不改变业务状态。
//! 主要职责包括筛选条件归一化和错误摘要截断。
//! 也包含 Agent JSON payload 中基础字段的读取。
//! 这些读取函数只做类型转换、默认值和长度限制。
//! 客户端地址摘要工具只生成稳定哈希，不保存明文地址。
//! 字节换算工具用于管理端和用户端读模型展示。
//! 模块保持小而稳定，避免持久化主文件继续膨胀。
//! 新增辅助逻辑应优先放在这里或更具体的校验模块中。
//! 本模块不得引入 SQL 或 PgStore 业务流程。

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{validation::is_valid_client_ip_hash, DbError};

pub(crate) fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn normalize_udp_packet_encoding(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "raw" | "none" => Ok(""),
        "xudp" => Ok("xudp"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的 UDP 封包模式: {other}"
        ))),
    }
}

pub(crate) fn truncate_error_summary(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn payload_items_with_root_field<'a>(
    payload: &'a Value,
    keys: &[&str],
    root_field: &str,
) -> Vec<&'a Value> {
    for key in keys {
        if let Some(items) = payload.get(*key).and_then(Value::as_array) {
            return items.iter().collect();
        }
    }
    if payload.get(root_field).is_some() {
        return vec![payload];
    }
    Vec::new()
}

pub(crate) fn payload_uuid(value: &Value, field: &str) -> Result<Uuid, DbError> {
    let raw = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("缺少 {field}")))?;
    Uuid::parse_str(raw).map_err(|_| DbError::InvalidAgentPayload(format!("{field} 格式无效")))
}

pub(crate) fn payload_required_text(value: &Value, field: &str) -> Result<String, DbError> {
    let raw = value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if raw.is_empty() {
        return Err(DbError::InvalidAgentPayload(format!("缺少 {field}")));
    }
    Ok(raw.to_string())
}

pub(crate) fn payload_text(
    value: &Value,
    fields: &[&str],
    default: &str,
    max_chars: usize,
) -> String {
    let raw = fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
        .unwrap_or(default)
        .trim();
    raw.chars().take(max_chars).collect()
}

pub(crate) fn payload_client_ip_hash(value: &Value) -> Result<String, DbError> {
    let raw = payload_text(value, &["client_ip_hash", "client_ip_sha256"], "", 128);
    if is_valid_client_ip_hash(&raw) {
        return Ok(raw);
    }
    Err(DbError::InvalidAgentPayload(
        "client_ip_hash 必须是 sha256:<64位hex>".to_string(),
    ))
}

pub(crate) fn payload_session_status(
    value: &Value,
    _active_connection_count: i32,
) -> Result<String, DbError> {
    let raw = payload_text(value, &["status", "session_status"], "", 32)
        .trim()
        .to_ascii_lowercase();
    if raw.is_empty() {
        // 旧版 agent 还没有 active_connection_count 字段，当前快照内出现的会话仍按在线处理。
        return Ok("online".to_string());
    }
    match raw.as_str() {
        "online" | "offline" | "expired" | "unknown" => Ok(raw),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的会话状态: {other}"
        ))),
    }
}

pub(crate) fn payload_u64(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or_default()
}

pub(crate) fn payload_i32(value: &Value, field: &str) -> i32 {
    payload_optional_i32(value, field).unwrap_or_default()
}

pub(crate) fn payload_i32_any(value: &Value, fields: &[&str]) -> i32 {
    fields
        .iter()
        .find_map(|field| payload_optional_i32(value, field))
        .unwrap_or_default()
}

pub(crate) fn payload_optional_i32(value: &Value, field: &str) -> Option<i32> {
    let raw = value.get(field)?;
    let number = raw
        .as_i64()
        .or_else(|| raw.as_u64().and_then(|value| i64::try_from(value).ok()))?;
    Some(number.clamp(0, i64::from(i32::MAX)) as i32)
}

pub(crate) fn payload_time(
    value: &Value,
    unix_fields: &[&str],
    text_fields: &[&str],
) -> DateTime<Utc> {
    payload_optional_time(value, unix_fields, text_fields).unwrap_or_else(Utc::now)
}

pub(crate) fn payload_optional_time(
    value: &Value,
    unix_fields: &[&str],
    text_fields: &[&str],
) -> Option<DateTime<Utc>> {
    unix_fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_i64))
        .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
        .or_else(|| {
            text_fields
                .iter()
                .find_map(|field| value.get(*field).and_then(Value::as_str))
                .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                .map(|time| time.with_timezone(&Utc))
        })
}

pub fn bytes_to_gb(bytes: u64) -> f64 {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    ((bytes as f64 / GB) * 100.0).round() / 100.0
}

pub(crate) fn safe_client_ip_summary(value: &str) -> String {
    let value = value.trim();
    if is_valid_client_ip_hash(value) {
        return value.to_string();
    }
    format!(
        "sha256:{:x}",
        Sha256::digest(format!("xrayc-audit-client:v1:{value}").as_bytes())
    )
}
