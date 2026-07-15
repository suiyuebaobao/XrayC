//! 本模块测试 agent 对单个运行内核启停任务的本地执行。
//! 测试只调用本地临时 shell 命令，不操作真实 Docker 或系统服务。
//! 控制面下发的 request_id 必须原样带回，便于服务端清除任务。
//! 命令输出和错误不能上报敏感明文，只返回短摘要。
//! 任务按 core_type 分发到各自的 start/stop command。
//! 这里验证 Xray stop 路径走对应的 stop command。
//! 测试目录使用 tempfile，执行结束后自动清理。
//! 新增字段时应同步更新测试配置构造器。
//! 本文件前十行中文注释满足仓库规则。
//! 不要在本测试中访问公网或真实控制面。

use tempfile::tempdir;

use crate::client::{RuntimeCoreAction, RuntimeCoreTask};
use crate::config::RuntimeCore;

use super::super::kernel::KernelCapability;
use super::super::{core_control::execute_runtime_core_task, send_heartbeat};
use super::support::{spawn_sequence_server, test_settings};

#[tokio::test]
async fn test_execute_runtime_core_task_uses_core_specific_stop_command() {
    let dir = tempdir().expect("tempdir");
    let marker = dir.path().join("xray-stopped");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.xray_stop_command = format!("printf stopped > '{}'", marker.display());

    let result = execute_runtime_core_task(
        &settings,
        RuntimeCoreTask {
            request_id: "00000000-0000-0000-0000-000000000902".to_owned(),
            core_type: RuntimeCore::Xray,
            action: RuntimeCoreAction::Stop,
        },
    )
    .await
    .expect("task result is produced");

    assert!(result.success);
    assert_eq!(result.core_type, RuntimeCore::Xray);
    assert_eq!(result.action, RuntimeCoreAction::Stop);
    assert_eq!(result.request_id, "00000000-0000-0000-0000-000000000902");
    assert_eq!(
        std::fs::read_to_string(marker).expect("marker written"),
        "stopped"
    );
}

#[tokio::test]
async fn test_send_heartbeat_returns_runtime_core_tasks() {
    let (base_url, request) = spawn_sequence_server(vec![
        r#"{"accepted":true,"desired_config_version":null,"config":null,"runtime_core_tasks":[{"request_id":"00000000-0000-0000-0000-000000000903","core_type":"xray","action":"start"}]}"#,
    ])
    .await;
    let dir = tempdir().expect("tempdir");
    let settings = test_settings(base_url, dir.path());
    let client = crate::client::AgentClient::new(&settings.control_plane_url, &settings.node_token)
        .expect("client builds");

    let outcome = send_heartbeat(
        &client,
        &settings,
        std::time::Duration::from_secs(1),
        None,
        None,
        None,
        Vec::new(),
        &[],
        KernelCapability::default(),
        None,
        None,
    )
    .await
    .expect("heartbeat succeeds");

    assert_eq!(outcome.runtime_core_tasks.len(), 1);
    assert_eq!(outcome.runtime_core_tasks[0].core_type, RuntimeCore::Xray);
    assert_eq!(
        outcome.runtime_core_tasks[0].action,
        RuntimeCoreAction::Start
    );
    let requests = request.await.expect("server task joins");
    assert!(requests[0].contains(r#""runtime_core_results":[]"#));
}
