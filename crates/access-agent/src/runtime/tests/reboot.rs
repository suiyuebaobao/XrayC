//! 本模块测试 access-agent 整机重启请求执行与安全自检。
//! 测试只用本地 `true`/`false`/写标记命令驱动各分支,绝不真重启宿主。
//! 钉死口径(§7.7.1):重启只由控制面下发命令触发,执行前先安全自检——
//! GRUB 默认引导最高内核、容器自启策略、docker 开机自启,任一不过即拒绝重启。
//! 自检全过才下达 reboot;reboot 命令置空只回报成功不真重启(测试)。
//! 自检命令置空按满足放行;自检命令退出非 0 或异常即保守拒绝重启。
//! request_id 必须原样带回,供控制面清理待执行重启请求。
//! 结果只回脱敏短摘要,不回宿主明文细节。
//! 新增 settings 字段时同步更新 test_settings 构造器。
//! 本文件前十行中文注释满足仓库规则。

use tempfile::tempdir;

use super::super::reboot::execute_reboot_task;
use super::support::test_settings;
use crate::client::RebootTask;

const REQUEST_ID: &str = "00000000-0000-0000-0000-0000000009f0";

#[tokio::test]
async fn test_execute_reboot_task_reboots_when_all_safety_checks_pass() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // 三项自检全部退出 0=满足。
    settings.reboot_check_grub_default_command = "true".to_string();
    settings.reboot_check_container_restart_command = "true".to_string();
    settings.reboot_check_docker_enabled_command = "true".to_string();
    // 重启命令用写标记代替真 reboot,验证确实被下达。
    let marker = dir.path().join("rebooted");
    settings.host_reboot_command = format!("printf x > '{}'", marker.display());

    let result = execute_reboot_task(
        &settings,
        RebootTask {
            request_id: REQUEST_ID.to_string(),
        },
    )
    .await;

    assert_eq!(result.request_id, REQUEST_ID);
    assert_eq!(result.status, "success");
    assert!(marker.exists(), "自检全过应下达重启命令");
}

#[tokio::test]
async fn test_execute_reboot_task_refuses_reboot_when_grub_check_fails() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // GRUB 自检退出非 0=不满足:必须拒绝重启。
    settings.reboot_check_grub_default_command = "false".to_string();
    settings.reboot_check_container_restart_command = "true".to_string();
    settings.reboot_check_docker_enabled_command = "true".to_string();
    let marker = dir.path().join("rebooted");
    settings.host_reboot_command = format!("printf x > '{}'", marker.display());

    let result = execute_reboot_task(
        &settings,
        RebootTask {
            request_id: REQUEST_ID.to_string(),
        },
    )
    .await;

    assert_eq!(result.request_id, REQUEST_ID);
    assert_eq!(result.status, "failed");
    assert!(
        result.message.contains("GRUB"),
        "失败原因应点明 GRUB 自检未过"
    );
    assert!(!marker.exists(), "自检未过绝不应下达重启命令");
}

#[tokio::test]
async fn test_execute_reboot_task_refuses_reboot_when_container_check_fails() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.reboot_check_grub_default_command = "true".to_string();
    // 容器自启自检不满足:必须拒绝重启。
    settings.reboot_check_container_restart_command = "false".to_string();
    settings.reboot_check_docker_enabled_command = "true".to_string();
    let marker = dir.path().join("rebooted");
    settings.host_reboot_command = format!("printf x > '{}'", marker.display());

    let result = execute_reboot_task(
        &settings,
        RebootTask {
            request_id: REQUEST_ID.to_string(),
        },
    )
    .await;

    assert_eq!(result.status, "failed");
    assert!(!marker.exists());
}

#[tokio::test]
async fn test_execute_reboot_task_success_without_real_reboot_when_command_empty() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    // 自检命令全空:按满足放行;重启命令也空:回报成功但不真重启。
    settings.reboot_check_grub_default_command = String::new();
    settings.reboot_check_container_restart_command = String::new();
    settings.reboot_check_docker_enabled_command = String::new();
    settings.host_reboot_command = String::new();

    let result = execute_reboot_task(
        &settings,
        RebootTask {
            request_id: REQUEST_ID.to_string(),
        },
    )
    .await;

    assert_eq!(result.status, "success");
    assert!(result.message.contains("未实际重启"));
}
