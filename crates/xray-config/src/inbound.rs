//! 本模块负责生成 Xray 入站配置。
//! 入口配置包含协议客户端、监听端口、传输层以及入站安全设置。
//! 主编译入口会为每条访问线路调用这里的生成函数。

use base64::Engine;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::util::{
    default_if_empty, default_if_empty_option, non_empty_option, require_access_field,
};
use crate::{AccessLine, AccessProtocol, LocalExitProtocol, LocalExitService, XrayConfigError};

pub(crate) const ACCESS_INBOUND_LISTEN: &str = "0.0.0.0";

pub(crate) fn compile_inbound(line: &AccessLine) -> Result<Value, XrayConfigError> {
    if line.users.is_empty() {
        return Err(XrayConfigError::EmptyUsers(line.id.clone()));
    }

    let clients = line
        .users
        .iter()
        .map(|user| match &line.protocol {
            AccessProtocol::Vless { flow, .. } => {
                let mut client = json!({
                    "id": user.credential,
                    "email": user.email,
                    "level": user.level,
                });
                if let Some(flow) = flow
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    client["flow"] = json!(flow);
                }
                client
            }
            AccessProtocol::Trojan => json!({
                "password": user.credential,
                "email": user.email,
                "level": user.level,
            }),
            AccessProtocol::Hysteria2 => json!({
                "auth": user.credential,
                "email": user.email,
                "level": user.level,
            }),
            AccessProtocol::Shadowsocks { method, .. } => {
                let mut client = json!({
                    "password": shadowsocks_user_password(method, &user.credential),
                    "email": user.email,
                    "level": user.level,
                });
                if let Some(method) = shadowsocks_user_method(method) {
                    client["method"] = json!(method);
                }
                client
            }
        })
        .collect::<Vec<_>>();

    let settings = match &line.protocol {
        AccessProtocol::Vless { decryption, .. } => json!({
            "clients": clients,
            "decryption": decryption.as_deref().unwrap_or("none"),
        }),
        AccessProtocol::Trojan => json!({
            "clients": clients,
        }),
        AccessProtocol::Hysteria2 => json!({
            "version": 2,
            "clients": clients,
        }),
        AccessProtocol::Shadowsocks {
            method,
            server_password,
            network,
        } => json!({
            "network": default_if_empty(network, "tcp"),
            "method": require_access_field(&line.id, "method", method)?,
            "password": require_access_field(&line.id, "server_password", server_password)?,
            "clients": clients,
        }),
    };

    let mut inbound = json!({
        "tag": inbound_tag(&line.id),
        // listen_host 是下发给客户端的公网访问地址，不一定存在于远端网卡。
        // Xray 入站绑定通配地址，才能兼容地址转换、公网映射和多云网络。
        "listen": ACCESS_INBOUND_LISTEN,
        "port": line.listen_port,
        "protocol": access_protocol_name(&line.protocol),
        "settings": settings,
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"],
        },
    });
    let stream_settings = inbound_stream_settings(line)?;
    if !stream_settings.is_null() {
        inbound["streamSettings"] = stream_settings;
    }
    Ok(inbound)
}

