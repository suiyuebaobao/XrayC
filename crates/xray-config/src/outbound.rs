//! 本模块负责生成 Xray 出站配置。
//! 出口配置覆盖代理协议、远端地址端口、认证信息和传输安全参数。
//! 主编译入口会为每个出口端点调用这里的生成函数。

use serde_json::{json, Map, Value};

use crate::util::{non_empty_option, require_field, require_port};
use crate::{ExitEndpoint, ExitProtocol, XrayConfigError};

pub(crate) fn compile_outbound(endpoint: &ExitEndpoint) -> Result<Value, XrayConfigError> {
    let mut value = match &endpoint.protocol {
        ExitProtocol::Direct => {
            json!({
                "tag": endpoint.tag,
                "protocol": "freedom",
                "settings": {},
            })
        }
        ExitProtocol::Socks {
            address,
            port,
            username,
            password,
        } => json!({
            "tag": endpoint.tag,
            "protocol": "socks",
            "settings": {
                "servers": [{
                    "address": require_field(&endpoint.id, "address", address)?,
                    "port": require_port(&endpoint.id, *port)?,
                    "users": outbound_users(&endpoint.id, username, password)?,
                }]
            },
        }),
        ExitProtocol::Http {
            address,
            port,
            username,
            password,
        } => json!({
            "tag": endpoint.tag,
            "protocol": "http",
            "settings": {
                "servers": [{
                    "address": require_field(&endpoint.id, "address", address)?,
                    "port": require_port(&endpoint.id, *port)?,
                    "users": outbound_users(&endpoint.id, username, password)?,
                }]
            },
        }),
        ExitProtocol::Vless {
            address,
            port,
            uuid,
            security,
            flow,
            server_name,
            public_key,
            short_id,
            fingerprint,
            network,
            xhttp_path,
            xhttp_host,
            xhttp_mode,
            udp_packet_encoding,
        } => {
            validate_vless_reality_transport(
                &endpoint.id,
                security.as_deref(),
                network.as_deref(),
                udp_packet_encoding.as_deref(),
            )?;
            let mut user = json!({
                "id": require_field(&endpoint.id, "uuid", uuid)?,
                "encryption": "none",
            });
            if let Some(flow) = non_empty_option(flow.as_deref()) {
                user["flow"] = json!(flow);
            }

            let mut outbound = json!({
                "tag": endpoint.tag,
                "protocol": "vless",
                "settings": {
                    "vnext": [{
                        "address": require_field(&endpoint.id, "address", address)?,
                        "port": require_port(&endpoint.id, *port)?,
                        "users": [user]
                    }]
                },
                "streamSettings": stream_settings(
                    &endpoint.id,
                    VlessStreamOptions {
                        security: security.as_deref(),
                        server_name: server_name.as_deref(),
                        public_key: public_key.as_deref(),
                        short_id: short_id.as_deref(),
                        fingerprint: fingerprint.as_deref(),
                        network: network.as_deref(),
                        xhttp_path: xhttp_path.as_deref(),
                        xhttp_host: xhttp_host.as_deref(),
                        xhttp_mode: xhttp_mode.as_deref(),
                    },
                )?,
            });
            if udp_packet_encoding
                .as_deref()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("xudp"))
            {
                outbound["mux"] = json!({
                    "enabled": true,
                    "xudpConcurrency": 16,
                });
            }
            outbound
        }
        ExitProtocol::Trojan {
            address,
            port,
            password,
            security,
            server_name,
        } => {
            let security = security
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("tls");
            json!({
                "tag": endpoint.tag,
                "protocol": "trojan",
                "settings": {
                    "servers": [{
                        "address": require_field(&endpoint.id, "address", address)?,
                        "port": require_port(&endpoint.id, *port)?,
                        "password": require_field(&endpoint.id, "password", password)?,
                    }]
                },
                "streamSettings": trojan_stream_settings(
                    &endpoint.id,
                    security,
                    server_name.as_deref(),
                )?,
            })
        }
        ExitProtocol::Shadowsocks {
            address,
            port,
            method,
            password,
        } => json!({
            "tag": endpoint.tag,
            "protocol": "shadowsocks",
            "settings": {
                "address": require_field(&endpoint.id, "address", address)?,
                "port": require_port(&endpoint.id, *port)?,
                "method": require_field(&endpoint.id, "method", method)?,
                "password": require_field(&endpoint.id, "password", password)?,
            },
        }),
        ExitProtocol::Hysteria2 {
            address,
            port,
            password,
            server_name,
            allow_insecure: _,
        } => json!({
            "tag": endpoint.tag,
            "protocol": "hysteria",
            "settings": {
                "version": 2,
                "address": require_field(&endpoint.id, "address", address)?,
                "port": require_port(&endpoint.id, *port)?,
            },
            "streamSettings": hysteria2_stream_settings(
                &endpoint.id,
                password,
                server_name.as_deref(),
            )?,
        }),
    };

    apply_sockopt_mark(&mut value, endpoint.sockopt_mark);
    Ok(value)
}

