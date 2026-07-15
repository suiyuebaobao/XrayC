//! 本模块负责 Xray 协议字段的纯转换。
//! 出口端点协议、入口协议和配置文本读取都集中在这里。
//! 函数只读取 AccessLine、ExitEndpoint 和 JSON 配置。
//! 模块不访问数据库，也不构建完整 agent 配置。
//! 完整配置渲染由 xray_render 模块组合这些 helper 完成。
//! 校验模块通过 crate root 复用配置文本和 VLESS 安全推断。
//! 标签生成保持稳定，避免 agent 配置 diff 抖动。
//! 新增协议支持应优先补充本模块和对应测试。
//! 本模块不得引入 SQL、PgStore 或持久化状态。
//! 文件行数保持低于 500 行，便于协议矩阵扩展。

use serde_json::Value;
use uuid::Uuid;
use xrayc_core::{AccessLine, EndpointType, ExitEndpoint};
use xrayc_xray_config::{AccessProtocol as XrayAccessProtocol, ExitProtocol as XrayExitProtocol};

use crate::validation::{TLS_CERTIFICATE_KEYS, TLS_KEY_KEYS};

pub(crate) fn xray_exit_protocol_for_endpoint(endpoint: &ExitEndpoint) -> Option<XrayExitProtocol> {
    match endpoint.outbound_type {
        EndpointType::Direct => Some(XrayExitProtocol::Direct),
        EndpointType::Socks if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            let (username, password) = optional_user_pass(&endpoint.outbound_config)?;
            Some(XrayExitProtocol::Socks {
                address: endpoint.host.clone(),
                port: endpoint.port,
                username,
                password,
            })
        }
        EndpointType::Http if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            let (username, password) = optional_user_pass(&endpoint.outbound_config)?;
            Some(XrayExitProtocol::Http {
                address: endpoint.host.clone(),
                port: endpoint.port,
                username,
                password,
            })
        }
        EndpointType::Vless if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            let uuid = config_text(&endpoint.outbound_config, &["uuid", "id"])?;
            let public_key = config_text(
                &endpoint.outbound_config,
                &["public_key", "publicKey", "reality_public_key"],
            );
            let security = vless_security(&endpoint.outbound_config);
            match security.as_str() {
                "tls" | "none" => {}
                "reality" => {
                    config_text(
                        &endpoint.outbound_config,
                        &["server_name", "servername", "sni"],
                    )?;
                    public_key.as_ref()?;
                }
                _ => return None,
            }
            Some(XrayExitProtocol::Vless {
                address: endpoint.host.clone(),
                port: endpoint.port,
                uuid,
                security: Some(security),
                flow: config_text(&endpoint.outbound_config, &["flow"]),
                server_name: config_text(
                    &endpoint.outbound_config,
                    &["server_name", "servername", "sni"],
                ),
                public_key,
                short_id: config_text(&endpoint.outbound_config, &["short_id", "shortId"]),
                fingerprint: config_text(
                    &endpoint.outbound_config,
                    &["fingerprint", "client_fingerprint", "clientFingerprint"],
                ),
                network: config_text(
                    &endpoint.stream_config,
                    &["network_mode", "networkMode", "network"],
                ),
                xhttp_path: config_text(
                    &endpoint.stream_config,
                    &["xhttp_path", "xhttpPath", "path"],
                ),
                xhttp_host: config_text(
                    &endpoint.stream_config,
                    &["xhttp_host", "xhttpHost", "host_header"],
                ),
                xhttp_mode: config_text(
                    &endpoint.stream_config,
                    &["xhttp_mode", "xhttpMode", "mode"],
                ),
                udp_packet_encoding: config_text(
                    &endpoint.stream_config,
                    &[
                        "udp_packet_encoding",
                        "udpPacketEncoding",
                        "packet_encoding",
                    ],
                ),
            })
        }
        EndpointType::Trojan if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            Some(XrayExitProtocol::Trojan {
                address: endpoint.host.clone(),
                port: endpoint.port,
                password: config_text(&endpoint.outbound_config, &["password"])?,
                security: config_text(&endpoint.outbound_config, &["security"]),
                server_name: config_text(
                    &endpoint.outbound_config,
                    &["server_name", "servername", "sni"],
                ),
            })
        }
        EndpointType::Shadowsocks if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            Some(XrayExitProtocol::Shadowsocks {
                address: endpoint.host.clone(),
                port: endpoint.port,
                method: config_text(&endpoint.outbound_config, &["method", "cipher"])?,
                password: config_text(&endpoint.outbound_config, &["password"])?,
            })
        }
        EndpointType::Hysteria if !endpoint.host.trim().is_empty() && endpoint.port > 0 => {
            Some(XrayExitProtocol::Hysteria2 {
                address: endpoint.host.clone(),
                port: endpoint.port,
                password: config_text(&endpoint.outbound_config, &["password", "auth"])?,
                server_name: config_text(
                    &endpoint.outbound_config,
                    &["server_name", "servername", "sni"],
                ),
                allow_insecure: Some(json_bool(
                    &endpoint.outbound_config,
                    &["allow_insecure", "allowInsecure", "insecure"],
                    false,
                )),
            })
        }
        _ => None,
    }
}

