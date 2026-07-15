//! 本模块负责中转入口和出口端点探测。
//! 探测结果只携带配置标识、健康状态、延迟和脱敏错误摘要，用于控制面
//! 观察节点实时可用性，不泄露上游凭据或原始目标细节。

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use tokio::net::{lookup_host, TcpStream, UdpSocket};
use tokio::time;
use xrayc_xray_config::{AccessConfig, AccessLine, ExitProtocol};

use crate::client::{AccessExitProbe, AccessLineProbe, ProbeTask, ProbeTaskTarget};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const UDP_REFUSED_WAIT: Duration = Duration::from_millis(150);

pub(super) async fn collect_probe_results(
    config: &AccessConfig,
    probed_at_unix: i64,
) -> (Vec<AccessLineProbe>, Vec<AccessExitProbe>) {
    let mut line_probes = Vec::new();
    let mut exit_probes = BTreeMap::<String, AccessExitProbe>::new();
    for endpoint in &config.exit_endpoints {
        let outcome = probe_exit_endpoint(&endpoint.protocol).await;
        let report_id = report_exit_endpoint_id(&endpoint.id);
        exit_probes.insert(
            report_id.to_owned(),
            exit_probe_record(report_id, probed_at_unix, outcome),
        );
    }
    for line in &config.access_lines {
        let outcome = probe_access_line(line).await;
        let line_probe = line_probe_record(line.report_line_id(), probed_at_unix, outcome.clone());
        line_probes.push(line_probe);
    }
    (line_probes, exit_probes.into_values().collect())
}

pub(super) async fn collect_probe_task_results(
    config: Option<&AccessConfig>,
    tasks: &[ProbeTask],
    probed_at_unix: i64,
) -> (Vec<AccessLineProbe>, Vec<AccessExitProbe>) {
    let endpoints_by_id = config
        .map(|config| {
            let mut endpoints = HashMap::new();
            for endpoint in &config.exit_endpoints {
                endpoints.entry(endpoint.id.as_str()).or_insert(endpoint);
                endpoints
                    .entry(report_exit_endpoint_id(&endpoint.id))
                    .or_insert(endpoint);
            }
            endpoints
        })
        .unwrap_or_default();
    let mut exit_probes = Vec::new();
    for task in tasks {
        let probe = match task.target.as_ref() {
            Some(target) => {
                let outcome = probe_task_target(target).await;
                exit_probe_record(&task.exit_endpoint_id, probed_at_unix, outcome)
            }
            None => match endpoints_by_id.get(task.exit_endpoint_id.as_str()) {
                Some(endpoint) => {
                    let outcome = probe_exit_endpoint(&endpoint.protocol).await;
                    exit_probe_record(&task.exit_endpoint_id, probed_at_unix, outcome)
                }
                None => AccessExitProbe {
                    exit_endpoint_id: task.exit_endpoint_id.clone(),
                    status: "unknown".to_owned(),
                    latency_ms: None,
                    error_summary: "手动探测目标不在当前配置中".to_owned(),
                    probed_at_unix,
                },
            },
        };
        exit_probes.push(probe);
    }
    (Vec::new(), exit_probes)
}

fn report_exit_endpoint_id(endpoint_id: &str) -> &str {
    endpoint_id
        .split_once(':')
        .map(|(source_endpoint_id, _)| source_endpoint_id)
        .filter(|source_endpoint_id| !source_endpoint_id.trim().is_empty())
        .unwrap_or(endpoint_id)
}

#[derive(Debug, Clone)]
pub(super) struct ProbeOutcome {
    pub(super) status: &'static str,
    pub(super) latency_ms: Option<u64>,
    pub(super) message: Option<String>,
}

async fn probe_exit_endpoint(protocol: &ExitProtocol) -> ProbeOutcome {
    let Some((address, port)) = exit_protocol_target(protocol) else {
        return ProbeOutcome {
            status: "healthy",
            latency_ms: None,
            message: Some("direct 出口无需远端 TCP 拨测".to_owned()),
        };
    };

    if matches!(protocol, ExitProtocol::Hysteria2 { .. }) {
        return probe_udp_target(address, port, "HY2 出口").await;
    }

    probe_tcp_target(address, port, "TCP").await
}

async fn probe_tcp_target(address: &str, port: u16, label: &str) -> ProbeOutcome {
    let started = Instant::now();
    match time::timeout(PROBE_TIMEOUT, TcpStream::connect((address, port))).await {
        Ok(Ok(_stream)) => ProbeOutcome {
            status: "healthy",
            latency_ms: Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
            message: None,
        },
        Ok(Err(error)) => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!(
                "{label} 拨测失败: {}",
                sanitize_probe_error(&error)
            )),
        },
        Err(_) => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!("{label} 拨测超时")),
        },
    }
}

