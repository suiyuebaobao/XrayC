//! VLESS Reality 中转入口配置辅助函数。
//! 本文件从 routing_entries 拆出纯配置校验和默认值生成。
//! 不包含数据库事务、SQL 写入或 dirty 标记逻辑。
//! 入口行创建仍由 routing_entries 负责。
//! Reality key 与 short id 只在本模块按需生成。
//! 对外只暴露同级模块需要调用的 helper。
//! 新增安全模式时应优先扩展这里的分支校验。
//! 错误消息保持中文，供后台表单直接展示。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 本头部满足前十行中文注释约束。

use super::routing_entries::EntryNetworkMode;
use crate::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::aead::OsRng;
use serde_json::{json, Value};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

const DEFAULT_REALITY_SERVER_NAME: &str = "www.cloudflare.com";

pub(super) fn server_name_for_entry(
    protocol: &str,
    listen_host: &str,
    inbound_config: &serde_json::Value,
) -> String {
    config_text(
        inbound_config,
        &["server_name", "serverName", "sni", "tls_server_name"],
    )
    .unwrap_or_else(|| {
        let security = vless_entry_security(protocol, inbound_config);
        match protocol {
            "trojan" | "hysteria" => listen_host.to_owned(),
            "vless" if security == "tls" => listen_host.to_owned(),
            "vless" if security == "reality" => default_reality_server_name(inbound_config),
            _ => String::new(),
        }
    })
}

fn default_reality_server_name(inbound_config: &serde_json::Value) -> String {
    config_text(inbound_config, &["dest", "reality_dest", "realityDest"])
        .and_then(|dest| reality_dest_host(&dest))
        .unwrap_or_else(|| DEFAULT_REALITY_SERVER_NAME.to_string())
}

fn reality_dest_host(dest: &str) -> Option<String> {
    let trimmed = dest.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(trimmed);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(without_scheme)
        .trim();

    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(value, _)| value).unwrap_or(rest)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.parse::<u16>().is_ok() {
            host
        } else {
            authority
        }
    } else {
        authority
    }
    .trim();

    if host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    Some(host.to_string())
}

pub(super) fn validate_vless_reality_entry_config(
    protocol: &str,
    inbound_config: &serde_json::Value,
) -> Result<(), DbError> {
    if protocol != "vless" {
        return Ok(());
    }

    match vless_entry_security(protocol, inbound_config).as_str() {
        "" | "tls" => Ok(()),
        "reality" => {
            require_entry_config_text(
                inbound_config,
                &[
                    "public_key",
                    "publicKey",
                    "reality_public_key",
                    "realityPublicKey",
                ],
                "VLESS Reality public_key",
                255,
            )?;
            require_entry_config_text(
                inbound_config,
                &[
                    "private_key",
                    "privateKey",
                    "reality_private_key",
                    "realityPrivateKey",
                ],
                "VLESS Reality private_key",
                255,
            )?;
            Ok(())
        }
        other => Err(DbError::InvalidAgentPayload(format!(
            "VLESS 入站 security 仅支持 tls 或 reality，当前为 {other}"
        ))),
    }
}

pub(super) fn fill_vless_reality_entry_config(
    protocol: &str,
    server_name: &str,
    inbound_config: Value,
) -> Result<Value, DbError> {
    if protocol != "vless" || vless_entry_security(protocol, &inbound_config) != "reality" {
        return Ok(inbound_config);
    }
    let mut object = inbound_config.as_object().cloned().ok_or_else(|| {
        DbError::InvalidAgentPayload("VLESS Reality 入站配置必须是 JSON 对象".to_string())
    })?;

    let current_config = Value::Object(object.clone());
    if config_text(
        &current_config,
        &["server_name", "serverName", "sni", "tls_server_name"],
    )
    .is_none()
        && !server_name.trim().is_empty()
    {
        object.insert("server_name".to_string(), json!(server_name.trim()));
    }

    let current_config = Value::Object(object.clone());
    if config_text(&current_config, &["dest", "reality_dest", "realityDest"]).is_none() {
        let dest = if server_name.trim().is_empty() {
            "www.cloudflare.com:443".to_string()
        } else {
            format!("{}:443", server_name.trim())
        };
        object.insert("dest".to_string(), json!(dest));
    }

    let current_config = Value::Object(object.clone());
    if config_text(&current_config, &["fingerprint", "fp"]).is_none() {
        object.insert("fingerprint".to_string(), json!("chrome"));
    }

    let current_config = Value::Object(object.clone());
    if config_text(
        &current_config,
        &["public_key", "publicKey", "reality_public_key"],
    )
    .is_none()
        || config_text(
            &current_config,
            &[
                "private_key",
                "privateKey",
                "reality_private_key",
                "realityPrivateKey",
            ],
        )
        .is_none()
    {
        let (public_key, private_key) = generate_reality_entry_key_pair();
        object.insert("public_key".to_string(), json!(public_key));
        object.insert("private_key".to_string(), json!(private_key));
    }

    let current_config = Value::Object(object.clone());
    if config_text(
        &current_config,
        &["short_id", "shortId", "reality_short_id", "realityShortId"],
    )
    .is_none()
    {
        object.insert(
            "short_id".to_string(),
            json!(random_reality_entry_short_id()),
        );
    }

    Ok(Value::Object(object))
}

