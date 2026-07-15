//! Shadowsocks 密钥校验工具。
//! 本模块只处理 SS 2022 方法的 base64 根密钥长度规则。
//! 普通 aes-*-gcm 等旧方法不进入这里校验，避免误伤历史配置。
//! Xray 要求 2022-blake3-aes-128-gcm 使用 16 字节密钥。
//! Xray 要求 2022-blake3-aes-256-gcm 和 chacha20-poly1305 使用 32 字节密钥。
//! 校验在写库前执行，避免 access-agent 到远端后才被 Xray 拒绝。
//! 错误信息只描述字段和长度，不回显真实密码。
//! 该模块不访问数据库、不记录日志、不保存敏感信息。
//! 新增 Shadowsocks 方法时优先扩展 expected_key_len。
//! 本头部满足前十行中文注释约束。

use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;

use crate::{config_text, DbError};

pub(crate) fn validate_shadowsocks_endpoint_config(config: &Value) -> Result<(), DbError> {
    let method = require_text(config, &["method", "cipher"], "Shadowsocks method")?;
    let password = require_text(config, &["password"], "Shadowsocks password")?;
    validate_shadowsocks_2022_key(&method, &password, "Shadowsocks password")
}

pub(crate) fn validate_shadowsocks_inbound_config(config: &Value) -> Result<(), DbError> {
    let method =
        config_text(config, &["method", "cipher"]).unwrap_or_else(|| "aes-256-gcm".to_string());
    if !method.trim().starts_with("2022-") {
        return Ok(());
    }
    let password = config_text(
        config,
        &[
            "password",
            "server_password",
            "serverPassword",
            "root_password",
        ],
    )
    .ok_or_else(|| {
        DbError::InvalidAgentPayload(
            "Shadowsocks 2022 接入必须显式配置 server_password/password".to_string(),
        )
    })?;
    validate_shadowsocks_2022_key(&method, &password, "Shadowsocks 2022 接入密码")
}

pub(crate) fn validate_shadowsocks_2022_key(
    method: &str,
    password: &str,
    label: &str,
) -> Result<(), DbError> {
    let Some(expected_len) = expected_key_len(method) else {
        return Ok(());
    };
    let decoded = general_purpose::STANDARD
        .decode(password.trim())
        .map_err(|_| invalid_key(label, expected_len))?;
    if decoded.len() != expected_len {
        return Err(invalid_key(label, expected_len));
    }
    Ok(())
}

fn expected_key_len(method: &str) -> Option<usize> {
    let method = method.trim().to_ascii_lowercase();
    match method.as_str() {
        "2022-blake3-aes-128-gcm" => Some(16),
        "2022-blake3-aes-256-gcm" | "2022-blake3-chacha20-poly1305" => Some(32),
        _ => None,
    }
}

fn invalid_key(label: &str, expected_len: usize) -> DbError {
    DbError::InvalidAgentPayload(format!(
        "{label} 必须是 base64，解码后长度为 {expected_len} 字节"
    ))
}

fn require_text(config: &Value, keys: &[&str], field_label: &str) -> Result<String, DbError> {
    config_text(config, keys)
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("{field_label} 不能为空")))
}
