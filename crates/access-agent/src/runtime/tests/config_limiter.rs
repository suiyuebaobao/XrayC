//! 本模块覆盖配置应用路径中的限速器前置检查和 dry-run 命令计划。
//! 正限速配置必须先通过 limiter，不能在 disabled、dry-run 或命令失败时
//! 替换 Xray 配置。

use std::fs;
use std::sync::OnceLock;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;
use tokio::sync::Mutex;
use xrayc_xray_config::UserRateLimit;

use crate::client::AgentClient;
use crate::config::RuntimeCore;

use super::super::apply::apply_config;
use super::super::limiter::reconcile_limiter;
use super::support::{sample_config, spawn_one_shot_server, test_settings};

static PATH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test]
async fn test_apply_config_writes_limiter_plan_when_dry_run_enabled() {
    let base_url = "http://127.0.0.1:1".to_string();
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    settings.rate_limiter_enabled = true;
    settings.rate_limiter_dry_run = true;
    settings.rate_limiter_interface = "eth0".to_string();
    settings.rate_limiter_ifb_interface = "ifb-xrayc".to_string();
    settings.rate_limiter_plan_path = dir.path().join("limiter-plan.sh");
    let mut config = sample_config();
    config.rate_limits = vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 25_000_000,
        rate_limit_up_bps: 25_000_000,
        rate_limit_down_bps: 25_000_000,
        mark: 65_537,
        class_id: 101,
    }];

    reconcile_limiter(&settings, &config.rate_limits, true)
        .await
        .expect("limiter dry-run plan is written");

    let rendered = fs::read_to_string(&settings.rate_limiter_plan_path)
        .expect("limiter dry-run plan was written");
    assert!(rendered
        .contains("tc class replace dev eth0 parent 1: classid 1:101 htb rate 25mbit ceil 25mbit"));
}

#[tokio::test]
async fn test_apply_config_rejects_rate_limited_config_when_limiter_disabled() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let settings = test_settings(base_url, dir.path());
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");
    let mut config = sample_config();
    config.rate_limits = single_user_limit();

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v-limiter-disabled",
        &config,
    )
    .await
    .expect("limiter disabled is reported to control plane");

    assert!(!applied);
    assert!(
        !settings.xray_config_path.exists(),
        "xray config must not be written when limiter is disabled"
    );
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""success":false"#));
    assert!(request.contains("rate limiter"));
}

#[tokio::test]
async fn test_apply_config_rejects_rate_limited_config_when_limiter_dry_run_enabled() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    settings.rate_limiter_enabled = true;
    settings.rate_limiter_dry_run = true;
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");
    let mut config = sample_config();
    config.rate_limits = single_user_limit();

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v-limiter-dry-run",
        &config,
    )
    .await
    .expect("limiter dry-run is reported to control plane");

    assert!(!applied);
    assert!(
        !settings.xray_config_path.exists(),
        "xray config must not be written when limiter dry-run is enabled"
    );
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""success":false"#));
    assert!(request.contains("dry-run"));
}