/// 给非 Reality 的 VLESS 入口填充「量子加密(VLESS native encryption,后量子)」密钥串。
/// 口径(详见 docs/superpowers/specs/2026-06-28-vless-quantum-encryption-and-sni-merge-design.md):
/// inbound_config 里 `vless_quantum_encryption=true` 且 protocol=vless 且 security≠reality 时,
/// 生成 x25519 对(密钥交换恒为 mlkem768x25519 后量子混合,协议固定)、拼成:
///   服务端 vless_decryption = mlkem768x25519plus.native.600s.<x25519私钥 b64url>(含私钥,绝不入订阅)
///   客户端 vless_encryption = mlkem768x25519plus.native.0rtt.<x25519公钥 b64url>(公钥,进订阅)
/// 不适用(Reality / 关开关 / 非 vless)时清空两串与标记,避免残留。Reality 走后量子 dest、不在此叠。
/// 复用 generate_reality_entry_key_pair():vlessenc 的 X25519 认证档与 Reality 同款 x25519(xray 两者共用)。
pub(super) fn fill_vless_quantum_entry_config(
    protocol: &str,
    inbound_config: Value,
) -> Result<Value, DbError> {
    let Some(mut object) = inbound_config.as_object().cloned() else {
        return Ok(inbound_config);
    };
    let security = vless_entry_security(protocol, &Value::Object(object.clone()));
    let quantum_requested = object
        .get("vless_quantum_encryption")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let applicable = protocol == "vless" && security != "reality" && quantum_requested;

    if !applicable {
        object.remove("vless_decryption");
        object.remove("vless_encryption");
        object.insert("vless_quantum_encryption".to_string(), json!(false));
        return Ok(Value::Object(object));
    }

    // 适用:仅当还没生成过(decryption 空)才生成新对,编辑保持开启时不换密钥、不踢现有客户端。
    let current = Value::Object(object.clone());
    if config_text(&current, &["vless_decryption"]).is_none() {
        let (public_key, private_key) = generate_reality_entry_key_pair();
        object.insert(
            "vless_decryption".to_string(),
            json!(format!("mlkem768x25519plus.native.600s.{private_key}")),
        );
        object.insert(
            "vless_encryption".to_string(),
            json!(format!("mlkem768x25519plus.native.0rtt.{public_key}")),
        );
    }
    object.insert("vless_quantum_encryption".to_string(), json!(true));
    Ok(Value::Object(object))
}

pub(super) fn validate_vless_reality_entry_network_mode(
    protocol: &str,
    inbound_config: &serde_json::Value,
    mode: &EntryNetworkMode,
) -> Result<(), DbError> {
    if protocol != "vless" || vless_entry_security(protocol, inbound_config) != "reality" {
        return Ok(());
    }
    if mode.udp_enabled || mode.udp_packet_encoding.eq_ignore_ascii_case("xudp") {
        return Err(DbError::InvalidAgentPayload(
            "VLESS Reality 入口仅支持 TCP、XHTTP 或 gRPC 网络模式".to_string(),
        ));
    }
    if !matches!(mode.transport, "tcp" | "xhttp" | "grpc") {
        return Err(DbError::InvalidAgentPayload(
            "VLESS Reality 入口仅支持 TCP、XHTTP 或 gRPC 网络模式".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn vless_reality_entry_public_fields(
    protocol: &str,
    inbound_config: &serde_json::Value,
) -> Result<(String, String), DbError> {
    if protocol != "vless" || vless_entry_security(protocol, inbound_config) != "reality" {
        return Ok((String::new(), String::new()));
    }

    let public_key = require_entry_config_text(
        inbound_config,
        &[
            "public_key",
            "publicKey",
            "reality_public_key",
            "realityPublicKey",
        ],
        "VLESS Reality public_key",
        255,
    )?;
    let short_id = config_text(
        inbound_config,
        &["short_id", "shortId", "reality_short_id", "realityShortId"],
    )
    .map(|value| optional_admin_text(&value, 64))
    .unwrap_or_default();
    Ok((public_key, short_id))
}

fn generate_reality_entry_key_pair() -> (String, String) {
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);
    (
        URL_SAFE_NO_PAD.encode(public_key.as_bytes()),
        URL_SAFE_NO_PAD.encode(private_key.to_bytes()),
    )
}

fn random_reality_entry_short_id() -> String {
    let value = Uuid::new_v4().simple().to_string();
    value[..16].to_string()
}

fn vless_entry_security(protocol: &str, inbound_config: &serde_json::Value) -> String {
    if protocol != "vless" {
        return String::new();
    }
    config_text(inbound_config, &["security"])
        .map(|security| security.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

fn require_entry_config_text(
    inbound_config: &serde_json::Value,
    keys: &[&str],
    label: &str,
    max_len: usize,
) -> Result<String, DbError> {
    let value = config_text(inbound_config, keys)
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("{label} 不能为空")))?;
    required_admin_text(&value, label, max_len)
}
