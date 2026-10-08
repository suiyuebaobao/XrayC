//! 本模块执行控制面下发的「整机重启」请求并带安全兜底自检。
//! 重启只由管理员从面板触发、经心跳命令通道到达,**绝不无命令自动重启**。
//! 收到 reboot 请求后先逐项自检:GRUB 默认引导最高版本内核、本机容器自启策略
//! always/unless-stopped、docker 服务开机自启;全部满足才下达宿主 reboot,
//! 避免重启后入口不自动拉起、或仍引导旧内核导致升级白做。
//! 自检任一项不过只回报失败、不重启,把"为什么没重启"如实带回控制面。
//! agent 多在容器内,reboot 宿主需宿主权限;无权限时自检/重启命令失败按失败回报。
//! 自检与 reboot 命令均可经 env 覆盖/置空,便于单测与无权限环境跳过真实执行。
//! 结果只回传脱敏短摘要,不回传宿主明文细节(GRUB 配置/进程列表等)。
//! 命令为空时按"已满足"放行,只用于测试/特殊部署,生产须配真实自检命令。
//! 文件前十行中文注释满足仓库约束。

use tokio::process::Command;
use tracing::{info, warn};

use crate::client::{RebootResult, RebootTask};
use crate::config::AgentSettings;

/// 执行一条重启请求:先安全自检,通过才下达 reboot,返回供心跳回报的结果。
///
/// 安全自检三项(任一不过即拒绝重启,回报 failed + 原因):
/// 其一 GRUB 默认引导最高版本内核——否则重启后仍跑旧内核,自动装内核白做;
/// 其二 本机容器自启策略 always/unless-stopped——否则重启后入口容器不自动拉起;
/// 其三 docker 服务开机自启——否则重启后 docker 不起、整机入口全停。
/// 自检命令以 sh -c 退出码为准(0=满足);命令为空按满足放行(测试/特殊部署)。
pub(super) async fn execute_reboot_task(
    settings: &AgentSettings,
    task: RebootTask,
) -> RebootResult {
    match run_reboot_safety_checks(settings).await {
        Ok(()) => {}
        Err(reason) => {
            // 自检不过:绝不重启,如实把原因回报控制面,等管理员修复后再触发。
            warn!(reason = %reason, "整机重启安全自检未通过,拒绝重启");
            return RebootResult {
                request_id: task.request_id,
                status: "failed".to_owned(),
                message: reason,
            };
        }
    }

    // 自检通过仍不代表重启已执行；缺少命令必须如实失败。
    let command = settings.host_reboot_command.trim();
    if command.is_empty() {
        warn!("整机重启命令未配置,未实际重启");
        return RebootResult {
            request_id: task.request_id,
            status: "failed".to_owned(),
            message: "重启自检通过;重启命令未配置,未实际重启".to_owned(),
        };
    }
    match run_check_command(command).await {
        Ok(true) => {
            info!("整机重启自检通过,已下达宿主重启");
            RebootResult {
                request_id: task.request_id,
                status: "success".to_owned(),
                message: "安全自检通过,已下达整机重启".to_owned(),
            }
        }
        Ok(false) => {
            // reboot 命令退出非 0:多为容器无宿主权限,如实回报失败。
            warn!("重启命令退出非 0(可能容器无宿主权限),未能重启");
            RebootResult {
                request_id: task.request_id,
                status: "failed".to_owned(),
                message: "重启命令执行失败(可能容器无宿主权限)".to_owned(),
            }
        }
        Err(error) => {
            warn!(%error, "重启命令执行异常,未能重启");
            RebootResult {
                request_id: task.request_id,
                status: "failed".to_owned(),
                message: "重启命令执行异常".to_owned(),
            }
        }
    }
}

/// 逐项跑安全自检命令,任一不过返回 Err(脱敏原因)。命令为空的项按满足跳过。
async fn run_reboot_safety_checks(settings: &AgentSettings) -> Result<(), String> {
    let checks = [
        (
            settings.reboot_check_grub_default_command.trim(),
            "GRUB 默认未引导最高版本内核,重启后仍会跑旧内核",
        ),
        (
            settings.reboot_check_container_restart_command.trim(),
            "本机容器自启策略不是 always/unless-stopped,重启后入口不会自动拉起",
        ),
        (
            settings.reboot_check_docker_enabled_command.trim(),
            "docker 服务未设开机自启,重启后入口会全停",
        ),
    ];
    for (command, fail_reason) in checks {
        if command.is_empty() {
            // 该自检项命令未配置:按满足放行(仅测试/特殊部署,生产须配齐)。
            continue;
        }
        match run_check_command(command).await {
            Ok(true) => {}
            Ok(false) => return Err(fail_reason.to_owned()),
            Err(_) => {
                // 自检命令本身执行异常(命令缺失/权限)也视为不满足,保守拒绝重启。
                return Err(format!("{fail_reason}(自检命令执行异常,保守拒绝重启)"));
            }
        }
    }
    Ok(())
}

/// 运行一条自检/重启命令,只返回退出是否成功,不回传 stdout/stderr 明文。
async fn run_check_command(command: &str) -> anyhow::Result<bool> {
    let output = Command::new("sh").arg("-c").arg(command).output().await?;
    Ok(output.status.success())
}
