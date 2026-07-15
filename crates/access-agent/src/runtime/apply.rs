//! 本模块负责控制面配置应用。
//! 这里完成 Xray 配置编译、运行时端口补丁、写入前校验、失败回滚
//! 以及配置应用结果回报，是心跳与流量上报共用的配置刷新通道。

use std::fs;
use std::path::Path;

use anyhow::Context;
use tokio::process::Command;
use tracing::warn;
#[cfg(test)]
use tracing::{error, info};
use xrayc_xray_config::{compile_xray_config, AccessConfig};

use crate::client::{AgentClient, ConfigResultRequest, ConfigStatus};
use crate::config::{AgentSettings, RuntimeCore};

use super::apply_config::{empty_runtime_config, patch_runtime_config};
use super::cache::persist_active_config_cache;
use super::limiter::reconcile_limiter;
use super::split::split_config_for_runtime;
use super::utils::{candidate_config_path, unix_now};
use super::HeartbeatOutcome;

pub(super) async fn apply_control_plane_config(
    client: &AgentClient,
    settings: &AgentSettings,
    _runtime_core: RuntimeCore,
    desired_config_version: Option<String>,
    config_status: ConfigStatus,
    config: Option<AccessConfig>,
    // 内核 act_connmark 软信号:逐层透传到 reconcile_limiter,不可用时下行整形优雅降级。
    connmark_available: bool,
) -> anyhow::Result<HeartbeatOutcome> {
    let version = desired_config_version.unwrap_or_else(|| unix_now().to_string());

    let Some(config) = config else {
        if config_status.required {
            let applied =
                apply_empty_node_config(client, settings, &version, connmark_available).await?;
            if applied {
                persist_active_config_cache(settings, Some(version.clone()), None);
            }
            return Ok(HeartbeatOutcome {
                applied_config_version: applied.then_some(version),
                replace_active_config: applied,
                active_config: None,
                probe_tasks: Vec::new(),
                tls_renew_task: None,
                runtime_core_tasks: Vec::new(),
                node_domains: Vec::new(),
                reboot_task: None,
            });
        }
        return Ok(HeartbeatOutcome::unchanged());
    };

    let applied =
        apply_node_runtime_config(client, settings, &version, &config, connmark_available).await?;
    if applied {
        persist_active_config_cache(settings, Some(version.clone()), Some(config.clone()));
    }
    Ok(HeartbeatOutcome {
        applied_config_version: applied.then_some(version),
        replace_active_config: applied,
        active_config: applied.then_some(config),
        probe_tasks: Vec::new(),
        tls_renew_task: None,
        runtime_core_tasks: Vec::new(),
        node_domains: Vec::new(),
        reboot_task: None,
    })
}

pub(super) async fn apply_empty_node_config(
    client: &AgentClient,
    settings: &AgentSettings,
    version: &str,
    connmark_available: bool,
) -> anyhow::Result<bool> {
    let result = match apply_empty_runtime_config(settings, RuntimeCore::Xray).await {
        Ok(()) => match reconcile_limiter(settings, &[], connmark_available).await {
            Ok(()) => ConfigResultRequest {
                node_id: settings.node_id.clone(),
                config_version: version.to_owned(),
                success: true,
                message: Some("applied empty access config".to_owned()),
            },
            Err(error) => ConfigResultRequest {
                node_id: settings.node_id.clone(),
                config_version: version.to_owned(),
                success: false,
                message: Some(error.to_string()),
            },
        },
        Err(error) => ConfigResultRequest {
            node_id: settings.node_id.clone(),
            config_version: version.to_owned(),
            success: false,
            message: Some(error.to_string()),
        },
    };
    let applied = result.success;
    client.report_config_result(&result).await?;
    Ok(applied)
}

