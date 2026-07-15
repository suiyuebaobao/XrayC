//! 本模块测试 access-agent 内核能力自检与自动装内核(不重启)逻辑。
//! 测试只用本地 `true`/`false` 等命令驱动各分支,绝不真 modprobe/装内核/重启。
//! 钉死口径(§7.7.1):connmark 探测可用→不装内核、无待重启;
//! 不可用→标 connmark 不可用,装内核命令成功则标待重启、失败只上报不强求。
//! 探测命令置空按可用放行(测试/特殊部署),装内核命令置空只上报不装。
//! 任何命令失败都软处理,绝不向上抛错、绝不影响后续配置 apply。
//! 测试用 tempdir 承载 settings 路径,执行结束自动清理。
//! KernelCapability 默认即可用、无待升级,作向后兼容兜底。
//! 新增 settings 字段时同步更新 test_settings 构造器。
//! 本文件前十行中文注释满足仓库规则。

use tempfile::tempdir;

use super::super::kernel::{
    detect_kernel_capability, should_attempt_kernel_install, KernelCapability,
};
use super::support::test_settings;

#[test]
fn test_kernel_capability_default_is_available_and_no_pending() {
    // 缺省软信号:旧探测未跑或被跳过时不误报降级、不误报待重启。
    let capability = KernelCapability::default();
    assert!(capability.connmark_available);
    assert!(!capability.upgrade_pending);
}

#[tokio::test]
async fn test_detect_kernel_capability_reports_available_when_probe_succeeds() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // 探测命令退出 0:判 connmark 可用,不应触发装内核、不应标待重启。
    settings.kernel_connmark_probe_command = "true".to_string();
    // 故意把装内核命令设成会写标记的命令,验证可用路径根本不执行它。
    let marker = dir.path().join("kernel-installed");
    settings.kernel_upgrade_command = format!("printf x > '{}'", marker.display());

    let capability = detect_kernel_capability(&settings, KernelCapability::default(), true).await;

    assert!(capability.connmark_available);
    assert!(!capability.upgrade_pending);
    assert!(!marker.exists(), "connmark 可用时绝不应执行装内核命令");
}

#[tokio::test]
async fn test_detect_kernel_capability_installs_kernel_when_connmark_unavailable() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // 探测命令退出非 0:判 connmark 不可用,触发装内核。
    settings.kernel_connmark_probe_command = "false".to_string();
    // 装内核命令退出 0:视为已装新内核,标待重启。
    let marker = dir.path().join("kernel-installed");
    settings.kernel_upgrade_command = format!("printf x > '{}'", marker.display());

    let capability = detect_kernel_capability(&settings, KernelCapability::default(), true).await;

    assert!(!capability.connmark_available);
    assert!(capability.upgrade_pending, "装内核成功应标内核待重启");
    assert!(marker.exists(), "connmark 不可用时应执行装内核命令");
}

#[tokio::test]
async fn test_detect_kernel_capability_reports_pending_false_when_install_fails() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.kernel_connmark_probe_command = "false".to_string();
    // 装内核命令退出非 0(模拟容器无宿主权限):软失败,只上报不可用、不标待重启。
    settings.kernel_upgrade_command = "false".to_string();

    let capability = detect_kernel_capability(&settings, KernelCapability::default(), true).await;

    assert!(!capability.connmark_available);
    assert!(
        !capability.upgrade_pending,
        "装内核失败(无权限)只上报降级,不标待重启"
    );
}

#[tokio::test]
async fn test_detect_kernel_capability_empty_probe_treated_as_available() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // 探测命令置空:不在本机真实探测,按可用放行(测试/特殊部署)。
    settings.kernel_connmark_probe_command = String::new();

    let capability = detect_kernel_capability(&settings, KernelCapability::default(), true).await;

    assert!(capability.connmark_available);
    assert!(!capability.upgrade_pending);
}

#[test]
fn test_should_attempt_kernel_install_backoff() {
    use std::time::Duration;
    let backoff = Duration::from_secs(3600);
    // 从未尝试过 → 允许装一次。
    assert!(should_attempt_kernel_install(None, backoff));
    // 距上次尝试未超退避窗口 → 不重复跑 apt(避免每次心跳一次)。
    assert!(!should_attempt_kernel_install(
        Some(Duration::from_secs(30)),
        backoff
    ));
    // 距上次尝试已达/超退避窗口 → 允许再试一次。
    assert!(should_attempt_kernel_install(
        Some(Duration::from_secs(3600)),
        backoff
    ));
    assert!(should_attempt_kernel_install(
        Some(Duration::from_secs(7200)),
        backoff
    ));
}

#[tokio::test]
async fn test_detect_kernel_capability_skips_install_when_not_allowed() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // connmark 不可用,但本轮处于退避窗口内(attempt_install=false):不应重复跑装内核命令,
    // 维持降级 + 沿用上一轮的待重启标记。这是 P1-3 修复的核心:降级期间不每心跳跑 apt。
    settings.kernel_connmark_probe_command = "false".to_string();
    let marker = dir.path().join("kernel-installed");
    settings.kernel_upgrade_command = format!("printf x > '{}'", marker.display());
    let previous = KernelCapability {
        connmark_available: false,
        upgrade_pending: true,
    };

    let capability = detect_kernel_capability(&settings, previous, false).await;

    assert!(!capability.connmark_available);
    assert!(capability.upgrade_pending, "退避内应沿用上一轮的待重启标记");
    assert!(!marker.exists(), "退避窗口内绝不应重复执行装内核命令");
}