pub(crate) fn compile_local_exit_service_inbound(
    service: &LocalExitService,
) -> Result<Value, XrayConfigError> {
    let settings = match service.protocol {
        LocalExitProtocol::Socks => json!({
            "auth": "password",
            "udp": true,
            "accounts": [{
                "user": require_access_field(&service.id, "username", &service.username)?,
                "pass": require_access_field(&service.id, "password", &service.password)?,
            }],
        }),
        LocalExitProtocol::Http => json!({
            "accounts": [{
                "user": require_access_field(&service.id, "username", &service.username)?,
                "pass": require_access_field(&service.id, "password", &service.password)?,
            }],
        }),
        LocalExitProtocol::Vless => json!({
            "clients": [{
                "id": require_access_field(&service.id, "uuid", &service.uuid)?,
                "email": format!("local-exit-{}", service.id),
            }],
            "decryption": "none",
        }),
        LocalExitProtocol::Trojan => json!({
            "clients": [{
                "password": require_access_field(&service.id, "password", &service.password)?,
                "email": format!("local-exit-{}", service.id),
            }],
        }),
        LocalExitProtocol::Shadowsocks => json!({
            "network": default_if_empty(&service.network, "tcp"),
            "method": require_access_field(&service.id, "method", &service.method)?,
            "password": require_access_field(&service.id, "password", &service.password)?,
        }),
        LocalExitProtocol::Hysteria2 => json!({
            "version": 2,
            "clients": [{
                "auth": require_access_field(&service.id, "password", &service.password)?,
                "email": format!("local-exit-{}", service.id),
            }],
        }),
    };

    let mut inbound = json!({
        "tag": service.tag,
        "listen": ACCESS_INBOUND_LISTEN,
        "port": service.listen_port,
        "protocol": match service.protocol {
            LocalExitProtocol::Socks => "socks",
            LocalExitProtocol::Http => "http",
            LocalExitProtocol::Vless => "vless",
            LocalExitProtocol::Trojan => "trojan",
            LocalExitProtocol::Shadowsocks => "shadowsocks",
            LocalExitProtocol::Hysteria2 => "hysteria",
        },
        "settings": settings,
    });
    let stream_settings = local_exit_stream_settings(service)?;
    if !stream_settings.is_null() {
        inbound["streamSettings"] = stream_settings;
    }
    Ok(inbound)
}

fn local_exit_stream_settings(service: &LocalExitService) -> Result<Value, XrayConfigError> {
    match service.protocol {
        LocalExitProtocol::Vless => {
            let mut settings = json!({
                "network": default_if_empty(&service.network, "tcp"),
            });
            if service
                .security
                .as_deref()
                .is_some_and(|security| security.eq_ignore_ascii_case("reality"))
            {
                settings["security"] = json!("reality");
                settings["realitySettings"] = json!({
                    "show": false,
                    "dest": default_if_empty_option(service.reality_dest.as_deref(), "www.cloudflare.com:443"),
                    "xver": 0,
                    "serverNames": server_names(service.server_name.as_deref()),
                    "privateKey": default_if_empty_option(service.reality_private_key.as_deref(), ""),
                    "shortIds": non_empty_list_or_default(&service.reality_short_ids, ""),
                });
            } else if service
                .security
                .as_deref()
                .is_some_and(|security| security.eq_ignore_ascii_case("tls"))
            {
                // VLESS 本机出口走 TLS 必须带服务器证书,否则 Xray 完不成 TLS 握手、该出口 100% 不可用。
                // 此前只写 serverName、漏了 certificates(Trojan/HY2 出口都写了)——真机压测发现的产品 bug。
                // 比照 Trojan/HY2 出口:强制要求 tls_certificate_file/tls_key_file 并写入 certificates。
                let certificate_file = require_access_field(
                    &service.id,
                    "tls_certificate_file",
                    default_if_empty_option(service.tls_certificate_file.as_deref(), ""),
                )?;
                let key_file = require_access_field(
                    &service.id,
                    "tls_key_file",
                    default_if_empty_option(service.tls_key_file.as_deref(), ""),
                )?;
                settings["security"] = json!("tls");
                let mut tls_settings = Map::new();
                if let Some(server_name) = non_empty_option(service.server_name.as_deref()) {
                    tls_settings.insert("serverName".to_owned(), json!(server_name));
                }
                tls_settings.insert(
                    "certificates".to_owned(),
                    json!([{ "certificateFile": certificate_file, "keyFile": key_file }]),
                );
                settings["tlsSettings"] = Value::Object(tls_settings);
            }
            Ok(settings)
        }
        LocalExitProtocol::Trojan => local_exit_trojan_stream_settings(service),
        LocalExitProtocol::Hysteria2 => local_exit_hysteria2_stream_settings(service),
        _ => Ok(Value::Null),
    }
}

