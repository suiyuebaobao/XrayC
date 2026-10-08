//! 本文件实现 access-agent 运行时主循环。
//! 运行时轮询控制面、写入 Xray 配置、回报应用结果，并把累计流量
//! 快照上报给中心计费。

mod apply;
mod apply_config;
mod apply_failure_summary;
mod cache;
mod core_control;
mod kernel;
mod limiter;
mod metrics;
mod node_metrics;
mod probes;
mod reboot;
mod reporting;
mod sessions;
mod split;
#[cfg(test)]
mod tests;
mod tls;
mod traffic_backlog;
mod utils;

use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::time;
use tracing::{info, warn};
use xrayc_xray_config::AccessConfig;

use crate::client::{
    AgentClient, HeartbeatRequest, ProbeTask, RuntimeCoreControlResult, RuntimeCoreTask,
};
use crate::config::AgentSettings;

use apply::apply_control_plane_config;
use cache::load_active_config_cache;
use core_control::execute_runtime_core_task;
use kernel::{detect_kernel_capability, should_attempt_kernel_install, KernelCapability};
use metrics::MetricsState;
use node_metrics::NodeMetricsState;
use reboot::execute_reboot_task;
use reporting::{collect_and_report_runtime, report_probe_tasks_snapshot};
use sessions::SessionState;
use tls::{
    collect_tls_certificates, execute_node_domains_sign, execute_tls_renew_task, SignBackoff,
};
use traffic_backlog::TrafficBacklog;
use utils::{hostname, unix_now};

const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 内核降级态下自动装内核的退避窗口:期间不重复跑 apt-get,防 dpkg 锁争用与 apt 网络风暴(P1-3)。
const KERNEL_INSTALL_BACKOFF: Duration = Duration::from_secs(3600);

/// 探测内核能力,并按退避窗口决定是否(再)尝试自动装内核——避免降级期间每心跳都跑 apt(P1-3)。
/// 仅当「本轮确实处于 connmark 降级且退避允许装」时推进退避时刻;connmark 恢复正常不推进。
async fn refresh_kernel_capability(
    settings: &AgentSettings,
    previous: KernelCapability,
    last_install_at: &mut Option<Instant>,
) -> KernelCapability {
    let attempt_install = should_attempt_kernel_install(
        last_install_at.map(|at| at.elapsed()),
        KERNEL_INSTALL_BACKOFF,
    );
    let capability = detect_kernel_capability(settings, previous, attempt_install).await;
    if attempt_install && !capability.connmark_available {
        // 本轮在降级态下允许并触发了一次装内核尝试,推进退避窗口,防下个心跳又跑 apt。
        *last_install_at = Some(Instant::now());
    }
    capability
}

