//! 数据库层纯校验函数：归一输入、检查枚举和裁剪文本，不执行 SQL。
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

use crate::{config_text, non_empty_text, vless_security, DbError};

fn invalid_payload(message: &str) -> DbError {
    DbError::InvalidAgentPayload(message.to_string())
}

pub(crate) const TLS_CERTIFICATE_KEYS: &[&str] = &[
    "certificate_file",
    "certificateFile",
    "cert_file",
    "certFile",
    "tls_certificate_file",
    "tlsCertificateFile",
];
pub(crate) const TLS_KEY_KEYS: &[&str] = &[
    "key_file",
    "keyFile",
    "private_key_file",
    "privateKeyFile",
    "tls_key_file",
    "tlsKeyFile",
];

pub(crate) fn required_admin_text(
    value: &str,
    field: &str,
    max_chars: usize,
) -> Result<String, DbError> {
    let text = optional_admin_text(value, max_chars);
    if text.is_empty() {
        return Err(DbError::InvalidAgentPayload(format!("{field}不能为空")));
    }
    Ok(text)
}

pub(crate) fn optional_required_admin_text(
    value: Option<String>,
    field: &str,
    max_chars: usize,
) -> Result<Option<String>, DbError> {
    value
        .map(|text| required_admin_text(&text, field, max_chars))
        .transpose()
}

pub(crate) fn optional_admin_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

pub(crate) fn validate_non_negative_i64(value: i64, field: &str) -> Result<i64, DbError> {
    if value < 0 {
        return Err(DbError::InvalidAgentPayload(format!("{field}不能小于 0")));
    }
    Ok(value)
}

pub(crate) fn validate_positive_i32(value: i32, field: &str) -> Result<i32, DbError> {
    if value <= 0 {
        return Err(DbError::InvalidAgentPayload(format!("{field}必须大于 0")));
    }
    Ok(value)
}

// 数据库用 -1 表示不限流量，保留 0 的零额度含义。
pub(crate) fn validate_traffic_limit(value: i64) -> Result<i64, DbError> {
    if value < -1 {
        return Err(invalid_payload("流量额度必须为 -1（不限流量）或非负数"));
    }
    Ok(value)
}

pub(crate) fn validate_plan_multiplier(value: f64, field: &str) -> Result<f64, DbError> {
    if !value.is_finite() || value <= 0.0 || value > 100.0 {
        return Err(DbError::InvalidAgentPayload(format!(
            "{field}必须在 0 到 100 之间"
        )));
    }
    Ok((value * 1000.0).round() / 1000.0)
}

pub(crate) fn validate_endpoint_type_name(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "direct" => Err(DbError::InvalidAgentPayload(
            "当前 V2 简化模型不支持 direct 出口协议".to_string(),
        )),
        "socks" | "socks5" => Ok("socks"),
        "http" => Ok("http"),
        "vless" => Ok("vless"),
        "trojan" => Ok("trojan"),
        "shadowsocks" | "ss" => Ok("shadowsocks"),
        "hysteria" | "hy2" | "hysteria2" => Ok("hysteria"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的出口协议: {other}"
        ))),
    }
}

