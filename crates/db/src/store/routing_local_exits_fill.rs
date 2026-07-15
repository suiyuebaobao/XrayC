//! 本机出口 outbound_config 填充与协议字段校验辅助。
//! 从 routing_local_exits 拆出,避免主文件超过 550 行硬上限。
//! 职责:按协议把空/缺字段补成可启动的最小配置(自动生成凭据/密钥),并做最小运行前校验。
//! socks/http 缺 username/password 时随机补齐(否则节点渲染跳过本机服务);shadowsocks 缺密钥补 2022 系列随机串。
//! VLESS Reality 缺 security/server_name/dest/密钥对/short_id 时填默认或随机值。
//! Trojan/HY2 必须自带 TLS 证书文件与私钥文件,缺则拒绝(否则中转节点 Xray 启动失败);password/auth 缺失时自动补随机串。
//! 本模块纯函数,不读写数据库、不访问远端;敏感串仅随机生成,不回显日志。
//! 由主文件以 use super:: 调用 fill_local_exit_outbound_config 入口。
//! 注释保持中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use super::routing_local_exits::vless_endpoint_security;
use crate::config_text;
use crate::validation::{TLS_CERTIFICATE_KEYS, TLS_KEY_KEYS};
use crate::DbError;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::OsRng;
use serde_json::{json, Value};
use std::net::IpAddr;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

const DEFAULT_REALITY_SERVER_NAME: &str = "www.cloudflare.com";

/// 按协议把本机出口 outbound_config 补成可启动的最小配置,并做必填项校验。
pub(super) fn fill_local_exit_outbound_config(
    outbound_type: &str,
    host: &str,
    outbound_config: Value,
) -> Result<Value, DbError> {
    if outbound_type == "socks" || outbound_type == "http" {
        return fill_local_exit_socks_http_config(outbound_config);
    }
    if outbound_type == "shadowsocks" {
        return fill_local_exit_shadowsocks_config(outbound_config);
    }
    if outbound_type == "hysteria" {
        return fill_local_exit_hysteria_config(outbound_config);
    }
    if outbound_type == "trojan" {
        return fill_local_exit_trojan_config(outbound_config);
    }
    if outbound_type != "vless" || vless_endpoint_security(&outbound_config) != "reality" {
        return Ok(outbound_config);
    }

    let mut object = outbound_config
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("连接配置必须是 JSON 对象".to_string()))?;

    let current_config = Value::Object(object.clone());
    if config_text(&current_config, &["security"]).is_none() {
        object.insert("security".to_string(), json!("reality"));
    }

    normalize_local_exit_reality_server_name(&mut object, host);

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
        let (public_key, private_key) = generate_reality_key_pair();
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
        object.insert("short_id".to_string(), json!(random_reality_short_id()));
    }

    Ok(Value::Object(object))
}

fn fill_local_exit_hysteria_config(outbound_config: Value) -> Result<Value, DbError> {
    let mut object = outbound_config
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("连接配置必须是 JSON 对象".to_string()))?;
    // 本机出口=中转节点自家 HY2 服务端,password/auth 缺失时自动补随机串(仿 socks/http/SS/Reality 自动凭据),
    // 避免再次编辑本机出口时 password 丢失触发"Hysteria2 password/auth 不能为空";管理员已填则保留。
    let current_config = Value::Object(object.clone());
    if config_text(&current_config, &["password", "auth"]).is_none() {
        object.insert("password".to_string(), json!(random_local_exit_secret()));
    }
    let filled = Value::Object(object);
    // 证书文件仍必须存在:HY2 走 TLS,缺证书会导致中转节点 Xray 启动失败。
    if config_text(&filled, TLS_CERTIFICATE_KEYS).is_none()
        || config_text(&filled, TLS_KEY_KEYS).is_none()
    {
        return Err(DbError::InvalidAgentPayload(
            "HY2 本机出口需要配置 TLS 证书文件和私钥文件，否则会导致中转节点 Xray 启动失败"
                .to_string(),
        ));
    }
    Ok(filled)
}