#[cfg(test)]
pub(super) async fn apply_empty_config(
    client: &AgentClient,
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
    version: &str,
) -> anyhow::Result<bool> {
    // 控制面要求刷新但没有可用线路时，写入空入口配置，避免旧用户继续留在 Xray。
    // 测试辅助路径固定按 connmark 可用编排;降级编排由 limiter 单测直接覆盖。
    let result = match apply_empty_runtime_config(settings, runtime_core).await {
        Ok(()) => match reconcile_limiter(settings, &[], true).await {
            Ok(()) => {
                info!(
                    config_version = version,
                    core_type = runtime_core.as_str(),
                    "empty runtime config applied"
                );
                ConfigResultRequest {
                    node_id: settings.node_id.clone(),
                    config_version: version.to_owned(),
                    success: true,
                    message: Some("applied empty access config".to_owned()),
                }
            }
            Err(error) => ConfigResultRequest {
                node_id: settings.node_id.clone(),
                config_version: version.to_owned(),
                success: false,
                message: Some(error.to_string()),
            },
        },
        Err(error) => ConfigResultRequest {
            node_id: settings.node_id.clone(),
            config_version: version.to_owned(),
            success: false,
            message: Some(error.to_string()),
        },
    };
    let applied = result.success;
    client.report_config_result(&result).await?;
    Ok(applied)
}

async fn apply_empty_runtime_config(
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
) -> anyhow::Result<()> {
    let rendered = serde_json::to_string_pretty(&empty_runtime_config(settings, runtime_core))?;
    apply_rendered_runtime_config(settings, runtime_core, &rendered).await
}

async fn apply_rendered_runtime_config(
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
    rendered: &str,
) -> anyhow::Result<()> {
    ensure_runtime_directories(settings)?;
    let target = runtime_config_path(settings, runtime_core);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
    }
    let candidate = candidate_config_path(target);
    fs::write(&candidate, rendered).with_context(|| format!("write {:?}", candidate))?;
    run_config_command(
        runtime_test_command(settings, runtime_core),
        &candidate,
        "validate rendered runtime config",
    )
    .await?;

    let previous = fs::read(target).ok();
    // 历史远端部署曾把宿主机配置作为单文件 bind mount 挂进容器。
    // 对这种路径执行 rename 可能只替换容器内挂载点，宿主机或 xray 容器仍看到旧文件。
    // 因此 Xray 配置必须在校验候选文件后复制覆盖目标文件内容。
    fs::copy(&candidate, target).with_context(|| format!("replace {:?}", target))?;
    fs::remove_file(&candidate).with_context(|| format!("remove {:?}", candidate))?;
    let persisted = fs::read(target).with_context(|| format!("read {:?}", target))?;
    if persisted != rendered.as_bytes() {
        anyhow::bail!("runtime config write verification failed");
    }

    if let Err(error) = run_config_command(
        runtime_reload_command(settings, runtime_core),
        target,
        "reload runtime after config update",
    )
    .await
    {
        // 新配置 reload 失败：先把磁盘文件回滚成旧配置，再尝试用旧配置 reload。
        // 这里区分三种结局，回报给控制面的错误必须能反映节点真实一致性：
        // 1) 文件回滚失败 → 磁盘上仍是坏配置，节点处于不一致态。
        // 2) 文件回滚成功但旧配置 reload 失败 → 磁盘已好，但内核仍在跑坏配置，
        //    这是最危险的"磁盘好、内核坏"不一致，绝不能只 warn 吞掉。
        // 3) 文件回滚 + 旧配置 reload 都成功 → 节点已恢复旧状态，只回报原始 reload 错误。
        if let Err(rollback_error) = restore_previous_config(target, previous.as_deref()) {
            warn!(%rollback_error, "rollback xray config failed");
            return Err(error.context(format!(
                "{core} config reload failed and config rollback failed; node config is inconsistent (disk still holds bad config): {rollback_error}",
                core = runtime_core.as_str()
            )));
        }
        if let Err(reload_error) = run_config_command(
            runtime_reload_command(settings, runtime_core),
            target,
            "reload runtime after rollback",
        )
        .await
        {
            warn!(%reload_error, "reload runtime after rollback failed");
            return Err(error.context(format!(
                "{core} config reload failed and reload after rollback also failed; node is inconsistent (disk restored to previous config but runtime still runs the failed config): {reload_error}",
                core = runtime_core.as_str()
            )));
        }
        return Err(error);
    }

    // reload 成功后回收旧 Xray 实例。切换/redeploy 时旧 Xray 容器可能已被删除，
    // 但其进程仍存活，并通过 SO_REUSEPORT 继续监听 Stats API 端口（如 10085）。
    // 此时 agent 的 statsquery 会被内核分流到旧进程，读到的是旧/空的用户级计数，
    // 不上报 → 计费与超额剔除全部失效。因此必须在 reload 后确保该端口只有当前实例在听。
    if runtime_core == RuntimeCore::Xray {
        reclaim_stale_xray_instances(settings).await?;
    }

    Ok(())
}