fn local_exit_trojan_stream_settings(service: &LocalExitService) -> Result<Value, XrayConfigError> {
    let certificate_file = require_access_field(
        &service.id,
        "tls_certificate_file",
        default_if_empty_option(service.tls_certificate_file.as_deref(), ""),
    )?;
    let key_file = require_access_field(
        &service.id,
        "tls_key_file",
        default_if_empty_option(service.tls_key_file.as_deref(), ""),
    )?;
    let mut tls_settings = Map::new();
    if let Some(server_name) = non_empty_option(service.server_name.as_deref()) {
        tls_settings.insert("serverName".to_owned(), json!(server_name));
    }
    tls_settings.insert(
        "certificates".to_owned(),
        json!([{ "certificateFile": certificate_file, "keyFile": key_file }]),
    );

    Ok(json!({
        "network": default_if_empty(&service.network, "tcp"),
        "security": "tls",
        "tlsSettings": tls_settings,
    }))
}

fn local_exit_hysteria2_stream_settings(
    service: &LocalExitService,
) -> Result<Value, XrayConfigError> {
    let certificate_file = require_access_field(
        &service.id,
        "tls_certificate_file",
        default_if_empty_option(service.tls_certificate_file.as_deref(), ""),
    )?;
    let key_file = require_access_field(
        &service.id,
        "tls_key_file",
        default_if_empty_option(service.tls_key_file.as_deref(), ""),
    )?;
    let mut tls_settings = Map::new();
    if let Some(server_name) = non_empty_option(service.server_name.as_deref()) {
        tls_settings.insert("serverName".to_owned(), json!(server_name));
    }
    tls_settings.insert("alpn".to_owned(), json!(["h3"]));
    tls_settings.insert(
        "certificates".to_owned(),
        json!([{ "certificateFile": certificate_file, "keyFile": key_file }]),
    );

    Ok(json!({
        "network": "hysteria",
        "security": "tls",
        "hysteriaSettings": {
            "version": 2,
        },
        "tlsSettings": tls_settings,
    }))
}

fn inbound_stream_settings(line: &AccessLine) -> Result<Value, XrayConfigError> {
    let transport = line.transport.trim().to_ascii_lowercase();
    let mut settings = match transport.as_str() {
        "" | "tcp" => json!({"network": "tcp"}),
        "xhttp" => json!({
            "network": "xhttp",
            "xhttpSettings": {
                "path": default_if_empty(&line.xhttp_path, "/xrayc"),
                "host": line.xhttp_host,
                "mode": default_if_empty(&line.xhttp_mode, "stream-one"),
            }
        }),
        "ws" => json!({
            "network": "ws",
            "wsSettings": {
                "path": default_if_empty(&line.xhttp_path, "/"),
                "headers": optional_host_header(&line.xhttp_host),
            }
        }),
        "grpc" => json!({
            "network": "grpc",
            "grpcSettings": {
                "serviceName": default_if_empty(&line.xhttp_path, "xrayc"),
            }
        }),
        "hysteria" => json!({
            "network": "hysteria",
            "hysteriaSettings": {
                "version": 2,
            }
        }),
        _ => Value::Null,
    };
    if settings.is_null() {
        return Ok(settings);
    }

    if let Some(security) = line
        .inbound_security
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if security.eq_ignore_ascii_case("reality") {
            // 官方 Xray 口径：VLESS Reality 仅支持 TCP/XHTTP/gRPC，不开放 WS/UDP/XUDP。
            // 入站此前对 transport 不做校验，会生成 Xray 拒绝的 Reality+WS 非法配置，
            // 这里补齐与出站对称的拦截，遇到 ws（或其它非法 transport）直接报错。
            reject_non_reality_transport(&line.id, &transport)?;
            if let Some(object) = settings.as_object_mut() {
                object.insert("security".to_owned(), json!("reality"));
                object.insert(
                    "realitySettings".to_owned(),
                    json!({
                        "show": false,
                        "dest": default_if_empty_option(line.reality_dest.as_deref(), "www.cloudflare.com:443"),
                        "xver": 0,
                        "serverNames": server_names(line.server_name.as_deref()),
                        "privateKey": default_if_empty_option(line.reality_private_key.as_deref(), ""),
                        "shortIds": non_empty_list_or_default(&line.reality_short_ids, ""),
                    }),
                );
            }
        } else if security.eq_ignore_ascii_case("tls") {
            apply_inbound_tls_settings(line, &mut settings)?;
        }
    }

    Ok(settings)
}