fn apply_sockopt_mark(value: &mut Value, mark: Option<u32>) {
    let Some(mark) = mark.filter(|mark| *mark != 0) else {
        return;
    };
    if !value.get("streamSettings").is_some_and(Value::is_object) {
        value["streamSettings"] = json!({});
    }
    if !value["streamSettings"]
        .get("sockopt")
        .is_some_and(Value::is_object)
    {
        value["streamSettings"]["sockopt"] = json!({});
    }
    value["streamSettings"]["sockopt"]["mark"] = json!(mark);
}

fn validate_vless_reality_transport(
    endpoint_id: &str,
    security: Option<&str>,
    network: Option<&str>,
    udp_packet_encoding: Option<&str>,
) -> Result<(), XrayConfigError> {
    let is_reality = security
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("reality"));
    if !is_reality {
        return Ok(());
    }
    if udp_packet_encoding
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("xudp"))
    {
        return Err(XrayConfigError::UnsupportedExitProtocol(format!(
            "{endpoint_id}: VLESS Reality does not support XUDP"
        )));
    }
    let normalized_network = network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tcp");
    // 官方 Xray 口径：VLESS Reality 仅支持 TCP/XHTTP/gRPC，不开放 WS/UDP/XUDP。
    // 这里继续拒绝 WS 等非法组合，避免静默编译出 Xray 不接受的 Reality+WS 出口。
    if !matches!(
        normalized_network.to_ascii_lowercase().as_str(),
        "tcp" | "xhttp" | "grpc"
    ) {
        return Err(XrayConfigError::UnsupportedExitProtocol(format!(
            "{endpoint_id}: VLESS Reality only supports TCP, XHTTP or gRPC"
        )));
    }
    Ok(())
}

fn outbound_users(
    endpoint_id: &str,
    username: &Option<String>,
    password: &Option<String>,
) -> Result<Value, XrayConfigError> {
    match (
        non_empty_option(username.as_deref()),
        non_empty_option(password.as_deref()),
    ) {
        (Some(user), Some(pass)) => Ok(json!([{ "user": user, "pass": pass }])),
        (None, None) => Ok(json!([])),
        (None, Some(_)) => Err(XrayConfigError::MissingExitField(
            endpoint_id.to_owned(),
            "username",
        )),
        (Some(_), None) => Err(XrayConfigError::MissingExitField(
            endpoint_id.to_owned(),
            "password",
        )),
    }
}

fn stream_settings(
    endpoint_id: &str,
    options: VlessStreamOptions<'_>,
) -> Result<Value, XrayConfigError> {
    let security = options
        .security
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let mut settings = match security.as_deref() {
        Some("tls") => tls_stream_settings(options.server_name, options.fingerprint),
        Some("none") => json!({}),
        Some("reality") => json!({
            "security": "reality",
            "realitySettings": {
                "serverName": require_field(
                    endpoint_id,
                    "server_name",
                    options.server_name.unwrap_or_default(),
                )?,
                "fingerprint": non_empty_option(options.fingerprint).unwrap_or("chrome"),
                "publicKey": require_field(
                    endpoint_id,
                    "public_key",
                    options.public_key.unwrap_or_default(),
                )?,
                "shortId": non_empty_option(options.short_id).unwrap_or_default(),
            },
        }),
        Some(value) => {
            return Err(XrayConfigError::UnsupportedExitProtocol(format!(
                "vless security {value}"
            )));
        }
        None => json!({}),
    };
    apply_vless_transport_settings(&mut settings, options);
    Ok(settings)
}

