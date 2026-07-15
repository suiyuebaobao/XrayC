//! 本模块自检中转节点 Linux 内核能力并按需自动升级内核。
//! 核心是探测下行限速整形依赖的 `act_connmark` 模块是否可加载;
//! 不可用即标记软信号 `connmark_available=false`,触发限速优雅降级。
//! 探测到不可用时自动 `apt-get install` 最新内核但**绝不自动重启**,
//! 只记 `upgrade_pending` 软状态,等管理员在控制面择时重启宿主。
//! agent 多在容器内,装宿主内核需宿主权限;无权限时只上报不强求,
//! 任何 modprobe/apt 失败都当软失败吞掉,绝不让配置 apply bail。
//! 探测结果经心跳上报落 access_nodes 读模型供面板显示。
//! 命令可经 env 覆盖/置空,便于单测与无权限环境跳过真实执行。
//! 文件前十行中文注释满足仓库约束。

use tokio::process::Command;
use tracing::{info, warn};

use crate::config::AgentSettings;

/// 内核能力自检结果(心跳上报与限速降级共用的软信号)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KernelCapability {
    /// 下行整形依赖的 `act_connmark` 是否可加载;false 即限速跳过下行 connmark 整形。
    pub(super) connmark_available: bool,
    /// 本轮是否触发过自动装新内核(装好但未重启);true 即面板提示"内核待升级·需重启"。
    pub(super) upgrade_pending: bool,
}

impl Default for KernelCapability {
    fn default() -> Self {
        // 缺省按可用、无待升级:探测未跑或被显式跳过时不误报降级。
        Self {
            connmark_available: true,
            upgrade_pending: false,
        }
    }
}

/// 探测内核能力;仅在 `attempt_install=true`(退避窗口外/首次)时才尝试自动装新内核(不重启),
/// 返回供心跳上报的软信号。`previous` 供"本轮降级但退避内不装"时沿用上一轮的待重启标记。
///
/// 优雅降级口径:`act_connmark` 探测失败只标记 `connmark_available=false`,不 bail;
/// 允许装时触发一次幂等装内核(命令为空跳过),装内核成功标记 `upgrade_pending`。装内核失败
/// (容器无权限/命令缺失/隔离)按软失败吞掉,只如实上报 connmark 不可用,绝不影响后续配置 apply。
/// 退避由调用方(runtime 主循环)用 should_attempt_kernel_install 控制,避免降级期间每心跳跑 apt。
pub(super) async fn detect_kernel_capability(
    settings: &AgentSettings,
    previous: KernelCapability,
    attempt_install: bool,
) -> KernelCapability {
    let connmark_available = probe_connmark_available(settings).await;
    if connmark_available {
        // 内核能力正常:不装内核、不标待重启,下行整形照常生成(行为零变化)。
        return KernelCapability {
            connmark_available: true,
            upgrade_pending: false,
        };
    }

    if !attempt_install {
        // 退避窗口内:不重复跑 apt,维持降级并沿用上一轮的待重启标记(装内核只在退避外做一次)。
        return KernelCapability {
            connmark_available: false,
            upgrade_pending: previous.upgrade_pending,
        };
    }

    warn!("act_connmark 不可加载:限速将跳过下行整形并尝试自动装新内核(不自动重启)");
    // 装成功标待重启;`|| previous` 保证"曾装成功后某轮命令偶发 false"不丢待重启标记。
    let upgrade_pending = try_auto_install_kernel(settings).await || previous.upgrade_pending;
    KernelCapability {
        connmark_available: false,
        upgrade_pending,
    }
}

/// 内核降级态下是否该(再次)尝试自动装内核:从未尝试过→true;否则须距上次尝试超过退避窗口才 true。
/// 避免降级期间每次心跳(默认 30s)重复跑 apt-get,造成 dpkg 锁争用、apt 网络风暴与日志刷屏。纯函数便于单测。
pub(super) fn should_attempt_kernel_install(
    last_attempt_elapsed: Option<std::time::Duration>,
    backoff: std::time::Duration,
) -> bool {
    match last_attempt_elapsed {
        None => true,
        Some(elapsed) => elapsed >= backoff,
    }
}

/// 探测 `act_connmark` 是否可加载(判可加载性而非内核版本号)。
///
/// 默认用 `modprobe act_connmark` 真加载判定;命令为空(env 显式置空)时
/// 直接返回可用,便于单测与无 modprobe 环境跳过真实探测。任何执行错误
/// (spawn 失败/退出非 0)一律判不可用,作软信号交给降级路径处理。
async fn probe_connmark_available(settings: &AgentSettings) -> bool {
    let command = settings.kernel_connmark_probe_command.trim();
    if command.is_empty() {
        // 探测命令被显式置空:不在本机做真实加载,按可用处理(测试/特殊部署)。
        return true;
    }
    match run_kernel_command(command).await {
        Ok(success) => success,
        Err(error) => {
            // spawn 失败(如容器无 modprobe)按不可用,触发降级与上报,绝不 bail。
            warn!(%error, "act_connmark 探测命令执行失败,按不可用处理");
            false
        }
    }
}

/// 触发一次幂等的自动装最新内核(绝不重启);返回是否成功执行了装内核命令。
///
/// 命令默认 `apt-get update && apt-get install -y linux-generic-hwe-22.04`,
/// 可经 env 覆盖或置空。装新内核保留旧内核作 GRUB 回退、幂等(已最新则 no-op)。
/// **本函数绝不调用 reboot**:重启时机交管理员在面板触发,避免客户无预警停机。
/// 命令为空、spawn 失败或退出非 0(多为容器无宿主权限)都按软失败返回 false,
/// 只如实反映"未能装新内核",不向上抛错、不影响配置 apply。
async fn try_auto_install_kernel(settings: &AgentSettings) -> bool {
    let command = settings.kernel_upgrade_command.trim();
    if command.is_empty() {
        // 装内核命令被置空:本部署不让 agent 自动装内核,只上报 connmark 不可用。
        info!("自动装内核命令为空,跳过装内核,仅上报内核能力降级");
        return false;
    }
    match run_kernel_command(command).await {
        Ok(true) => {
            info!("已自动安装最新内核(保留旧内核 GRUB 回退),等待管理员从面板择时重启");
            true
        }
        Ok(false) => {
            // 退出非 0:多为容器无宿主权限装宿主内核,软失败,不强求。
            warn!("自动装内核命令退出非 0(可能容器无宿主权限),仅上报内核能力降级");
            false
        }
        Err(error) => {
            warn!(%error, "自动装内核命令执行失败,仅上报内核能力降级");
            false
        }
    }
}

/// 运行一条内核相关命令,只返回退出是否成功,不回传 stdout/stderr 明文。
///
/// 这些命令(modprobe/apt-get)可能含本机包名,按仓库安全口径不把输出落日志,
/// 只用成功布尔值驱动降级与上报。spawn 失败向上抛错,由调用方按软失败处理。
async fn run_kernel_command(command: &str) -> anyhow::Result<bool> {
    let output = Command::new("sh").arg("-c").arg(command).output().await?;
    Ok(output.status.success())
}