pub async fn run_agent(settings: AgentSettings) -> anyhow::Result<()> {
    let client = AgentClient::new(&settings.control_plane_url, &settings.node_token)
        .context("create control plane HTTP client")?;
    let start = Instant::now();
    let cached_config = match load_active_config_cache(&settings) {
        Ok(cache) => cache,
        Err(error) => {
            warn!(%error, "active config cache restore failed");
            None
        }
    };
    let mut applied_config_version = cached_config
        .as_ref()
        .and_then(|cache| cache.config_version.clone());
    let mut active_config = cached_config.and_then(|cache| cache.config);
    let mut metrics_state = MetricsState::default();
    // 监控中心宿主指标采集状态(阶段B):loop 外声明,跨心跳保留上次 CPU 采样以算占用率。
    let mut node_metrics_state = NodeMetricsState::default();
    let mut session_state = SessionState::default();
    let mut traffic_backlog = TrafficBacklog::load(&settings.traffic_backlog_path);
    let mut heartbeat = time::interval(settings.heartbeat_interval());
    let mut traffic = time::interval(settings.traffic_interval());
    let mut pending_tls_renew_result = None;
    let mut pending_runtime_core_results = Vec::new();
    // 内核能力自检(§7.7.1):启动时探测一次,connmark 不可用即装新内核(不重启)。
    // 后续每次心跳前若仍处降级态再探测一次,管理员重启换新内核后即可自动恢复下行整形。
    // last_kernel_install_at 记录降级态下上次装内核时刻,配合退避避免每心跳都跑 apt(P1-3 修复)。
    let mut last_kernel_install_at: Option<Instant> = None;
    let mut kernel_capability = refresh_kernel_capability(
        &settings,
        KernelCapability::default(),
        &mut last_kernel_install_at,
    )
    .await;
    // 跨心跳保留待回报的整机重启结果:agent 执行 reboot 任务后存这里,下次心跳带回控制面。
    // 注:reboot 命令真生效时进程会随宿主重启而终止,这里保存的多是自检未过/无权限的失败结果。
    let mut pending_reboot_result: Option<crate::client::RebootResult> = None;
    // 按需自签的失败退避状态:跨心跳保留每域名上次签发失败时刻,
    // 退避窗口内不对同域名重试,避免每 30s 心跳锤 Let's Encrypt 触发限频。
    let mut sign_backoff = SignBackoff::default();
    // 跨心跳保留上次控制面下发的节点 node_domains 清单:本次心跳采集证书状态时把它纳入,
    // 让 PUT 新加、还没建入口的域名也被上报状态,供控制面刷新 cert_status(修 #60)。
    // 心跳响应里的 node_domains 在本次响应解析后才拿到,故用于「下一次」心跳的采集。
    let mut last_node_domains: Vec<crate::client::NodeDomain> = Vec::new();

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                // 仍处内核降级态时每次心跳重新探测:管理员重启换上新内核后,
                // connmark 恢复可用即自动撤销降级、复位待重启标记,无需重启 agent。
                if !kernel_capability.connmark_available {
                    kernel_capability = refresh_kernel_capability(
                        &settings,
                        kernel_capability,
                        &mut last_kernel_install_at,
                    )
                    .await;
                }
                // 限速整形自愈:整形被外部拆掉(降级恢复/网卡变动/手动清/重启后配置未变、
                // 中心不再下发 config)时,不等配置变化就在本心跳幂等重建,杜绝限速静默失效;
                // 整形完好则只探测不动(不干扰在途流量)。用上一轮生效配置的限速项。
                if let Some(cfg) = active_config.as_ref() {
                    limiter::ensure_limiter_shaping(
                        &settings,
                        &cfg.rate_limits,
                        kernel_capability.connmark_available,
                    )
                    .await;
                }
                let runtime_core_results = std::mem::take(&mut pending_runtime_core_results);
                // 心跳前采集一次宿主指标:任一项失败软失败返回 None,不影响心跳照常发出。
                let node_metrics = node_metrics_state.collect(&settings);
                match send_heartbeat(
                    &client,
                    &settings,
                    start.elapsed(),
                    applied_config_version.as_deref(),
                    active_config.as_ref(),
                    pending_tls_renew_result.take(),
                    runtime_core_results.clone(),
                    &last_node_domains,
                    kernel_capability,
                    pending_reboot_result.take(),
                    node_metrics,
                ).await {
                    Ok(outcome) => {
                        // 记下本次控制面下发的节点域名清单,供下一次心跳采集证书状态时纳入(修 #60)。
                        last_node_domains = outcome.node_domains.clone();
                        if let Some(version) = outcome.applied_config_version {
                            applied_config_version = Some(version);
                        }
                        if outcome.replace_active_config {
                            active_config = outcome.active_config;
                            // 只丢弃新配置里已不存在的用户/线路对应的过期速率基线，
                            // 仍在配置中的用户保留累计基线，避免每次带配置心跳把
                            // /metrics 实时速率清成 0（见 metrics::retain_for_active_config）。
                            metrics_state.retain_for_active_config(active_config.as_ref());
                        }
                        if !outcome.probe_tasks.is_empty() {
                            if let Err(error) = report_probe_tasks_snapshot(
                                &client,
                                &settings,
                                active_config.as_ref(),
                                &outcome.probe_tasks,
                                unix_now() as i64,
                            )
                            .await
                            {
                                warn!(%error, "manual probe task report failed");
                            }
                        }
                        if let Some(task) = outcome.tls_renew_task {
                            // 显式续期/签发任务:走逐域名签发(内部复用按需自签逻辑 + 退避)。
                            match execute_tls_renew_task(
                                &settings,
                                task,
                                &outcome.node_domains,
                                &mut sign_backoff,
                            )
                            .await
                            {
                                Ok(result) => pending_tls_renew_result = Some(result),
                                Err(error) => warn!(%error, "TLS certificate renewal failed"),
                            }
                        } else if !outcome.node_domains.is_empty() {
                            // 按需自签:无显式任务但有节点域名清单时,每次心跳核验逐域名,
                            // 只签缺的/过期的(x509 -checkend 有效即跳过),失败进退避,
                            // 使「编辑加域名→下次心跳自动签」成立。结果不回报(无 request_id)。
                            if let Err(error) = execute_node_domains_sign(
                                &settings,
                                &outcome.node_domains,
                                &mut sign_backoff,
                                "heartbeat-auto-sign",
                            )
                            .await
                            {
                                warn!(%error, "on-demand certificate sign failed");
                            }
                        }
                        for task in outcome.runtime_core_tasks {
                            match execute_runtime_core_task(&settings, task).await {
                                Ok(result) => pending_runtime_core_results.push(result),
                                Err(error) => warn!(%error, "runtime core control task failed"),
                            }
                        }
                        // 管理员从面板触发的整机重启:先安全自检再 reboot 宿主。
                        // 真重启时进程随宿主终止、不会回到这里;能存下的多是自检未过/无权限的失败结果,
                        // 下次心跳带回控制面清理待执行请求。绝不无命令自动重启。
                        if let Some(task) = outcome.reboot_task {
                            pending_reboot_result = Some(execute_reboot_task(&settings, task).await);
                        }
                    }
                    Err(error) => {
                        pending_runtime_core_results = runtime_core_results;
                        warn!(%error, "heartbeat failed")
                    },
                }
            }
            _ = traffic.tick() => {
                match collect_and_report_runtime(
                    &client,
                    &settings,
                    &mut active_config,
                    &mut applied_config_version,
                    &mut metrics_state,
                    &mut session_state,
                    &mut traffic_backlog,
                    kernel_capability.connmark_available,
                )
                .await
                {
                    Ok(()) => {}
                    Err(error) => {
                        warn!(%error, "runtime report failed");
                    }
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for shutdown signal")?;
                info!("shutdown signal received");
                break;
            }
        }
    }

    Ok(())
}