async fn probe_udp_target(address: &str, port: u16, label: &str) -> ProbeOutcome {
    let started = Instant::now();
    let target = match time::timeout(PROBE_TIMEOUT, lookup_host((address, port))).await {
        Ok(Ok(mut addrs)) => match addrs.next() {
            Some(addr) => addr,
            None => {
                return ProbeOutcome {
                    status: "unhealthy",
                    latency_ms: None,
                    message: Some(format!("{label} UDP 拨测失败: 目标不可解析")),
                };
            }
        },
        Ok(Err(error)) => {
            return ProbeOutcome {
                status: "unhealthy",
                latency_ms: None,
                message: Some(format!(
                    "{label} UDP 拨测失败: {}",
                    sanitize_probe_error(&error)
                )),
            };
        }
        Err(_) => {
            return ProbeOutcome {
                status: "unhealthy",
                latency_ms: None,
                message: Some(format!("{label} UDP 解析超时")),
            };
        }
    };
    let bind_addr = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = match UdpSocket::bind(bind_addr).await {
        Ok(socket) => socket,
        Err(error) => {
            return ProbeOutcome {
                status: "unhealthy",
                latency_ms: None,
                message: Some(format!(
                    "{label} UDP 拨测失败: {}",
                    sanitize_probe_error(&error)
                )),
            };
        }
    };
    let send_result = async {
        socket.connect(target).await?;
        socket.send(b"xrayc-probe").await?;
        Ok::<(), std::io::Error>(())
    };
    match time::timeout(PROBE_TIMEOUT, send_result).await {
        Ok(Ok(())) => udp_probe_outcome_after_send(&socket, started, label).await,
        Ok(Err(error)) => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!(
                "{label} UDP 拨测失败: {}",
                sanitize_probe_error(&error)
            )),
        },
        Err(_) => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!("{label} UDP 拨测超时")),
        },
    }
}

async fn udp_probe_outcome_after_send(
    socket: &UdpSocket,
    started: Instant,
    label: &str,
) -> ProbeOutcome {
    let mut buf = [0_u8; 32];
    match time::timeout(UDP_REFUSED_WAIT, socket.recv(&mut buf)).await {
        Ok(Ok(_)) | Err(_) => ProbeOutcome {
            status: "healthy",
            latency_ms: Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
            message: None,
        },
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!("{label} UDP 拨测失败: 目标端口未响应")),
        },
        Ok(Err(error)) => ProbeOutcome {
            status: "unhealthy",
            latency_ms: None,
            message: Some(format!(
                "{label} UDP 拨测失败: {}",
                sanitize_probe_error(&error)
            )),
        },
    }
}

async fn probe_task_target(target: &ProbeTaskTarget) -> ProbeOutcome {
    if protocol_uses_udp_probe(&target.protocol) {
        return probe_udp_target(&target.address, target.port, "HY2 出口").await;
    }
    probe_tcp_target(&target.address, target.port, "出口").await
}

fn protocol_uses_udp_probe(protocol: &str) -> bool {
    matches!(
        protocol.trim().to_ascii_lowercase().as_str(),
        "hysteria" | "hysteria2" | "hy2"
    )
}

pub(super) async fn probe_access_line(line: &AccessLine) -> ProbeOutcome {
    let address = access_line_probe_host(&line.listen_host);
    if matches!(line.protocol, xrayc_xray_config::AccessProtocol::Hysteria2)
        || line.transport.eq_ignore_ascii_case("hysteria")
    {
        return probe_udp_target(address, line.listen_port, "HY2 入口").await;
    }
    probe_tcp_target(address, line.listen_port, "入口").await
}

pub(super) fn access_line_probe_host(listen_host: &str) -> &str {
    match listen_host.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        host => host,
    }
}

pub(super) fn line_probe_record(
    access_line_id: &str,
    probed_at_unix: i64,
    outcome: ProbeOutcome,
) -> AccessLineProbe {
    AccessLineProbe {
        access_line_id: access_line_id.to_owned(),
        status: outcome.status.to_owned(),
        latency_ms: outcome.latency_ms,
        error_summary: outcome.message.unwrap_or_default(),
        probed_at_unix,
    }
}

pub(super) fn exit_probe_record(
    exit_endpoint_id: &str,
    probed_at_unix: i64,
    outcome: ProbeOutcome,
) -> AccessExitProbe {
    AccessExitProbe {
        exit_endpoint_id: exit_endpoint_id.to_owned(),
        status: outcome.status.to_owned(),
        latency_ms: outcome.latency_ms,
        error_summary: outcome.message.unwrap_or_default(),
        probed_at_unix,
    }
}

fn exit_protocol_target(protocol: &ExitProtocol) -> Option<(&str, u16)> {
    match protocol {
        ExitProtocol::Direct => None,
        ExitProtocol::Socks { address, port, .. }
        | ExitProtocol::Http { address, port, .. }
        | ExitProtocol::Vless { address, port, .. }
        | ExitProtocol::Trojan { address, port, .. }
        | ExitProtocol::Shadowsocks { address, port, .. }
        | ExitProtocol::Hysteria2 { address, port, .. } => Some((address.as_str(), *port)),
    }
}

fn sanitize_probe_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "连接被拒绝".to_owned(),
        std::io::ErrorKind::TimedOut => "连接超时".to_owned(),
        std::io::ErrorKind::NotFound => "目标不可解析".to_owned(),
        _ => "连接失败".to_owned(),
    }
}