fn fill_local_exit_trojan_config(outbound_config: Value) -> Result<Value, DbError> {
    let mut object = outbound_config
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("连接配置必须是 JSON 对象".to_string()))?;
    // Trojan 本机出口同 HY2:password 缺失自动补随机串(本机=自家服务端),避免再次编辑丢 password 报错。
    let current_config = Value::Object(object.clone());
    if config_text(&current_config, &["password", "auth"]).is_none() {
        object.insert("password".to_string(), json!(random_local_exit_secret()));
    }
    let filled = Value::Object(object);
    // 证书文件仍必须存在:Trojan 走 TLS,缺证书会导致中转节点 Xray 启动失败。
    if config_text(&filled, TLS_CERTIFICATE_KEYS).is_none()
        || config_text(&filled, TLS_KEY_KEYS).is_none()
    {
        return Err(DbError::InvalidAgentPayload(
            "Trojan 本机出口需要配置 TLS 证书文件和私钥文件，否则会导致中转节点 Xray 启动失败"
                .to_string(),
        ));
    }
    Ok(filled)
}

fn normalize_local_exit_reality_server_name(
    object: &mut serde_json::Map<String, Value>,
    host: &str,
) {
    let current_config = Value::Object(object.clone());
    let server_name = config_text(&current_config, &["server_name", "servername", "sni"]);
    if server_name
        .as_deref()
        .is_none_or(|value| reality_name_matches_endpoint_host(value, host))
    {
        object.insert(
            "server_name".to_string(),
            json!(DEFAULT_REALITY_SERVER_NAME),
        );
    }

    let current_config = Value::Object(object.clone());
    let dest = config_text(&current_config, &["dest", "reality_dest", "realityDest"]);
    if dest
        .as_deref()
        .is_none_or(|value| reality_dest_matches_endpoint_host(value, host))
    {
        object.insert(
            "dest".to_string(),
            json!(format!("{DEFAULT_REALITY_SERVER_NAME}:443")),
        );
    }
}

fn reality_name_matches_endpoint_host(value: &str, host: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case(host.trim()) || value.parse::<IpAddr>().is_ok()
}

fn reality_dest_matches_endpoint_host(value: &str, host: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case(&format!("{}:443", host.trim())) || value.parse::<IpAddr>().is_ok()
}

/// socks/http 本机出口凭据自动补齐:节点渲染(xray_render_local)要求 username+password 非空,
/// 否则本机服务直接被跳过、不渲染。缺哪个就随机生成哪个(仿 Reality 自动密钥的做法),已有则保留。
/// 不强制覆盖管理员显式填入的凭据;空白(仅空格)视为未填,统一以随机串补齐。
fn fill_local_exit_socks_http_config(outbound_config: Value) -> Result<Value, DbError> {
    let mut object = outbound_config
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("连接配置必须是 JSON 对象".to_string()))?;
    let current_config = Value::Object(object.clone());
    // username:渲染读 ["username","user"];任一非空即视为已填,缺失/空白则补随机串。
    if config_text(&current_config, &["username", "user"]).is_none() {
        object.insert("username".to_string(), json!(random_local_exit_secret()));
    }
    let current_config = Value::Object(object.clone());
    // password:渲染读 ["password","pass","auth"];同上,缺失/空白补随机串。
    if config_text(&current_config, &["password", "pass", "auth"]).is_none() {
        object.insert("password".to_string(), json!(random_local_exit_secret()));
    }
    Ok(Value::Object(object))
}

