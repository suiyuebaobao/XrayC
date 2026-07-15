//! 本文件实现 access-agent 的 Xray 流量采集。
//! 采集器通过本机 `xray api statsquery` 读取用户累计计数，并把
//! Xray 统计 email 还原成中心服务需要的线路 ID 与用户键。

use std::collections::BTreeMap;

use anyhow::{bail, Context};
use serde_json::Value;
use tokio::process::Command;
use xrayc_xray_config::parse_stats_user_email;

use crate::client::TrafficSnapshot;
use crate::config::AgentSettings;

#[derive(Debug, Default)]
struct UserCounter {
    access_line_id: Option<String>,
    xray_user_key: String,
    uplink_bytes: u64,
    downlink_bytes: u64,
}

// Xray statsquery 的二进制与 server 地址:读取 Xray 自身 Stats API。
pub fn xray_stats_query_target(settings: &AgentSettings) -> (String, String) {
    (
        settings.xray_binary.clone(),
        settings.xray_api_server.clone(),
    )
}

// 通过本机 `xray api statsquery` 采集用户累计计数,解析复用 parse_stats_output,
// 统一接入既有计费快照差值管线。
pub async fn collect_user_traffic_snapshots(
    settings: &AgentSettings,
    captured_at_unix: i64,
) -> anyhow::Result<Vec<TrafficSnapshot>> {
    let (binary, server) = xray_stats_query_target(settings);

    let output = Command::new(&binary)
        .args([
            "api",
            "statsquery",
            "--server",
            &server,
            "-pattern",
            "user>>>",
        ])
        .output()
        .await
        .with_context(|| format!("execute {binary}"))?;

    if !output.status.success() {
        bail!(
            "statsquery failed: stderr redacted ({} bytes)",
            output.stderr.len()
        );
    }

    let stdout = String::from_utf8(output.stdout).context("decode statsquery output")?;
    parse_stats_output(&stdout, captured_at_unix)
}