fn optional_user_pass(config: &Value) -> Option<(Option<String>, Option<String>)> {
    if endpoint_config_is_empty(config) {
        return Some((None, None));
    }
    let username = config_text(config, &["username", "user"])?;
    let password = config_text(config, &["password", "pass"])?;
    Some((Some(username), Some(password)))
}

pub(crate) fn vless_security(config: &Value) -> String {
    config_text(config, &["security"])
        .map(|security| security.trim().to_ascii_lowercase())
        .filter(|security| !security.is_empty())
        .unwrap_or_else(|| {
            if config_text(config, &["public_key", "publicKey", "reality_public_key"]).is_some() {
                "reality".to_string()
            } else {
                "tls".to_string()
            }
        })
}

pub(crate) fn config_text(config: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| config.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn endpoint_config_is_empty(config: &Value) -> bool {
    match config.as_object() {
        Some(object) => object.is_empty(),
        None => false,
    }
}

pub(crate) fn access_protocol_for_line(line: &AccessLine) -> Option<XrayAccessProtocol> {
    match line.protocol.trim().to_ascii_lowercase().as_str() {
        "" | "vless" => Some(XrayAccessProtocol::Vless {
            flow: non_empty_text(&line.flow),
            // 量子加密(VLESS native encryption,后量子)开启时入站 decryption = 存的 mlkem768x25519plus 串;
            // 未开启则为 "none"(原口径)。串含服务端私钥,只进 agent 入站配置、绝不入订阅/日志。
            decryption: Some(
                config_text(&line.inbound_config, &["vless_decryption"])
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "none".to_string()),
            ),
        }),
        "trojan" => Some(XrayAccessProtocol::Trojan),
        "hysteria" | "hy2" | "hysteria2" => Some(XrayAccessProtocol::Hysteria2),
        "shadowsocks" | "ss" if line.transport.trim().eq_ignore_ascii_case("tcp") => {
            let method = shadowsocks_method_for_line(line);
            let server_password = shadowsocks_server_password_for_line(line, &method)?;
            Some(XrayAccessProtocol::Shadowsocks {
                method,
                server_password,
                network: shadowsocks_network_for_line(line).to_string(),
            })
        }
        _ => None,
    }
}

fn shadowsocks_method_for_line(line: &AccessLine) -> String {
    config_text(&line.inbound_config, &["method", "cipher"])
        .unwrap_or_else(|| "aes-256-gcm".to_string())
}

fn shadowsocks_server_password_for_line(line: &AccessLine, method: &str) -> Option<String> {
    config_text(
        &line.inbound_config,
        &[
            "password",
            "server_password",
            "serverPassword",
            "root_password",
        ],
    )
    .or_else(|| (!method.trim().starts_with("2022-")).then(|| line.uuid.clone()))
}

fn shadowsocks_network_for_line(line: &AccessLine) -> &'static str {
    if line.udp_enabled {
        "tcp,udp"
    } else {
        "tcp"
    }
}

pub(crate) fn access_line_inbound_security(line: &AccessLine) -> Option<String> {
    config_text(&line.inbound_config, &["security"])
        .map(|security| security.trim().to_ascii_lowercase())
        .filter(|security| !security.is_empty())
        .or_else(|| {
            config_text(
                &line.inbound_config,
                &[
                    "private_key",
                    "privateKey",
                    "reality_private_key",
                    "realityPrivateKey",
                ],
            )
            .map(|_| "reality".to_string())
        })
}

pub(crate) fn access_line_server_name(line: &AccessLine) -> Option<String> {
    non_empty_text(&line.server_name).or_else(|| {
        config_text(
            &line.inbound_config,
            &["server_name", "serverName", "sni", "tls_server_name"],
        )
    })
}

pub(crate) fn access_line_tls_certificate_file(line: &AccessLine) -> Option<String> {
    config_text(&line.inbound_config, TLS_CERTIFICATE_KEYS)
}

pub(crate) fn access_line_tls_key_file(line: &AccessLine) -> Option<String> {
    config_text(&line.inbound_config, TLS_KEY_KEYS)
}

pub(crate) fn access_line_short_ids(line: &AccessLine) -> Vec<String> {
    let mut ids = line
        .inbound_config
        .get("short_ids")
        .or_else(|| line.inbound_config.get("shortIds"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        if let Some(short_id) = non_empty_text(&line.short_id) {
            ids.push(short_id);
        }
    }
    ids
}

pub(crate) fn non_empty_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(crate) fn access_inbound_tag_for_id(access_line_id: &str) -> String {
    format!("access-{access_line_id}")
}

pub(crate) fn exit_endpoint_tag(exit_endpoint_id: Uuid) -> String {
    format!("exit-{exit_endpoint_id}")
}

fn json_bool(value: &Value, fields: &[&str], default: bool) -> bool {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_bool))
        .unwrap_or(default)
}
