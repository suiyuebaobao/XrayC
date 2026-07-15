//! 本模块测试 Xray reload 后的陈旧实例回收逻辑。
//! 重点验证：reload 成功后回收命令会被执行（带上当前容器名与 Stats 端口环境变量），
//! 且回收命令失败时整次配置应用必须失败上报，绝不静默吞掉——
//! 因为残留旧 Xray 仍占用 Stats 端口会让计费读错进程。

use std::fs;

use tempfile::tempdir;

use crate::client::AgentClient;
use crate::config::RuntimeCore;

use super::super::apply::apply_config;
use super::support::{sample_config, spawn_one_shot_server, test_settings};

#[tokio::test]
async fn test_reload_runs_reclaim_command_with_context_env() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let marker = dir.path().join("reclaim-marker");
    let mut settings = test_settings(base_url, dir.path());
    settings.xray_container_name = "xrayc-xray-test".to_owned();
    settings.xray_api_listen_port = 10085;
    // 回收命令把当前容器名与端口写进 marker，证明 reload 成功后确实跑了回收并带上了上下文。
    settings.xray_reclaim_command = format!(
        "printf '%s %s' \"$XRAYC_XRAY_CONTAINER_NAME\" \"$XRAYC_XRAY_API_LISTEN_PORT\" > {}",
        marker.display()
    );
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v1",
        &sample_config(),
    )
    .await
    .expect("config applies");

    assert!(applied);
    let recorded = fs::read_to_string(&marker).expect("reclaim command ran");
    assert_eq!(recorded, "xrayc-xray-test 10085");
    let _ = request.await.expect("server task joins");
}

#[tokio::test]
async fn test_reclaim_failure_fails_config_apply() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    // 回收失败模拟旧 Xray 没清理掉：整次应用必须失败上报，让控制面看到节点异常。
    settings.xray_reclaim_command = "false".to_owned();
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v1",
        &sample_config(),
    )
    .await
    .expect("failure is reported to control plane");

    assert!(!applied);
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""success":false"#));
}

#[tokio::test]
async fn test_empty_reclaim_command_is_skipped() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    // 默认空回收命令（非 docker 环境/单测）直接跳过，应用照常成功。
    settings.xray_reclaim_command = String::new();
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v1",
        &sample_config(),
    )
    .await
    .expect("config applies");

    assert!(applied);
    let _ = request.await.expect("server task joins");
}
