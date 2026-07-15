//! 本模块负责把中转入口渲染成客户端代理节点。
//! 支持协议会在这里统一筛选，避免把不可安全表达的入站配置下发。
//! 所有节点字段都只来源于中转入口，不读取上游出口敏感信息。
//! 订阅节点名称来自中转入口自身，不暴露出口池或上游线路名。
//! 订阅不输出图标字段，避免客户端显示和后台分组配置耦合。
//! VLESS、Trojan、Shadowsocks、HY2 的字段在本文件集中生成。
//! 协议安全模式只读取中转入口配置，不能推断上游出口配置。
//! 不支持的入站协议会在生成前过滤，避免客户端拿到不可用节点。
//! 新增协议时需要补充 redaction 测试，防止上游凭据泄露。
//! 本头部满足前十行中文注释约束。

use crate::model::AccessLine;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::options::string_field;
use super::VisibleLine;

#[derive(Debug, Serialize)]
struct XhttpOptions {
    path: String,
    mode: String,
}

#[derive(Debug, Serialize)]
struct WebSocketOptions {
    path: String,
    headers: WebSocketHeaders,
}

#[derive(Debug, Serialize)]
struct WebSocketHeaders {
    #[serde(rename = "Host")]
    host: String,
}

#[derive(Debug, Serialize)]
struct GrpcOptions {
    #[serde(rename = "grpc-service-name")]
    service_name: String,
}

#[derive(Debug, Serialize)]
struct RealityOptions {
    #[serde(rename = "public-key")]
    public_key: String,
    #[serde(rename = "short-id")]
    short_id: String,
}

pub(super) fn clash_proxy_for_line(visible_line: &VisibleLine) -> Value {
    match visible_line
        .line
        .protocol
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "trojan" => trojan_proxy_for_line(visible_line),
        "hysteria" | "hy2" | "hysteria2" => hy2_proxy_for_line(visible_line),
        "shadowsocks" | "ss" => shadowsocks_proxy_for_line(visible_line),
        _ => vless_proxy_for_line(visible_line),
    }
}

fn vless_proxy_for_line(visible_line: &VisibleLine) -> Value {
    let mut proxy = json!({
        "name": &visible_line.proxy_name,
        "type": "vless",
        "server": &visible_line.line.listen_host,
        "port": visible_line.line.listen_port,
        "uuid": &visible_line.credential,
        "udp": visible_line.line.udp_enabled,
        "network": access_line_network(&visible_line.line),
    });
    if let Some(flow) = non_empty_text(&visible_line.line.flow) {
        proxy["flow"] = json!(flow);
    }
    // 客户端订阅必须严格匹配中转入口的入站安全模式。
    // 没有显式安全配置时，下发普通 VLESS TCP，避免客户端误用 TLS/Reality。
    match access_line_inbound_security(&visible_line.line).as_deref() {
        Some("reality") => {
            proxy["tls"] = json!(true);
            if let Some(server_name) = access_line_server_name(&visible_line.line) {
                proxy["servername"] = json!(server_name);
            }
            proxy["client-fingerprint"] = json!("chrome");
            if let Some(public_key) = non_empty_text(&visible_line.line.public_key) {
                proxy["reality-opts"] = json!(RealityOptions {
                    public_key,
                    short_id: visible_line.line.short_id.clone(),
                });
            }
        }
        Some("tls") => {
            proxy["tls"] = json!(true);
            if let Some(server_name) = access_line_server_name(&visible_line.line) {
                proxy["servername"] = json!(server_name);
            }
            proxy["client-fingerprint"] = json!("chrome");
        }
        _ => {
            proxy["tls"] = json!(false);
        }
    }
    // 量子加密(VLESS native encryption,后量子)开启时客户端必须带 encryption = 存的公钥串,
    // 否则与中转入站 decryption 对不上、握手失败。只下发公钥串;服务端私钥串 vless_decryption 绝不输出(防泄露)。
    if let Some(encryption) = visible_line
        .line
        .inbound_config
        .get("vless_encryption")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        proxy["encryption"] = json!(encryption);
    }
    insert_xhttp_options(&mut proxy, &visible_line.line);
    insert_websocket_options(&mut proxy, &visible_line.line);
    insert_grpc_options(&mut proxy, &visible_line.line);
    insert_udp_packet_encoding(&mut proxy, &visible_line.line);
    proxy
}

