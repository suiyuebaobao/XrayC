//! 本机出口服务渲染辅助模块。
//! 这里把本机出口 endpoint 转成 access-agent 可启动的本地服务。
//! 模块只读取 StoreData 快照，不访问数据库或修改状态。
//! 本机出口只服务当前 owner_access_node_id 对应的中转节点。
//! direct endpoint 不渲染为本机出口服务。
//! 协议字段继续复用 xray_protocol 的配置读取工具。
//! TLS 和 Reality 必填项在这里做最小运行前校验。
//! 输出顺序保持 listen_port 和 id 稳定排序。
//! 本文件由 xray_render 拆分而来，避免主渲染文件膨胀。
//! 本头部满足前十行中文注释约束。

use uuid::Uuid;
use xrayc_core::{EndpointType, ExitEndpoint, StoreData};
use xrayc_xray_config::{
    LocalExitProtocol as XrayLocalExitProtocol, LocalExitService as XrayLocalExitService,
};

use crate::validation::{TLS_CERTIFICATE_KEYS, TLS_KEY_KEYS};
use crate::xray_protocol::config_text;

pub(crate) fn local_exit_services_for_node(
    data: &StoreData,
    node_id: Uuid,
) -> Vec<XrayLocalExitService> {
    let mut services = data
        .local_exit_endpoints
        .iter()
        .filter(|endpoint| endpoint.owner_access_node_id == Some(node_id) && endpoint.healthy)
        .filter_map(local_exit_service_for_endpoint)
        .collect::<Vec<_>>();
    services.sort_by(|left, right| {
        (left.listen_port, left.id.as_str()).cmp(&(right.listen_port, right.id.as_str()))
    });
    services
}

fn local_exit_service_for_endpoint(endpoint: &ExitEndpoint) -> Option<XrayLocalExitService> {
    let protocol = match endpoint.outbound_type {
        EndpointType::Socks => XrayLocalExitProtocol::Socks,
        EndpointType::Http => XrayLocalExitProtocol::Http,
        EndpointType::Vless => XrayLocalExitProtocol::Vless,
        EndpointType::Trojan => XrayLocalExitProtocol::Trojan,
        EndpointType::Shadowsocks => XrayLocalExitProtocol::Shadowsocks,
        EndpointType::Hysteria => XrayLocalExitProtocol::Hysteria2,
        EndpointType::Direct => return None,
    };
    let password =
        config_text(&endpoint.outbound_config, &["password", "pass", "auth"]).unwrap_or_default();
    let uuid = config_text(&endpoint.outbound_config, &["uuid", "id"]).unwrap_or_default();
    let security = config_text(&endpoint.outbound_config, &["security"])
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if matches!(
        protocol,
        XrayLocalExitProtocol::Socks | XrayLocalExitProtocol::Http
    ) && (config_text(&endpoint.outbound_config, &["username", "user"]).is_none()
        || password.is_empty())
    {
        return None;
    }
    if protocol == XrayLocalExitProtocol::Vless && uuid.is_empty() {
        return None;
    }
    if matches!(
        protocol,
        XrayLocalExitProtocol::Trojan
            | XrayLocalExitProtocol::Shadowsocks
            | XrayLocalExitProtocol::Hysteria2
    ) && password.is_empty()
    {
        return None;
    }
    if security.as_deref() == Some("reality")
        && config_text(
            &endpoint.outbound_config,
            &[
                "private_key",
                "privateKey",
                "reality_private_key",
                "realityPrivateKey",
            ],
        )
        .is_none()
    {
        return None;
    }
    let tls_certificate_file = config_text(&endpoint.outbound_config, TLS_CERTIFICATE_KEYS);
    let tls_key_file = config_text(&endpoint.outbound_config, TLS_KEY_KEYS);
    // 需要 TLS 服务器证书的本机出口:Trojan/HY2 一律要;VLESS 仅 security=tls 时要(reality 用密钥、不需证书)。
    // 缺 cert/key 就拒绝建该出口,避免渲染出"security=tls 却无证书"的不可用入站——VLESS 此前漏判,
    // 与 inbound.rs 漏写 certificates 两层一致地把 VLESS-TLS 本机出口证书整条漏掉(真机压测发现)。
    let needs_tls_certificate = matches!(
        protocol,
        XrayLocalExitProtocol::Trojan | XrayLocalExitProtocol::Hysteria2
    ) || (protocol == XrayLocalExitProtocol::Vless
        && security.as_deref() == Some("tls"));
    if needs_tls_certificate && (tls_certificate_file.is_none() || tls_key_file.is_none()) {
        return None;
    }

    Some(XrayLocalExitService {
        id: endpoint.id.to_string(),
        tag: local_exit_service_tag(endpoint.id),
        listen_port: endpoint.port,
        protocol,
        network: local_exit_runtime_network(endpoint, protocol),
        username: config_text(&endpoint.outbound_config, &["username", "user"]).unwrap_or_default(),
        password,
        uuid,
        method: config_text(&endpoint.outbound_config, &["method", "cipher"])
            .unwrap_or_else(|| "aes-256-gcm".to_string()),
        security,
        server_name: config_text(
            &endpoint.outbound_config,
            &["server_name", "servername", "sni"],
        ),
        reality_dest: config_text(
            &endpoint.outbound_config,
            &["dest", "reality_dest", "realityDest"],
        )
        .or_else(|| {
            config_text(
                &endpoint.outbound_config,
                &["server_name", "servername", "sni"],
            )
            .map(|server_name| format!("{server_name}:443"))
        }),
        reality_private_key: config_text(
            &endpoint.outbound_config,
            &[
                "private_key",
                "privateKey",
                "reality_private_key",
                "realityPrivateKey",
            ],
        ),
        reality_short_ids: config_text(
            &endpoint.outbound_config,
            &["short_id", "shortId", "reality_short_id", "realityShortId"],
        )
        .into_iter()
        .collect(),
        tls_certificate_file,
        tls_key_file,
    })
}

fn local_exit_service_tag(endpoint_id: Uuid) -> String {
    format!("local-exit-{endpoint_id}")
}

fn local_exit_runtime_network(endpoint: &ExitEndpoint, protocol: XrayLocalExitProtocol) -> String {
    let network = config_text(
        &endpoint.stream_config,
        &["network", "network_mode", "networkMode"],
    )
    .unwrap_or_else(|| {
        if protocol == XrayLocalExitProtocol::Hysteria2 {
            "hysteria".to_string()
        } else {
            "tcp".to_string()
        }
    });
    if network.trim().eq_ignore_ascii_case("xudp") {
        "tcp".to_string()
    } else {
        network
    }
}