pub(crate) fn validate_exit_endpoint_protocol_config(
    outbound_type: &str,
    outbound_config: &Value,
) -> Result<(), DbError> {
    let Some(object) = outbound_config.as_object() else {
        return Err(DbError::InvalidAgentPayload(
            "出口协议配置必须是 JSON 对象".to_string(),
        ));
    };

    match outbound_type {
        "direct" if !object.is_empty() => {
            return Err(DbError::InvalidAgentPayload(
                "direct 出口协议配置必须为空对象".to_string(),
            ));
        }
        "direct" => {}
        "socks" => validate_optional_auth_pair(outbound_config, "SOCKS")?,
        "http" => validate_optional_auth_pair(outbound_config, "HTTP")?,
        "vless" => {
            require_config_text(outbound_config, &["uuid", "id"], "VLESS uuid")?;
            match vless_security(outbound_config).as_str() {
                "reality" => {
                    require_config_text(
                        outbound_config,
                        &["server_name", "servername", "sni"],
                        "VLESS server_name",
                    )?;
                    require_config_text(
                        outbound_config,
                        &["public_key", "publicKey", "reality_public_key"],
                        "VLESS public_key",
                    )?;
                }
                "tls" | "none" => {}
                security => {
                    return Err(DbError::InvalidAgentPayload(format!(
                        "不支持的 VLESS security: {security}"
                    )));
                }
            }
        }
        "trojan" => {
            require_config_text(outbound_config, &["password"], "Trojan password")?;
            match config_text(outbound_config, &["security"])
                .map(|security| security.trim().to_ascii_lowercase())
                .filter(|security| !security.is_empty())
                .unwrap_or_else(|| "tls".to_string())
                .as_str()
            {
                "tls" => {
                    require_config_text(
                        outbound_config,
                        &["server_name", "servername", "sni"],
                        "Trojan server_name/SNI",
                    )?;
                }
                security => {
                    return Err(DbError::InvalidAgentPayload(format!(
                        "不支持的 Trojan security: {security}；Trojan 出口必须使用 TLS"
                    )));
                }
            }
        }
        "shadowsocks" => {
            crate::shadowsocks_keys::validate_shadowsocks_endpoint_config(outbound_config)?;
        }
        "hysteria" => {
            require_config_text(
                outbound_config,
                &["password", "auth"],
                "Hysteria2 password/auth",
            )?;
        }
        _ => {}
    }

    Ok(())
}

fn validate_optional_auth_pair(config: &Value, protocol_label: &str) -> Result<(), DbError> {
    let username = config_text(config, &["username", "user"]);
    let password = config_text(config, &["password", "pass"]);
    if username.is_some() == password.is_some() {
        return Ok(());
    }

    Err(DbError::InvalidAgentPayload(format!(
        "{protocol_label} 认证用户名和密码必须同时填写或同时为空"
    )))
}

fn require_config_text(
    config: &Value,
    keys: &[&str],
    field_label: &str,
) -> Result<String, DbError> {
    config_text(config, keys)
        .ok_or_else(|| DbError::InvalidAgentPayload(format!("{field_label} 不能为空")))
}

pub(crate) fn validate_ownership(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "third_party" => Ok("third_party"),
        "self_hosted" => Ok("self_hosted"),
        "local_direct" => Err(DbError::InvalidAgentPayload(
            "当前 V2 简化模型不支持 local_direct 出口归属".to_string(),
        )),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的出口归属: {other}"
        ))),
    }
}

pub(crate) fn validate_pool_strategy(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "priority" => Ok("priority"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的出口池策略: {other}"
        ))),
    }
}

pub(crate) fn validate_member_status(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "healthy" => Ok("healthy"),
        "offline" => Ok("offline"),
        "degraded" => Ok("degraded"),
        "draining" => Ok("draining"),
        "unknown" => Ok("unknown"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的成员状态: {other}"
        ))),
    }
}

pub(crate) fn validate_access_protocol(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "vless" => Ok("vless"),
        "trojan" => Ok("trojan"),
        "hysteria" | "hy2" | "hysteria2" => Ok("hysteria"),
        "shadowsocks" | "ss" => Ok("shadowsocks"),
        "http" | "socks" | "socks5" | "vmess" => Err(DbError::InvalidAgentPayload(format!(
            "接入协议 {value} 当前无法稳定绑定到 Xray 用户统计，已标记 unsupported"
        ))),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的接入协议: {other}"
        ))),
    }
}

pub(crate) fn validate_access_transport(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "tcp" => Ok("tcp"),
        "hysteria" | "hy2" | "hysteria2" => Ok("hysteria"),
        "xhttp" => Ok("xhttp"),
        "grpc" => Ok("grpc"),
        "ws" => Ok("ws"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的接入传输: {other}"
        ))),
    }
}