fn trojan_proxy_for_line(visible_line: &VisibleLine) -> Value {
    let inbound_security = access_line_inbound_security(&visible_line.line);
    let tls_enabled = matches!(inbound_security.as_deref(), Some("tls" | "reality"));
    let mut proxy = json!({
        "name": &visible_line.proxy_name,
        "type": "trojan",
        "server": &visible_line.line.listen_host,
        "port": visible_line.line.listen_port,
        "password": &visible_line.credential,
        "udp": visible_line.line.udp_enabled,
        "tls": tls_enabled,
        "skip-cert-verify": false,
        "client-fingerprint": "chrome",
        "network": &visible_line.line.transport,
    });
    // Trojan 订阅必须和中转入站安全模式一致；默认明文 TCP 不能伪装成 TLS。
    if tls_enabled {
        if let Some(server_name) = access_line_server_name(&visible_line.line) {
            proxy["sni"] = json!(server_name);
            proxy["servername"] = json!(server_name);
        }
    }
    if inbound_security.as_deref() == Some("reality") {
        if let Some(public_key) = non_empty_text(&visible_line.line.public_key) {
            proxy["reality-opts"] = json!(RealityOptions {
                public_key,
                short_id: visible_line.line.short_id.clone(),
            });
        }
    }
    insert_xhttp_options(&mut proxy, &visible_line.line);
    insert_websocket_options(&mut proxy, &visible_line.line);
    insert_grpc_options(&mut proxy, &visible_line.line);
    proxy
}

fn shadowsocks_proxy_for_line(visible_line: &VisibleLine) -> Value {
    let method = shadowsocks_method(&visible_line.line);
    json!({
        "name": &visible_line.proxy_name,
        "type": "ss",
        "server": &visible_line.line.listen_host,
        "port": visible_line.line.listen_port,
        "cipher": method,
        "password": shadowsocks_subscription_password(
            &visible_line.line,
            &method,
            &visible_line.credential
        ),
        "udp": visible_line.line.udp_enabled,
    })
}

fn hy2_proxy_for_line(visible_line: &VisibleLine) -> Value {
    let mut proxy = json!({
        "name": &visible_line.proxy_name,
        "type": "hysteria2",
        "server": &visible_line.line.listen_host,
        "port": visible_line.line.listen_port,
        "password": &visible_line.credential,
        "udp": visible_line.line.udp_enabled,
        "skip-cert-verify": false,
        "alpn": ["h3"],
    });
    if let Some(server_name) = access_line_server_name(&visible_line.line) {
        proxy["sni"] = json!(server_name);
    }
    proxy
}

pub(super) fn subscription_protocol_supported(line: &AccessLine) -> bool {
    match line.protocol.trim().to_ascii_lowercase().as_str() {
        "" | "vless" => vless_subscription_supported(line),
        "trojan" => trojan_subscription_supported(line),
        "hysteria" | "hy2" | "hysteria2" => hy2_subscription_supported(line),
        "shadowsocks" | "ss" => shadowsocks_subscription_supported(line),
        _ => false,
    }
}

fn vless_subscription_supported(line: &AccessLine) -> bool {
    if access_line_inbound_security(line).as_deref() != Some("reality") {
        return true;
    }
    matches!(
        line.transport.trim().to_ascii_lowercase().as_str(),
        "tcp" | "xhttp" | "grpc"
    ) && !line.udp_enabled
        && line.udp_packet_encoding.trim().is_empty()
}

fn hy2_subscription_supported(line: &AccessLine) -> bool {
    matches!(access_line_inbound_security(line).as_deref(), Some("tls"))
        && access_line_server_name(line).is_some()
        && line.transport.trim().eq_ignore_ascii_case("hysteria")
        && line.udp_enabled
        && line.udp_packet_encoding.trim().is_empty()
}

fn trojan_subscription_supported(line: &AccessLine) -> bool {
    matches!(access_line_inbound_security(line).as_deref(), Some("tls"))
        && access_line_server_name(line).is_some()
}

fn shadowsocks_subscription_supported(line: &AccessLine) -> bool {
    let method = shadowsocks_method(line);
    method.trim().starts_with("2022-")
        && string_field(
            &line.inbound_config,
            &[
                "password",
                "server_password",
                "serverPassword",
                "root_password",
            ],
        )
        .is_some()
}