pub fn parse_stats_output(
    output: &str,
    captured_at_unix: i64,
) -> anyhow::Result<Vec<TrafficSnapshot>> {
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }

    let value = serde_json::from_str::<Value>(output).context("parse xray stats json output")?;
    let stats = value
        .get("stat")
        .or_else(|| value.get("stats"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut counters = BTreeMap::<String, UserCounter>::new();
    for stat in stats {
        let Some(name) = stat.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(counter_value) = stat_value(&stat) else {
            continue;
        };
        absorb_stat(&mut counters, name, counter_value);
    }

    Ok(counters
        .into_values()
        .map(|counter| TrafficSnapshot {
            access_line_id: counter.access_line_id,
            xray_user_key: counter.xray_user_key,
            uplink_bytes: counter.uplink_bytes,
            downlink_bytes: counter.downlink_bytes,
            captured_at_unix,
        })
        .collect())
}

fn stat_value(stat: &Value) -> Option<u64> {
    let value = stat.get("value")?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

fn absorb_stat(counters: &mut BTreeMap<String, UserCounter>, name: &str, value: u64) {
    let parts = name.split(">>>").collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "user" || parts[2] != "traffic" {
        return;
    }

    let Some((access_line_id, xray_user_key)) = parse_stats_user_email(parts[1]) else {
        return;
    };
    let entry_key = format!("{access_line_id}::{xray_user_key}");
    let counter = counters.entry(entry_key).or_insert_with(|| UserCounter {
        access_line_id: Some(access_line_id),
        xray_user_key,
        ..UserCounter::default()
    });

    match parts[3] {
        "uplink" => counter.uplink_bytes = value,
        "downlink" => counter.downlink_bytes = value,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use xrayc_xray_config::stats_user_email;

    use crate::config::{AgentSettings, RuntimeCore};

    use super::*;

    // 当前只剩 Xray 单内核，统一以 RuntimeCore::Xray 构造测试设置。

    #[test]
    fn test_parse_stats_output_restores_line_and_user_key() {
        let email = stats_user_email("line-a", "u-1@xrayc.local");
        let output = serde_json::json!({
            "stat": [
                {"name": format!("user>>>{email}>>>traffic>>>uplink"), "value": 100_u64},
                {"name": format!("user>>>{email}>>>traffic>>>downlink"), "value": "200"}
            ]
        })
        .to_string();

        let snapshots = parse_stats_output(&output, 1_700_000_000).expect("stats parse");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].access_line_id.as_deref(), Some("line-a"));
        assert_eq!(snapshots[0].xray_user_key, "u-1@xrayc.local");
        assert_eq!(snapshots[0].uplink_bytes, 100);
        assert_eq!(snapshots[0].downlink_bytes, 200);
        assert_eq!(snapshots[0].captured_at_unix, 1_700_000_000);
    }

    #[test]
    fn test_parse_stats_output_ignores_unscoped_user_stats() {
        let output = r#"{
            "stat": [
                {"name": "user>>>u-legacy@xrayc.local>>>traffic>>>uplink", "value": 10},
                {"name": "user>>>u-legacy@xrayc.local>>>traffic>>>downlink", "value": 20}
            ]
        }"#;

        let snapshots = parse_stats_output(output, 10).expect("stats parse");

        assert!(snapshots.is_empty());
    }

    #[test]
    fn test_parse_stats_output_accepts_legacy_line_prefix() {
        let output = r#"{
            "stat": [
                {"name": "user>>>rp-line-line-a--u-legacy@xrayc.local>>>traffic>>>uplink", "value": 10},
                {"name": "user>>>rp-line-line-a--u-legacy@xrayc.local>>>traffic>>>downlink", "value": 20}
            ]
        }"#;

        let snapshots = parse_stats_output(output, 10).expect("legacy line stats parse");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].access_line_id.as_deref(), Some("line-a"));
        assert_eq!(snapshots[0].xray_user_key, "u-legacy@xrayc.local");
        assert_eq!(snapshots[0].uplink_bytes, 10);
        assert_eq!(snapshots[0].downlink_bytes, 20);
    }

    #[test]
    fn test_parse_stats_output_accepts_empty_xray_response() {
        let snapshots = parse_stats_output("{}", 10).expect("empty xray response parses");

        assert!(snapshots.is_empty());
    }

    // 构造测试用 AgentSettings,只关心 runtime_core 与两个 statsquery target 字段,
    // 其余路径/命令填占位值,避免真实进程依赖。
    fn sample_settings(runtime_core: RuntimeCore, dir: &std::path::Path) -> AgentSettings {
        AgentSettings {
            node_id: "node-a".to_owned(),
            control_plane_url: "http://127.0.0.1:1".to_owned(),
            node_token: "token".to_owned(),
            runtime_core,
            heartbeat_interval_seconds: 30,
            traffic_interval_seconds: 60,
            session_idle_seconds: 180,
            xray_config_path: dir.join("xray.json"),
            agent_state_path: dir.join("state.json"),
            traffic_backlog_path: dir.join("backlog.json"),
            xray_access_log_path: dir.join("access.log"),
            xray_binary: "xray-binary".to_owned(),
            xray_test_command: "true".to_owned(),
            xray_reload_command: "true".to_owned(),
            xray_reclaim_command: "true".to_owned(),
            xray_container_name: String::new(),
            xray_start_command: "true".to_owned(),
            xray_stop_command: "true".to_owned(),
            xray_api_server: "127.0.0.1:10085".to_owned(),
            xray_api_listen_host: "127.0.0.1".to_owned(),
            xray_api_listen_port: 10085,
            rate_limiter_enabled: false,
            rate_limiter_dry_run: false,
            rate_limiter_interface: "eth0".to_owned(),
            rate_limiter_ifb_interface: "ifb-xrayc".to_owned(),
            rate_limiter_root_rate_bps: 10_000_000_000,
            rate_limiter_plan_path: dir.join("limiter-plan.sh"),
            tls_cert_domains: Vec::new(),
            certbot_binary: "certbot".to_owned(),
            openssl_binary: "openssl".to_owned(),
            cloudflare_api_token: String::new(),
            cf_cert_mode: "reuse_direct".to_owned(),
            cf_domain: String::new(),
            acme_email: String::new(),
            kernel_connmark_probe_command: String::new(),
            kernel_upgrade_command: String::new(),
            host_reboot_command: String::new(),
            reboot_check_grub_default_command: String::new(),
            reboot_check_container_restart_command: String::new(),
            reboot_check_docker_enabled_command: String::new(),
            host_fs_root: "/".to_owned(),
        }
    }

    #[test]
    fn test_xray_stats_query_target_uses_xray_api_server_for_xray_runtime() {
        let dir = tempdir().expect("tempdir");
        let settings = sample_settings(RuntimeCore::Xray, dir.path());

        let (binary, server) = xray_stats_query_target(&settings);

        assert_eq!(binary, "xray-binary");
        assert_eq!(server, "127.0.0.1:10085");
    }
}
