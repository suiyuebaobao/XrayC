//! 本模块负责编排运行时周期性上报。
//! 它串联流量采集、积压合并、配置刷新、指标构建、会话快照和探测
//! 上报，让主循环只关注定时触发与状态切换。

use anyhow::Context;
use tracing::warn;
use xrayc_xray_config::AccessConfig;

use crate::client::{
    AccessUserSession, AgentClient, MetricsReportRequest, ProbeTask, ProbesReportRequest,
    SessionsReportRequest, TrafficReportRequest, TrafficSnapshot,
};
use crate::config::AgentSettings;
use crate::stats::collect_user_traffic_snapshots;

use super::apply::apply_control_plane_config;
use super::metrics::MetricsState;
use super::probes::{collect_probe_results, collect_probe_task_results};
use super::sessions::{collect_session_snapshot, SessionState};
use super::traffic_backlog::TrafficBacklog;
use super::utils::unix_now;
use super::HeartbeatOutcome;

// 串联流量采集/上报/配置刷新/指标/会话/探测的运行态参数各自语义独立、无共同归属,
// 合并成结构体反而割裂调用处可读性,放行参数数量上限(与 send_heartbeat 同理)。
#[allow(clippy::too_many_arguments)]
pub(super) async fn collect_and_report_runtime(
    client: &AgentClient,
    settings: &AgentSettings,
    active_config: &mut Option<AccessConfig>,
    applied_config_version: &mut Option<String>,
    metrics_state: &mut MetricsState,
    session_state: &mut SessionState,
    traffic_backlog: &mut TrafficBacklog,
    // 内核 connmark 软信号:流量上报触发配置刷新时同样按降级口径编排限速。
    connmark_available: bool,
) -> anyhow::Result<()> {
    let captured_at_unix = unix_now() as i64;
    let snapshots = collect_user_traffic_snapshots(settings, captured_at_unix).await?;
    let report_snapshots = traffic_backlog.merge_with_current(snapshots.clone());
    let outcome = match report_traffic_snapshot(
        client,
        settings,
        report_snapshots.clone(),
        applied_config_version.as_deref(),
        connmark_available,
    )
    .await
    {
        Ok(outcome) => {
            traffic_backlog.clear();
            outcome
        }
        Err(error) => {
            traffic_backlog.save(&report_snapshots).with_context(|| {
                format!("persist traffic backlog after report failure: {error}")
            })?;
            return Err(error);
        }
    };
    if let Some(version) = outcome.applied_config_version {
        *applied_config_version = Some(version);
    }
    if outcome.replace_active_config {
        *active_config = outcome.active_config;
        // 同心跳路径：配置刷新只裁剪失效用户的速率基线，保留存活用户基线，
        // 防止流量上报触发的配置刷新把实时速率清零（见 metrics::retain_for_active_config）。
        metrics_state.retain_for_active_config(active_config.as_ref());
    }
    let sessions = collect_session_snapshot(
        settings,
        active_config.as_ref(),
        session_state,
        captured_at_unix,
    );
    report_metrics_snapshot(
        client,
        settings,
        active_config.as_ref(),
        metrics_state,
        &snapshots,
        &sessions,
    )
    .await?;
    report_sessions_snapshot(client, settings, sessions).await?;
    report_probes_snapshot(client, settings, active_config.as_ref(), captured_at_unix).await?;
    Ok(())
}

pub(super) async fn report_traffic_snapshot(
    client: &AgentClient,
    settings: &AgentSettings,
    snapshots: Vec<TrafficSnapshot>,
    applied_config_version: Option<&str>,
    connmark_available: bool,
) -> anyhow::Result<HeartbeatOutcome> {
    let snapshots = reportable_traffic_snapshots(snapshots);
    if snapshots.is_empty() {
        return Ok(HeartbeatOutcome::unchanged());
    }
    let response = client
        .report_traffic(&TrafficReportRequest {
            node_id: settings.node_id.clone(),
            applied_config_version: applied_config_version.map(ToOwned::to_owned),
            snapshots,
        })
        .await?;

    if !response.accepted {
        anyhow::bail!("control plane rejected traffic report");
    }
    if response.config_status.required || response.config.is_some() {
        return apply_control_plane_config(
            client,
            settings,
            response.core_type,
            response.desired_config_version,
            response.config_status,
            response.config,
            connmark_available,
        )
        .await;
    }
    Ok(HeartbeatOutcome::unchanged())
}

fn reportable_traffic_snapshots(snapshots: Vec<TrafficSnapshot>) -> Vec<TrafficSnapshot> {
    let original_len = snapshots.len();
    let snapshots = snapshots
        .into_iter()
        .filter(|snapshot| {
            snapshot
                .access_line_id
                .as_deref()
                .is_some_and(|access_line_id| !access_line_id.trim().is_empty())
        })
        .collect::<Vec<_>>();
    let dropped = original_len.saturating_sub(snapshots.len());
    if dropped > 0 {
        warn!(dropped, "dropped traffic snapshots without access_line_id");
    }
    snapshots
}

async fn report_metrics_snapshot(
    client: &AgentClient,
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    metrics_state: &mut MetricsState,
    snapshots: &[TrafficSnapshot],
    sessions: &[AccessUserSession],
) -> anyhow::Result<()> {
    let collected_at_unix = snapshots
        .first()
        .map(|snapshot| snapshot.captured_at_unix)
        .unwrap_or_else(|| unix_now() as i64);
    let metrics = metrics_state.build_metrics(
        &settings.node_id,
        active_config,
        snapshots,
        sessions,
        collected_at_unix,
    );
    let response = client
        .report_metrics(&MetricsReportRequest {
            access_node_id: settings.node_id.clone(),
            metrics,
        })
        .await?;

    if !response.accepted {
        warn!("control plane rejected metrics report");
    }
    Ok(())
}

async fn report_sessions_snapshot(
    client: &AgentClient,
    settings: &AgentSettings,
    sessions: Vec<AccessUserSession>,
) -> anyhow::Result<()> {
    let response = client
        .report_sessions(&SessionsReportRequest {
            access_node_id: settings.node_id.clone(),
            sessions,
        })
        .await?;

    if !response.accepted {
        warn!("control plane rejected sessions report");
    }
    Ok(())
}

async fn report_probes_snapshot(
    client: &AgentClient,
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    checked_at_unix: i64,
) -> anyhow::Result<()> {
    let (line_probes, exit_probes) = match active_config {
        Some(config) => collect_probe_results(config, checked_at_unix).await,
        None => (Vec::new(), Vec::new()),
    };
    let response = client
        .report_probes(&ProbesReportRequest {
            access_node_id: settings.node_id.clone(),
            line_probes,
            exit_probes,
        })
        .await?;

    if !response.accepted {
        warn!("control plane rejected probes report");
    }
    Ok(())
}

pub(super) async fn report_probe_tasks_snapshot(
    client: &AgentClient,
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    tasks: &[ProbeTask],
    checked_at_unix: i64,
) -> anyhow::Result<()> {
    let (_, exit_probes) = collect_probe_task_results(active_config, tasks, checked_at_unix).await;
    if exit_probes.is_empty() {
        return Ok(());
    }
    let response = client
        .report_probes(&ProbesReportRequest {
            access_node_id: settings.node_id.clone(),
            line_probes: Vec::new(),
            exit_probes,
        })
        .await?;

    if !response.accepted {
        warn!("control plane rejected manual probe task report");
    }
    Ok(())
}