/// 回收占用 Stats API 端口的陈旧 Xray 进程/容器。
/// 命令由部署脚本注入（基于 docker），通过环境变量拿到当前容器名与 Stats 端口，
/// 自行 `docker rm -f` 同端口的旧容器并杀掉残留进程。命令为空时跳过（便于单测与
/// 非 docker 环境）。回收失败必须向上抛出：只要旧实例还活着，计费就可能继续读错进程，
/// 这是真实生产故障，不能静默吞掉。
async fn reclaim_stale_xray_instances(settings: &AgentSettings) -> anyhow::Result<()> {
    let command = settings.xray_reclaim_command.trim();
    if command.is_empty() {
        return Ok(());
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("XRAYC_XRAY_CONTAINER_NAME", &settings.xray_container_name)
        .env("XRAYC_XRAY_API_LISTEN_HOST", &settings.xray_api_listen_host)
        .env(
            "XRAYC_XRAY_API_LISTEN_PORT",
            settings.xray_api_listen_port.to_string(),
        )
        .output()
        .await
        .context("reclaim stale xray instances: spawn command")?;
    if output.status.success() {
        return Ok(());
    }
    anyhow::bail!(
        "reclaim stale xray instances failed: {}",
        command_failure_summary(&output.stdout, &output.stderr)
    );
}

fn ensure_runtime_directories(settings: &AgentSettings) -> anyhow::Result<()> {
    for path in [
        &settings.xray_config_path,
        &settings.agent_state_path,
        &settings.traffic_backlog_path,
        &settings.xray_access_log_path,
        &settings.rate_limiter_plan_path,
    ] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
        }
    }
    Ok(())
}

fn restore_previous_config(target: &Path, previous: Option<&[u8]>) -> anyhow::Result<()> {
    match previous {
        Some(bytes) => fs::write(target, bytes).with_context(|| format!("restore {:?}", target))?,
        None if target.exists() => {
            fs::remove_file(target).with_context(|| format!("remove {:?}", target))?;
        }
        None => {}
    }
    Ok(())
}

fn runtime_config_path(settings: &AgentSettings, runtime_core: RuntimeCore) -> &Path {
    match runtime_core {
        RuntimeCore::Xray => &settings.xray_config_path,
    }
}

fn runtime_test_command(settings: &AgentSettings, runtime_core: RuntimeCore) -> &str {
    match runtime_core {
        RuntimeCore::Xray => &settings.xray_test_command,
    }
}

fn runtime_reload_command(settings: &AgentSettings, runtime_core: RuntimeCore) -> &str {
    match runtime_core {
        RuntimeCore::Xray => &settings.xray_reload_command,
    }
}