/// 生成本机出口随机凭据串:32 位十六进制(128 bit),足够强且不含特殊字符,适配 socks/http 用户名密码。
fn random_local_exit_secret() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fill_local_exit_shadowsocks_config(outbound_config: Value) -> Result<Value, DbError> {
    let mut object = outbound_config
        .as_object()
        .cloned()
        .ok_or_else(|| DbError::InvalidAgentPayload("连接配置必须是 JSON 对象".to_string()))?;
    let current_config = Value::Object(object.clone());
    let method = config_text(&current_config, &["method", "cipher"])
        .unwrap_or_else(|| "2022-blake3-aes-128-gcm".to_string());
    if config_text(&current_config, &["method", "cipher"]).is_none() {
        object.insert("method".to_string(), json!(method.clone()));
    }
    let current_config = Value::Object(object.clone());
    if crate::shadowsocks_keys::validate_shadowsocks_endpoint_config(&current_config).is_err()
        && method.trim().starts_with("2022-")
    {
        object.insert(
            "password".to_string(),
            json!(random_shadowsocks_2022_key(&method)?),
        );
    }
    Ok(Value::Object(object))
}

fn random_shadowsocks_2022_key(method: &str) -> Result<String, DbError> {
    let len = match method.trim().to_ascii_lowercase().as_str() {
        "2022-blake3-aes-128-gcm" => 16,
        "2022-blake3-aes-256-gcm" | "2022-blake3-chacha20-poly1305" => 32,
        other => {
            return Err(DbError::InvalidAgentPayload(format!(
                "不支持的 Shadowsocks 2022 method: {other}"
            )));
        }
    };
    let mut bytes = vec![0_u8; len];
    OsRng.fill_bytes(&mut bytes);
    Ok(STANDARD.encode(bytes))
}

fn generate_reality_key_pair() -> (String, String) {
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);
    (
        URL_SAFE_NO_PAD.encode(public_key.as_bytes()),
        URL_SAFE_NO_PAD.encode(private_key.to_bytes()),
    )
}

fn random_reality_short_id() -> String {
    let value = Uuid::new_v4().simple().to_string();
    value[..16].to_string()
}

#[cfg(test)]
mod tests {
    //! 本机出口 HY2/Trojan 的 password 自动补齐回归:再次编辑丢 password 时后端兜底,不再报"不能为空"。
    use super::*;

    fn certs_only() -> Value {
        json!({
            "certificate_file": "/etc/xray/certs/exit.crt",
            "key_file": "/etc/xray/certs/exit.key",
            "server_name": "exit.example.test"
        })
    }

    #[test]
    fn hysteria_local_exit_auto_fills_password_when_missing() {
        // 再次编辑本机出口 HY2、config 缺 password 时:自动补随机串,而非报"password/auth 不能为空"。
        let filled = fill_local_exit_outbound_config("hysteria", "192.0.2.1", certs_only())
            .expect("HY2 缺 password 应自动补齐而非报错");
        assert!(config_text(&filled, &["password", "auth"])
            .as_deref()
            .is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn trojan_local_exit_auto_fills_password_when_missing() {
        let filled = fill_local_exit_outbound_config("trojan", "192.0.2.1", certs_only())
            .expect("Trojan 缺 password 应自动补齐");
        assert!(config_text(&filled, &["password", "auth"])
            .as_deref()
            .is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn hysteria_local_exit_keeps_admin_password() {
        // 管理员已填 password 则保留,不被随机串覆盖。
        let mut cfg = certs_only();
        cfg.as_object_mut()
            .unwrap()
            .insert("password".to_string(), json!("admin-set-pw"));
        let filled = fill_local_exit_outbound_config("hysteria", "192.0.2.1", cfg).unwrap();
        assert_eq!(
            config_text(&filled, &["password"]).as_deref(),
            Some("admin-set-pw")
        );
    }

    #[test]
    fn hysteria_local_exit_still_requires_cert_files() {
        // 缺 TLS 证书/私钥文件仍必须拒绝(HY2 走 TLS,缺证书会导致 Xray 启动失败)。
        let cfg = json!({ "password": "x" });
        assert!(fill_local_exit_outbound_config("hysteria", "192.0.2.1", cfg).is_err());
    }
}