pub(crate) fn validate_access_protocol_transport(
    protocol: &str,
    transport: &str,
) -> Result<(), DbError> {
    if protocol == "vless" && !matches!(transport, "tcp" | "xhttp" | "ws" | "grpc") {
        return Err(invalid_payload("VLESS 接入仅支持 tcp/xhttp/ws/grpc 传输"));
    }
    if transport == "hysteria" && protocol != "hysteria" {
        return Err(invalid_payload("HY2 传输只能用于 HY2 接入"));
    }
    if protocol == "shadowsocks" && transport != "tcp" {
        return Err(invalid_payload("Shadowsocks 接入仅支持 tcp 传输"));
    }
    if protocol == "hysteria" && transport != "hysteria" {
        return Err(invalid_payload("HY2 接入必须使用 hysteria 传输"));
    }
    if protocol == "trojan" && !matches!(transport, "tcp" | "xhttp" | "ws" | "grpc") {
        return Err(invalid_payload("Trojan 接入仅支持 tcp/xhttp/ws/grpc 传输"));
    }
    Ok(())
}

/// CF(Cloudflare)护栏:启用 CDN 时强制协议组合可过 CF。
///
/// 核心事实(spec §1/§2):CF 只代理 HTTP(S),只有 WS+TLS 的 VLESS 能过 CF;
/// Reality、HY2(UDP/QUIC)、裸 TCP Trojan、Shadowsocks 都不能过 CF,只能直连。
/// 因此 `cdn_enabled=true` 时必须满足:protocol=="vless" && transport 含 "ws" && security=="tls"。
/// 否则拒绝并提示改用 VLESS-WS 或走灰云/IP 直连(security=reality 也在拒绝之列)。
///
/// 入参为已归一的协议/传输/安全字段(小写);本函数纯校验,不读写 SQL。
pub(crate) fn validate_hong_kong_hy2(protocol: &str, region_code: &str) -> Result<(), DbError> {
    let normalized_protocol = protocol.trim().to_ascii_lowercase();
    if matches!(
        normalized_protocol.as_str(),
        "hysteria" | "hy2" | "hysteria2"
    ) && is_hong_kong_region(region_code)
    {
        return Err(invalid_payload(
            "香港线路不支持 HY2，请改用 VLESS、Trojan 或 Shadowsocks",
        ));
    }
    Ok(())
}

pub(crate) fn is_hong_kong_region(region_code: &str) -> bool {
    matches!(
        region_code.trim().to_ascii_uppercase().as_str(),
        "HK" | "HKG" | "HONGKONG" | "HONG_KONG" | "香港"
    )
}

pub(crate) fn normalize_access_inbound_config(
    protocol: &str,
    mut inbound_config: Value,
    server_name: &str,
) -> Result<Value, DbError> {
    let security = config_text(&inbound_config, &["security"])
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(protocol, "trojan" | "hysteria") && security != "tls" {
        return Ok(inbound_config);
    }
    if !inbound_config.is_object() {
        return Err(DbError::InvalidAgentPayload(
            "TLS 接入配置必须是 JSON 对象".to_string(),
        ));
    }
    let sni = non_empty_text(server_name)
        .or_else(|| {
            config_text(
                &inbound_config,
                &["server_name", "serverName", "sni", "tls_server_name"],
            )
        })
        .ok_or_else(|| {
            DbError::InvalidAgentPayload("TLS 接入必须配置 server_name/SNI".to_string())
        })?;
    let domain = sni.split(',').next().map(str::trim).unwrap_or(&sni);
    let object = inbound_config.as_object_mut().expect("checked object");
    object
        .entry("security")
        .or_insert_with(|| Value::String("tls".to_string()));
    object
        .entry("certificate_file")
        .or_insert_with(|| Value::String(format!("/etc/letsencrypt/live/{domain}/fullchain.pem")));
    object
        .entry("key_file")
        .or_insert_with(|| Value::String(format!("/etc/letsencrypt/live/{domain}/privkey.pem")));
    Ok(inbound_config)
}