async fn run_config_command(command: &str, config_path: &Path, action: &str) -> anyhow::Result<()> {
    let command = command.trim();
    if command.is_empty() {
        return Ok(());
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("XRAYC_XRAY_CONFIG", config_path)
        .env("XRAYC_RUNTIME_CONFIG", config_path)
        .output()
        .await
        .with_context(|| format!("{action}: spawn command"))?;
    if output.status.success() {
        return Ok(());
    }

    anyhow::bail!(
        "{action} failed: {}",
        command_failure_summary(&output.stdout, &output.stderr)
    );
}

// 命令失败输出的可诊断脱敏摘要拆到 apply_failure_summary 模块（BUG-G 可观测性），
// 这里只重导出，保持本文件聚焦配置应用主流程并守住单文件行数上限。
pub(super) use super::apply_failure_summary::command_failure_summary;

#[cfg(test)]
pub(super) async fn apply_config(
    client: &AgentClient,
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
    version: &str,
    config: &AccessConfig,
) -> anyhow::Result<bool> {
    // 测试辅助路径固定按 connmark 可用编排;降级编排由 limiter 单测直接覆盖。
    let result = match render_and_apply_runtime_config(settings, runtime_core, config, true).await {
        Ok(()) => {
            info!(
                config_version = version,
                core_type = runtime_core.as_str(),
                "runtime config applied"
            );
            ConfigResultRequest {
                node_id: settings.node_id.clone(),
                config_version: version.to_owned(),
                success: true,
                message: None,
            }
        }
        Err(error) => {
            error!(%error, config_version = version, core_type = runtime_core.as_str(), "runtime config apply failed");
            ConfigResultRequest {
                node_id: settings.node_id.clone(),
                config_version: version.to_owned(),
                success: false,
                message: Some(error.to_string()),
            }
        }
    };

    let applied = result.success;
    client.report_config_result(&result).await?;
    Ok(applied)
}

async fn apply_node_runtime_config(
    client: &AgentClient,
    settings: &AgentSettings,
    version: &str,
    config: &AccessConfig,
    connmark_available: bool,
) -> anyhow::Result<bool> {
    let xray_config = split_config_for_runtime(config, RuntimeCore::Xray);
    let result = match apply_split_runtime_config(
        settings,
        RuntimeCore::Xray,
        &xray_config,
        connmark_available,
    )
    .await
    {
        Ok(()) => ConfigResultRequest {
            node_id: settings.node_id.clone(),
            config_version: version.to_owned(),
            success: true,
            message: None,
        },
        Err(error) => ConfigResultRequest {
            node_id: settings.node_id.clone(),
            config_version: version.to_owned(),
            success: false,
            message: Some(error.to_string()),
        },
    };

    let applied = result.success;
    client.report_config_result(&result).await?;
    Ok(applied)
}

async fn apply_split_runtime_config(
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
    config: &AccessConfig,
    connmark_available: bool,
) -> anyhow::Result<()> {
    if config.access_lines.is_empty() && config.local_exit_services.is_empty() {
        if runtime_core == RuntimeCore::Xray {
            reconcile_limiter(settings, &[], connmark_available).await?;
        }
        return apply_empty_runtime_config(settings, runtime_core).await;
    }
    render_and_apply_runtime_config(settings, runtime_core, config, connmark_available).await
}

async fn render_and_apply_runtime_config(
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
    config: &AccessConfig,
    connmark_available: bool,
) -> anyhow::Result<()> {
    let compiled = match runtime_core {
        RuntimeCore::Xray => compile_xray_config(config),
    };
    let mut value = compiled?;
    patch_runtime_config(&mut value, settings, runtime_core);
    let rendered = serde_json::to_string_pretty(&value)?;
    if runtime_core == RuntimeCore::Xray {
        prepare_limiter_for_config(settings, config, connmark_available).await?;
    }
    apply_rendered_runtime_config(settings, runtime_core, &rendered).await
}

async fn prepare_limiter_for_config(
    settings: &AgentSettings,
    config: &AccessConfig,
    connmark_available: bool,
) -> anyhow::Result<()> {
    let has_positive_rate_limits = config
        .rate_limits
        .iter()
        .any(|limit| limit.rate_limit_bps > 0);
    if has_positive_rate_limits {
        if !settings.rate_limiter_enabled {
            anyhow::bail!("rate limiter is disabled while positive user rate limits are present");
        }
        if settings.rate_limiter_dry_run {
            anyhow::bail!("rate limiter dry-run cannot enforce positive user rate limits");
        }
    }
    // 优雅降级(§7.7.1 红线):限速整形失败(网卡名与实际不符/内核不支持 ingress qdisc/
    // connmark filter 失败/写 plan 文件失败等)只记警告,**绝不让整次 Xray apply 失败**——
    // 否则新配置在写盘/reload 前中断、节点配置永不收敛(首次 apply 则 443 不监听)。
    // 「有正向限速却 rate_limiter 关闭/dry-run」的配置矛盾已在上面提前 bail,不会走到这里降级。
    if let Err(error) = reconcile_limiter(settings, &config.rate_limits, connmark_available).await {
        tracing::warn!(error = %error, "限速 reconcile 失败,已降级跳过整形、继续 apply Xray 配置");
    }
    Ok(())
}