// 心跳请求需要逐项透传多个独立运行态(版本/配置/续期结果/核控结果/节点域名清单),
// 这些参数各自语义清晰且无共同归属,合并成结构体反而割裂调用处可读性,故放行参数数量上限。
#[allow(clippy::too_many_arguments)]
async fn send_heartbeat(
    client: &AgentClient,
    settings: &AgentSettings,
    uptime: Duration,
    applied_config_version: Option<&str>,
    active_config: Option<&AccessConfig>,
    tls_renew_result: Option<crate::client::TlsRenewResult>,
    runtime_core_results: Vec<RuntimeCoreControlResult>,
    node_domains: &[crate::client::NodeDomain],
    kernel_capability: KernelCapability,
    reboot_result: Option<crate::client::RebootResult>,
    node_metrics: Option<crate::client::NodeMetricsReport>,
) -> anyhow::Result<HeartbeatOutcome> {
    let response = client
        .heartbeat(&HeartbeatRequest {
            node_id: settings.node_id.clone(),
            agent_version: option_env!("XRAYC_BUILD_ID")
                .map(|release| format!("{AGENT_VERSION}+{release}"))
                .unwrap_or_else(|| AGENT_VERSION.to_owned()),
            hostname: hostname(),
            core_type: settings.runtime_core,
            xray_version: std::env::var("XRAYC_XRAY_VERSION")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            applied_config_version: applied_config_version.map(ToOwned::to_owned),
            uptime_seconds: uptime.as_secs(),
            tls_certificates: collect_tls_certificates(settings, active_config, node_domains),
            tls_renew_result,
            runtime_core_results,
            // 内核能力软信号上报:供控制面落库、面板显示"内核待升级·需重启"。
            kernel_connmark_available: Some(kernel_capability.connmark_available),
            kernel_upgrade_pending: Some(kernel_capability.upgrade_pending),
            reboot_result,
            node_metrics,
        })
        .await?;

    if !response.accepted {
        warn!("control plane rejected heartbeat");
        return Ok(HeartbeatOutcome::unchanged());
    }
    let tls_renew_task = response.tls_renew_task.clone();
    let runtime_core_tasks = response.runtime_core_tasks.clone();
    // 节点多域名清单随心跳下发,透传给证书续期/签发,供其遍历逐域名签证书。
    let node_domains = response.node_domains.clone();
    let reboot_task = response.reboot_task.clone();

    let mut outcome = apply_control_plane_config(
        client,
        settings,
        response.core_type,
        response.desired_config_version,
        response.config_status,
        response.config,
        // 内核 connmark 不可用时,本次配置 apply 走限速优雅降级(跳过下行整形、不 bail)。
        kernel_capability.connmark_available,
    )
    .await?;
    outcome.probe_tasks = response.probe_tasks;
    outcome.tls_renew_task = tls_renew_task;
    outcome.runtime_core_tasks = runtime_core_tasks;
    outcome.node_domains = node_domains;
    outcome.reboot_task = reboot_task;
    Ok(outcome)
}

#[derive(Debug)]
pub(super) struct HeartbeatOutcome {
    pub(super) applied_config_version: Option<String>,
    pub(super) replace_active_config: bool,
    pub(super) active_config: Option<AccessConfig>,
    pub(super) probe_tasks: Vec<ProbeTask>,
    pub(super) tls_renew_task: Option<crate::client::TlsRenewTask>,
    pub(super) runtime_core_tasks: Vec<RuntimeCoreTask>,
    // 节点多域名清单:证书续期/签发据此遍历逐域名签证书,空则沿用旧单域名路径。
    pub(super) node_domains: Vec<crate::client::NodeDomain>,
    // 管理员触发的整机重启请求:agent 自检后执行,空则无待执行重启。
    pub(super) reboot_task: Option<crate::client::RebootTask>,
}

impl HeartbeatOutcome {
    pub(super) fn unchanged() -> Self {
        Self {
            applied_config_version: None,
            replace_active_config: false,
            active_config: None,
            probe_tasks: Vec::new(),
            tls_renew_task: None,
            runtime_core_tasks: Vec::new(),
            node_domains: Vec::new(),
            reboot_task: None,
        }
    }
}