pub(crate) fn validate_access_inbound_config(
    protocol: &str,
    inbound_config: &Value,
    server_name: &str,
) -> Result<(), DbError> {
    let security = config_text(inbound_config, &["security"]).unwrap_or_else(|| {
        if matches!(protocol, "trojan" | "hysteria") {
            "tls".to_string()
        } else {
            String::new()
        }
    });
    if matches!(protocol, "trojan" | "hysteria") {
        if !security.trim().eq_ignore_ascii_case("tls") {
            return Err(DbError::InvalidAgentPayload(
                "Trojan/HY2 用户入站必须使用 TLS 和有效 SSL 证书".to_string(),
            ));
        }
        return validate_tls_inbound_config(inbound_config, server_name, "Trojan/HY2");
    }
    if security.trim().eq_ignore_ascii_case("tls") {
        validate_tls_inbound_config(inbound_config, server_name, "TLS")?;
    }
    if protocol != "shadowsocks" {
        return Ok(());
    }
    if !inbound_config.is_object() {
        return Err(DbError::InvalidAgentPayload(
            "Shadowsocks 接入配置必须是 JSON 对象".to_string(),
        ));
    }
    crate::shadowsocks_keys::validate_shadowsocks_inbound_config(inbound_config)?;
    Ok(())
}

fn validate_tls_inbound_config(
    inbound_config: &Value,
    server_name: &str,
    label: &str,
) -> Result<(), DbError> {
    if !inbound_config.is_object() {
        return Err(DbError::InvalidAgentPayload(format!(
            "{label} 接入配置必须是 JSON 对象"
        )));
    }
    let sni = non_empty_text(server_name).or_else(|| {
        config_text(
            inbound_config,
            &["server_name", "serverName", "sni", "tls_server_name"],
        )
    });
    if sni.is_none() {
        return Err(DbError::InvalidAgentPayload(format!(
            "{label} 接入必须配置 server_name/SNI"
        )));
    }
    if config_text(inbound_config, TLS_CERTIFICATE_KEYS).is_none()
        || config_text(inbound_config, TLS_KEY_KEYS).is_none()
    {
        return Err(DbError::InvalidAgentPayload(format!(
            "{label} 接入必须配置证书文件和私钥文件"
        )));
    }
    Ok(())
}

pub(crate) fn validate_xhttp_mode(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => Ok("auto"),
        "auto" => Ok("auto"),
        "packet-up" => Ok("packet-up"),
        "stream-up" => Ok("stream-up"),
        "stream-one" => Ok("stream-one"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "不支持的 XHTTP 模式: {other}"
        ))),
    }
}

pub(crate) fn validate_identity_mode(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "credential" => Ok("credential"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "当前中转入口只支持 credential 身份模式: {other}"
        ))),
    }
}

pub(crate) fn validate_user_key_source(value: &str) -> Result<&'static str, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "xray_email" => Ok("xray_email"),
        other => Err(DbError::InvalidAgentPayload(format!(
            "当前中转入口只支持 xray_email 用户标识来源: {other}"
        ))),
    }
}

pub(crate) fn ensure_unique_uuids(values: &[Uuid], message: &str) -> Result<(), DbError> {
    let mut seen = HashSet::new();
    if values.iter().copied().all(|value| seen.insert(value)) {
        Ok(())
    } else {
        Err(DbError::InvalidAgentPayload(message.to_string()))
    }
}

pub(crate) fn normalize_email(email: &str) -> Result<String, DbError> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty()
        || email.contains(' ')
        || email.contains('<')
        || email.contains('>')
        || email.matches('@').count() != 1
    {
        return Err(DbError::InvalidEmail);
    }
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(DbError::InvalidEmail);
    }
    Ok(email)
}

pub(crate) fn validate_password(password: &str) -> Result<(), DbError> {
    if password.len() < 8 {
        return Err(DbError::WeakPassword);
    }
    Ok(())
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

pub(crate) fn is_valid_client_ip_hash(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn db_port_to_u16(port: i32, field: &str, allow_zero: bool) -> Result<u16, DbError> {
    if port < 0 || port > u16::MAX as i32 || (!allow_zero && port == 0) {
        return Err(DbError::InvalidInput(format!("{field} 超出有效端口范围")));
    }
    Ok(port as u16)
}