/// 校验 VLESS Reality 入站的传输层是否合法。
/// `transport` 已在调用方完成 trim + 小写归一；空串等价 TCP。
/// 仅放行 tcp/xhttp/grpc，其余（含 ws）按非法组合返回明确错误，
/// 复用出站侧的 `UnsupportedAccessProtocol` 口径以保持入站/出站对称。
fn reject_non_reality_transport(line_id: &str, transport: &str) -> Result<(), XrayConfigError> {
    let normalized = if transport.is_empty() {
        "tcp"
    } else {
        transport
    };
    if matches!(normalized, "tcp" | "xhttp" | "grpc") {
        return Ok(());
    }
    Err(XrayConfigError::UnsupportedAccessProtocol(format!(
        "{line_id}: VLESS Reality only supports TCP, XHTTP or gRPC"
    )))
}

fn apply_inbound_tls_settings(
    line: &AccessLine,
    settings: &mut Value,
) -> Result<(), XrayConfigError> {
    let Some(object) = settings.as_object_mut() else {
        return Ok(());
    };
    let certificate_file = require_access_field(
        &line.id,
        "tls_certificate_file",
        default_if_empty_option(line.tls_certificate_file.as_deref(), ""),
    )?;
    let key_file = require_access_field(
        &line.id,
        "tls_key_file",
        default_if_empty_option(line.tls_key_file.as_deref(), ""),
    )?;
    let mut tls_settings = Map::new();
    if let Some(server_name) = non_empty_option(line.server_name.as_deref()) {
        tls_settings.insert("serverName".to_owned(), json!(server_name));
    }
    if line.transport.trim().eq_ignore_ascii_case("hysteria") {
        // HY2 基于 QUIC，服务端必须声明 h3，否则客户端会因 ALPN 不匹配拒绝握手。
        tls_settings.insert("alpn".to_owned(), json!(["h3"]));
    }
    tls_settings.insert(
        "certificates".to_owned(),
        json!([{ "certificateFile": certificate_file, "keyFile": key_file }]),
    );
    object.insert("security".to_owned(), json!("tls"));
    object.insert("tlsSettings".to_owned(), Value::Object(tls_settings));
    Ok(())
}

fn server_names(server_name: Option<&str>) -> Vec<String> {
    server_name
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec!["www.cloudflare.com".to_owned()])
}

fn non_empty_list_or_default(values: &[String], default: &str) -> Vec<String> {
    let items = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if items.is_empty() {
        vec![default.to_owned()]
    } else {
        items
    }
}

fn optional_host_header(host: &str) -> Value {
    let host = host.trim();
    if host.is_empty() {
        json!({})
    } else {
        json!({ "Host": host })
    }
}

fn access_protocol_name(protocol: &AccessProtocol) -> &'static str {
    match protocol {
        AccessProtocol::Vless { .. } => "vless",
        AccessProtocol::Trojan => "trojan",
        AccessProtocol::Hysteria2 => "hysteria",
        AccessProtocol::Shadowsocks { .. } => "shadowsocks",
    }
}

fn shadowsocks_user_method(method: &str) -> Option<&str> {
    let method = method.trim();
    if method.starts_with("2022-") {
        None
    } else {
        Some(method)
    }
}

fn shadowsocks_user_password(method: &str, credential: &str) -> String {
    let method = method.trim();
    if !method.starts_with("2022-") {
        return credential.to_owned();
    }

    let key_len = if method.contains("aes-128") { 16 } else { 32 };
    let digest = Sha256::digest(credential.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(&digest[..key_len])
}

fn inbound_tag(line_id: &str) -> String {
    format!("access-{line_id}")
}