#[derive(Clone, Copy)]
struct VlessStreamOptions<'a> {
    security: Option<&'a str>,
    server_name: Option<&'a str>,
    public_key: Option<&'a str>,
    short_id: Option<&'a str>,
    fingerprint: Option<&'a str>,
    network: Option<&'a str>,
    xhttp_path: Option<&'a str>,
    xhttp_host: Option<&'a str>,
    xhttp_mode: Option<&'a str>,
}

fn apply_vless_transport_settings(settings: &mut Value, options: VlessStreamOptions<'_>) {
    let network = options
        .network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    // tcp/空走默认 TCP，不写 network 字段；其余传输层必须显式落 network 与对应配置，
    // 否则 gRPC/WS 出口会被 Xray 静默当 TCP 编译，导致与服务端传输层不匹配。
    let Some(object) = settings.as_object_mut() else {
        return;
    };
    match network.as_deref() {
        Some("xhttp") => {
            object.insert("network".to_owned(), json!("xhttp"));
            object.insert(
                "xhttpSettings".to_owned(),
                json!({
                    "path": non_empty_option(options.xhttp_path).unwrap_or("/xrayc"),
                    "host": non_empty_option(options.xhttp_host).unwrap_or_default(),
                    "mode": non_empty_option(options.xhttp_mode).unwrap_or("stream-one"),
                }),
            );
        }
        Some("grpc") => {
            // gRPC 出站口径对齐入站：serviceName 复用 xhttp_path，默认 "xrayc"。
            object.insert("network".to_owned(), json!("grpc"));
            object.insert(
                "grpcSettings".to_owned(),
                json!({
                    "serviceName": non_empty_option(options.xhttp_path).unwrap_or("xrayc"),
                }),
            );
        }
        Some("ws") => {
            // WS 出站口径对齐入站：path 复用 xhttp_path（默认 "/"），host 落入 Host 头。
            object.insert("network".to_owned(), json!("ws"));
            object.insert(
                "wsSettings".to_owned(),
                json!({
                    "path": non_empty_option(options.xhttp_path).unwrap_or("/"),
                    "headers": ws_host_header(options.xhttp_host),
                }),
            );
        }
        _ => {}
    }
}

/// 出站 WS 的 Host 头：非空 host 写入 `Host`，空 host 留空对象，口径与入站保持一致。
fn ws_host_header(host: Option<&str>) -> Value {
    match non_empty_option(host) {
        Some(host) => json!({ "Host": host }),
        None => json!({}),
    }
}

fn trojan_stream_settings(
    endpoint_id: &str,
    security: &str,
    server_name: Option<&str>,
) -> Result<Value, XrayConfigError> {
    match security.trim().to_ascii_lowercase().as_str() {
        "tls" => Ok(tls_stream_settings(
            Some(require_field(
                endpoint_id,
                "server_name",
                server_name.unwrap_or_default(),
            )?),
            None,
        )),
        value => Err(XrayConfigError::UnsupportedExitProtocol(format!(
            "trojan security {value}; trojan outbound must use tls"
        ))),
    }
}

fn tls_stream_settings(server_name: Option<&str>, fingerprint: Option<&str>) -> Value {
    let mut tls_settings = Map::new();
    if let Some(server_name) = non_empty_option(server_name) {
        tls_settings.insert("serverName".to_owned(), json!(server_name));
    }
    if let Some(fingerprint) = non_empty_option(fingerprint) {
        tls_settings.insert("fingerprint".to_owned(), json!(fingerprint));
    }
    json!({
        "security": "tls",
        "tlsSettings": tls_settings,
    })
}

fn hysteria2_stream_settings(
    endpoint_id: &str,
    password: &str,
    server_name: Option<&str>,
) -> Result<Value, XrayConfigError> {
    let mut settings = tls_stream_settings(server_name, None);
    if let Some(object) = settings.as_object_mut() {
        object.insert("network".to_owned(), json!("hysteria"));
        object.insert(
            "hysteriaSettings".to_owned(),
            json!({
                "version": 2,
                "auth": require_field(endpoint_id, "password", password)?,
            }),
        );
    }
    Ok(settings)
}
