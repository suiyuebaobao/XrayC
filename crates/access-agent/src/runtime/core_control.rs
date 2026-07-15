//! 本模块执行控制面下发的单内核运行控制任务。
//! 任务只允许 start/stop，并且只在本地执行预配置命令。
//! 控制面不下发任意 shell，避免把管理平台变成远程命令执行入口。
//! 命令由安装脚本写入 access-agent 环境变量，通常是 Docker start/stop。
//! 执行结果只返回成功布尔值和短消息，不回传 stdout/stderr 明文。
//! request_id 必须原样带回，服务端据此清理待同步任务。
//! 失败不让 agent 主循环退出，而是作为结果上报控制面。
//! 本文件不依赖真实 Docker，便于单元测试使用临时命令。
//! 修改字段时同步更新 client DTO 和部署脚本环境变量。
//! 文件前十行中文注释满足仓库规则。

use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;

use crate::client::{RuntimeCoreAction, RuntimeCoreControlResult, RuntimeCoreTask};
use crate::config::{AgentSettings, RuntimeCore};

pub(super) async fn execute_runtime_core_task(
    settings: &AgentSettings,
    task: RuntimeCoreTask,
) -> anyhow::Result<RuntimeCoreControlResult> {
    let command = runtime_core_command(settings, task.core_type, task.action);
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| "run runtime core control command")?;
    let success = status.success();
    Ok(RuntimeCoreControlResult {
        request_id: task.request_id,
        core_type: task.core_type,
        action: task.action,
        success,
        message: runtime_core_result_message(task.core_type, task.action, success),
    })
}

fn runtime_core_command(
    settings: &AgentSettings,
    core_type: RuntimeCore,
    action: RuntimeCoreAction,
) -> &str {
    match (core_type, action) {
        (RuntimeCore::Xray, RuntimeCoreAction::Start) => &settings.xray_start_command,
        (RuntimeCore::Xray, RuntimeCoreAction::Stop) => &settings.xray_stop_command,
    }
}

fn runtime_core_result_message(
    core_type: RuntimeCore,
    action: RuntimeCoreAction,
    success: bool,
) -> String {
    let action = match action {
        RuntimeCoreAction::Start => "start",
        RuntimeCoreAction::Stop => "stop",
    };
    let status = if success { "succeeded" } else { "failed" };
    format!("{action} {core_type} runtime core {status}")
}