pub(super) fn proxy_name_for_line(line: &AccessLine, index: usize) -> String {
    let name = line.name.trim();
    if name.is_empty() {
        format!("节点 {index}")
    } else {
        format!("{name} {index}")
    }
}

fn insert_xhttp_options(proxy: &mut Value, line: &AccessLine) {
    if let Some(options) = xhttp_options(line) {
        proxy["xhttp-opts"] = json!(options);
    }
}

fn insert_websocket_options(proxy: &mut Value, line: &AccessLine) {
    if let Some(options) = websocket_options(line) {
        proxy["ws-opts"] = json!(options);
    }
}

fn insert_grpc_options(proxy: &mut Value, line: &AccessLine) {
    if let Some(options) = grpc_options(line) {
        proxy["grpc-opts"] = json!(options);
    }
}

fn insert_udp_packet_encoding(proxy: &mut Value, line: &AccessLine) {
    if line.udp_enabled && line.udp_packet_encoding.trim().eq_ignore_ascii_case("xudp") {
        proxy["packet-encoding"] = json!("xudp");
    }
}

fn xhttp_options(line: &AccessLine) -> Option<XhttpOptions> {
    line.transport
        .trim()
        .eq_ignore_ascii_case("xhttp")
        .then(|| XhttpOptions {
            path: default_if_empty(&line.xhttp_path, "/xrayc").to_string(),
            mode: default_if_empty(&line.xhttp_mode, "stream-one").to_string(),
        })
}

fn websocket_options(line: &AccessLine) -> Option<WebSocketOptions> {
    line.transport
        .trim()
        .eq_ignore_ascii_case("ws")
        .then(|| WebSocketOptions {
            path: default_if_empty(&line.xhttp_path, "/").to_string(),
            headers: WebSocketHeaders {
                host: websocket_host(line),
            },
        })
}

fn grpc_options(line: &AccessLine) -> Option<GrpcOptions> {
    line.transport
        .trim()
        .eq_ignore_ascii_case("grpc")
        .then(|| GrpcOptions {
            service_name: default_if_empty(&line.xhttp_path, "xrayc").to_string(),
        })
}

fn websocket_host(line: &AccessLine) -> String {
    non_empty_text(&line.xhttp_host)
        .or_else(|| access_line_server_name(line))
        .unwrap_or_else(|| line.listen_host.clone())
}

fn access_line_network(line: &AccessLine) -> String {
    match line.transport.trim().to_ascii_lowercase().as_str() {
        "" => "tcp".to_string(),
        transport => transport.to_string(),
    }
}

fn default_if_empty<'a>(value: &'a str, default: &'a str) -> &'a str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default
    } else {
        trimmed
    }
}

fn access_line_inbound_security(line: &AccessLine) -> Option<String> {
    string_field(&line.inbound_config, &["security"])
        .map(|security| security.trim().to_ascii_lowercase())
        .filter(|security| matches!(security.as_str(), "tls" | "reality"))
        .or_else(|| {
            string_field(
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

fn access_line_server_name(line: &AccessLine) -> Option<String> {
    non_empty_text(&line.server_name).or_else(|| {
        string_field(
            &line.inbound_config,
            &["server_name", "serverName", "sni", "tls_server_name"],
        )
    })
}

fn shadowsocks_method(line: &AccessLine) -> String {
    string_field(&line.inbound_config, &["method", "cipher"])
        .unwrap_or_else(|| "aes-256-gcm".to_string())
}

fn shadowsocks_subscription_password(line: &AccessLine, method: &str, credential: &str) -> String {
    let server_password = || {
        string_field(
            &line.inbound_config,
            &[
                "password",
                "server_password",
                "serverPassword",
                "root_password",
            ],
        )
        .unwrap_or_else(|| line.uuid.clone())
    };
    if method.trim().starts_with("2022-") {
        format!(
            "{}:{}",
            server_password(),
            shadowsocks_2022_user_key(method, credential)
        )
    } else {
        // Xray 的旧版 Shadowsocks 入站按线路级服务端密码验证。
        // 多用户凭据仍用于其他协议和服务端授权，但不能直接作为 Shadowsocks 客户端密码。
        server_password()
    }
}

fn shadowsocks_2022_user_key(method: &str, credential: &str) -> String {
    let key_len = if method.trim().contains("aes-128") {
        16
    } else {
        32
    };
    let digest = Sha256::digest(credential.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(&digest[..key_len])
}

fn non_empty_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