#[tokio::test]
async fn test_apply_config_does_not_replace_xray_when_limiter_plan_write_fails() {
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    settings.rate_limiter_enabled = true;
    let blocked_parent = dir.path().join("not-a-directory");
    fs::write(&blocked_parent, "file").expect("write blocking file");
    settings.rate_limiter_plan_path = blocked_parent.join("limiter-plan.sh");
    fs::write(&settings.xray_config_path, "previous-good-config").expect("write previous config");
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");
    let mut config = sample_config();
    config.rate_limits = single_user_limit();

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v-limiter-plan-fails",
        &config,
    )
    .await
    .expect("limiter failure is reported to control plane");

    assert!(!applied);
    assert_eq!(
        fs::read_to_string(&settings.xray_config_path).expect("config restored"),
        "previous-good-config"
    );
    let request = request.await.expect("server task joins");
    assert!(request.contains(r#""success":false"#));
}

#[tokio::test]
async fn test_apply_config_replaces_xray_and_degrades_when_limiter_command_fails() {
    // P1-2 修复:限速整形命令(tc/iptables)失败时**优雅降级**——跳过整形但仍继续 apply、
    // 替换 xray 配置并 reload,绝不让整次 apply 中止(否则新配置不落地、节点永不收敛)。
    // 对比历史行为(命令失败即整次 apply 失败、不替换 xray、上报 success:false)。
    let _path_guard = PATH_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (base_url, request) = spawn_one_shot_server().await;
    let dir = tempdir().expect("tempdir");
    install_fake_limiter_commands(dir.path().join("bin"), false);
    let original_path = std::env::var("PATH").unwrap_or_default();
    let _env_guard = EnvVarGuard::set(
        "PATH",
        format!("{}:{original_path}", dir.path().join("bin").display()),
    );
    let mut settings = test_settings(base_url, dir.path());
    settings.rate_limiter_enabled = true;
    fs::write(&settings.xray_config_path, "previous-good-config").expect("write previous config");
    let client =
        AgentClient::new(&settings.control_plane_url, &settings.node_token).expect("client builds");
    let mut config = sample_config();
    config.rate_limits = single_user_limit();

    let applied = apply_config(
        &client,
        &settings,
        RuntimeCore::Xray,
        "v-limiter-command-fails",
        &config,
    )
    .await
    .expect("apply succeeds after limiter degradation");

    assert!(
        applied,
        "限速命令失败应降级并继续 apply(不中止整次配置下发)"
    );
    assert_ne!(
        fs::read_to_string(&settings.xray_config_path).expect("xray config written"),
        "previous-good-config",
        "降级后新 xray 配置应已写入(不再是旧配置)"
    );
    let request = request.await.expect("server task joins");
    assert!(
        request.contains(r#""success":true"#),
        "限速降级不影响 apply 本身成功,应上报 success:true"
    );
}

#[tokio::test]
async fn test_apply_config_dry_run_simulates_independent_multi_user_limiter_rules() {
    let base_url = "http://127.0.0.1:1".to_string();
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings(base_url, dir.path());
    settings.rate_limiter_enabled = true;
    settings.rate_limiter_dry_run = true;
    settings.rate_limiter_interface = "eth0".to_string();
    settings.rate_limiter_ifb_interface = "ifb-xrayc".to_string();
    settings.rate_limiter_plan_path = dir.path().join("multi-user-limiter-plan.sh");
    let mut config = sample_config();
    config.rate_limits = vec![
        UserRateLimit {
            user_id: "user-b".to_string(),
            xray_user_key: "u-b@xrayc.local".to_string(),
            rate_limit_bps: 50_000_000,
            rate_limit_up_bps: 50_000_000,
            rate_limit_down_bps: 50_000_000,
            mark: 65_538,
            class_id: 102,
        },
        UserRateLimit {
            user_id: "user-a".to_string(),
            xray_user_key: "u-a@xrayc.local".to_string(),
            rate_limit_bps: 25_000_000,
            rate_limit_up_bps: 25_000_000,
            rate_limit_down_bps: 25_000_000,
            mark: 65_537,
            class_id: 101,
        },
        UserRateLimit {
            user_id: "user-unlimited".to_string(),
            xray_user_key: "u-unlimited@xrayc.local".to_string(),
            rate_limit_bps: 0,
            rate_limit_up_bps: 0,
            rate_limit_down_bps: 0,
            mark: 65_539,
            class_id: 103,
        },
    ];

    reconcile_limiter(&settings, &config.rate_limits, true)
        .await
        .expect("limiter dry-run plan is written");

    let rendered = fs::read_to_string(&settings.rate_limiter_plan_path)
        .expect("limiter dry-run plan was written");
    assert!(rendered
        .contains("tc class replace dev eth0 parent 1: classid 1:101 htb rate 25mbit ceil 25mbit"));
    assert!(rendered.contains(
        "tc class replace dev ifb-xrayc parent 2: classid 2:101 htb rate 25mbit ceil 25mbit"
    ));
    assert!(rendered
        .contains("tc class replace dev eth0 parent 1: classid 1:102 htb rate 50mbit ceil 50mbit"));
    assert!(rendered.contains(
        "tc class replace dev ifb-xrayc parent 2: classid 2:102 htb rate 50mbit ceil 50mbit"
    ));
    assert!(rendered.contains(
        "tc filter replace dev eth0 parent 1: protocol ip prio 101 handle 65537 fw flowid 1:101"
    ));
    assert!(rendered.contains(
        "tc filter replace dev ifb-xrayc parent 2: protocol ip prio 102 handle 65538 fw flowid 2:102"
    ));
    assert!(rendered.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65537 -j CONNMARK --save-mark"
    ));
    assert!(rendered.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65538 -j CONNMARK --save-mark"
    ));
    // 下行整形必须随真实 apply 路径一并落盘：缺这条回程不进 IFB、下载不限速。
    assert!(rendered.contains(
        "tc filter add dev eth0 parent ffff: protocol ip prio 32766 matchall action connmark action mirred egress redirect dev ifb-xrayc"
    ));
    assert!(rendered.contains("modprobe act_connmark || true"));
    assert!(rendered
        .contains("iptables -t mangle -A XRAYC_LIMITER_PREROUTING -j CONNMARK --restore-mark"));
    assert!(!rendered.contains("1:103"));
    assert!(!rendered.contains("2:103"));
    assert!(!rendered.contains("--mark 65539"));
    assert!(!rendered.contains("protocol tcp"));
    assert!(!rendered.contains("--protocol tcp"));
    assert!(!rendered.contains("-p tcp"));

    let user_a_index = rendered.find("classid 1:101").expect("user-a class");
    let user_b_index = rendered.find("classid 1:102").expect("user-b class");
    assert!(user_a_index < user_b_index);
}

fn single_user_limit() -> Vec<UserRateLimit> {
    vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 25_000_000,
        rate_limit_up_bps: 25_000_000,
        rate_limit_down_bps: 25_000_000,
        mark: 65_537,
        class_id: 101,
    }]
}

fn install_fake_limiter_commands(bin_dir: std::path::PathBuf, tc_succeeds: bool) {
    fs::create_dir_all(&bin_dir).expect("create fake bin");
    write_executable(bin_dir.join("iptables"), "#!/bin/sh\nexit 0\n");
    write_executable(bin_dir.join("ip"), "#!/bin/sh\nexit 0\n");
    write_executable(bin_dir.join("modprobe"), "#!/bin/sh\nexit 0\n");
    let tc_exit = if tc_succeeds { 0 } else { 1 };
    write_executable(bin_dir.join("tc"), &format!("#!/bin/sh\nexit {tc_exit}\n"));
}

fn write_executable(path: std::path::PathBuf, content: &str) {
    fs::write(&path, content).expect("write fake command");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod fake command");
    }
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            std::env::set_var(self.key, original);
        } else {
            std::env::remove_var(self.key);
        }
    }
}
