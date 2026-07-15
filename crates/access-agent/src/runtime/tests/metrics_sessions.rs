//! 本模块测试运行时指标、会话和流量积压。
//! 用例覆盖速率差分、访问日志会话匿名化，以及失败流量快照落盘后
//! 再次合并上报的行为。

use std::fs;

use tempfile::tempdir;

use crate::client::{AccessUserSession, TrafficSnapshot};

use super::super::metrics::MetricsState;
use super::super::sessions::{collect_session_snapshot, SessionState};
use super::super::traffic_backlog::TrafficBacklog;
use super::support::{sample_config, test_settings};
use xrayc_xray_config::{stats_user_email, AccessUser};

#[test]
fn test_metrics_state_builds_rates_per_access_line() {
    let mut state = MetricsState::default();
    let first = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 100,
        downlink_bytes: 200,
        captured_at_unix: 10,
    }];
    let second = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 150,
        downlink_bytes: 300,
        captured_at_unix: 20,
    }];
    let sessions = vec![AccessUserSession {
        access_line_id: "line-a".to_owned(),
        xray_user_key: "user-1".to_owned(),
        client_ip: "198.51.100.10".to_owned(),
        client_ip_hash: "sha256:test-client".to_owned(),
        active_connection_count: 1,
        started_at_unix: 10,
        last_seen_at_unix: 20,
    }];

    let initial = state.build_metrics("node-a", Some(&sample_config()), &first, &sessions, 10);
    let metrics = state.build_metrics("node-a", Some(&sample_config()), &second, &sessions, 20);

    assert_eq!(initial[0].uplink_rate_bps, 0);
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].access_node_id, "node-a");
    assert_eq!(metrics[0].access_line_id, "line-a");
    assert_eq!(metrics[0].online_users, 1);
    assert_eq!(metrics[0].active_connections, 1);
    assert_eq!(metrics[0].unique_client_ips, 1);
    assert_eq!(metrics[0].uplink_rate_bps, 40);
    assert_eq!(metrics[0].downlink_rate_bps, 80);
    assert_eq!(metrics[0].collected_at_unix, 20);
}

/// 构造一条速率基线，配置用户 email 用 stats 格式，便于 retain 命中。
fn config_with_stats_user(line_id: &str, xray_user_key: &str) -> xrayc_xray_config::AccessConfig {
    let mut config = sample_config();
    config.access_lines[0].id = line_id.to_owned();
    config.access_lines[0].source_line_id = line_id.to_owned();
    config.access_lines[0].users[0].xray_user_key = xray_user_key.to_owned();
    config.access_lines[0].users[0].email = stats_user_email(line_id, xray_user_key);
    config
}

#[test]
fn test_retain_for_active_config_keeps_rate_baseline_for_surviving_users() {
    // 配置刷新但用户仍在配置里时，速率基线必须保留，否则刷新后第一拍速率清成 0。
    let config = config_with_stats_user("line-a", "user-1");
    let mut state = MetricsState::default();
    let first = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 100,
        downlink_bytes: 200,
        captured_at_unix: 10,
    }];
    state.build_metrics("node-a", Some(&config), &first, &[], 10);

    // 模拟一次带配置的刷新：用户没变，基线应当保留。
    let changed = state.retain_for_active_config(Some(&config));
    assert!(!changed, "未变更的用户集合不应删除任何基线");

    let second = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 150,
        downlink_bytes: 300,
        captured_at_unix: 20,
    }];
    let metrics = state.build_metrics("node-a", Some(&config), &second, &[], 20);

    assert_eq!(metrics.len(), 1);
    // 基线保留 → 速率按差值计算，而不是 0。
    assert_eq!(metrics[0].uplink_rate_bps, 40);
    assert_eq!(metrics[0].downlink_rate_bps, 80);
}

#[test]
fn test_retain_for_active_config_drops_baseline_for_removed_users() {
    // 用户被剔除时其基线确实失效，应当丢弃。
    let config = config_with_stats_user("line-a", "user-1");
    let mut state = MetricsState::default();
    let first = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 100,
        downlink_bytes: 200,
        captured_at_unix: 10,
    }];
    state.build_metrics("node-a", Some(&config), &first, &[], 10);

    // 刷新成只剩另一个用户的配置：原用户基线应被裁掉。
    let other = config_with_stats_user("line-a", "user-2");
    let changed = state.retain_for_active_config(Some(&other));
    assert!(changed, "剔除用户后应删除其过期基线");

    // 空配置（全部剔除）时基线也应清空。
    state.build_metrics("node-a", Some(&config), &first, &[], 30);
    let cleared = state.retain_for_active_config(None);
    assert!(cleared, "空配置应清空所有基线");
}

#[test]
fn test_access_log_sessions_build_user_sessions_and_metrics() {
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_owned(), dir.path());
    fs::write(
        &settings.xray_access_log_path,
        "2026/05/18 00:00:00 from tcp:198.51.100.10:43210 accepted tcp:example.com:443 [user-1@xrayc]\n",
    )
    .expect("write access log");
    let mut session_state = SessionState::default();

    let sessions = collect_session_snapshot(
        &settings,
        Some(&sample_config()),
        &mut session_state,
        1_700_000_000,
    );
    let mut metrics_state = MetricsState::default();
    let metrics = metrics_state.build_metrics(
        "node-a",
        Some(&sample_config()),
        &[],
        &sessions,
        1_700_000_000,
    );

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].access_line_id, "line-a");
    assert_eq!(sessions[0].xray_user_key, "user-1");
    assert_eq!(sessions[0].client_ip, "198.51.100.10");
    assert!(sessions[0].client_ip_hash.starts_with("sha256:"));
    assert!(!sessions[0].client_ip_hash.contains("198.51.100.10"));
    assert_eq!(metrics[0].online_users, 1);
    assert_eq!(metrics[0].active_connections, 1);
    assert_eq!(metrics[0].unique_client_ips, 1);
}

#[test]
fn test_access_log_sessions_parse_xray_email_suffix() {
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_owned(), dir.path());
    fs::write(
        &settings.xray_access_log_path,
        "2026/06/04 12:13:24 from tcp:198.51.100.10:43210 accepted tcp:example.com:443 [access-line -> exit-line] email: user-1@xrayc\n",
    )
    .expect("write access log");
    let mut session_state = SessionState::default();

    let sessions = collect_session_snapshot(
        &settings,
        Some(&sample_config()),
        &mut session_state,
        1_700_000_000,
    );

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].access_line_id, "line-a");
    assert_eq!(sessions[0].xray_user_key, "user-1");
    assert_eq!(sessions[0].client_ip, "198.51.100.10");
}

#[test]
fn test_runtime_line_sessions_report_source_line_id() {
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_owned(), dir.path());
    fs::write(
        &settings.xray_access_log_path,
        "2026/05/18 00:00:00 from tcp:198.51.100.10:43210 accepted tcp:example.com:443 [user-1@xrayc]\n",
    )
    .expect("write access log");
    let mut config = sample_config();
    config.access_lines[0].id = "line-a-user-runtime".to_string();
    config.access_lines[0].source_line_id = "line-a".to_string();
    let mut session_state = SessionState::default();

    let sessions =
        collect_session_snapshot(&settings, Some(&config), &mut session_state, 1_700_000_000);
    let mut metrics_state = MetricsState::default();
    let metrics =
        metrics_state.build_metrics("node-a", Some(&config), &[], &sessions, 1_700_000_000);

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].access_line_id, "line-a");
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].access_line_id, "line-a");
}

#[test]
fn test_merged_runtime_line_sessions_use_binding_id_from_stats_email() {
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_owned(), dir.path());
    let second_email = stats_user_email("line-b", "user-1");
    fs::write(
        &settings.xray_access_log_path,
        format!("2026/05/18 00:00:00 from tcp:198.51.100.10:43210 accepted tcp:example.com:443 [{second_email}]\n"),
    )
    .expect("write access log");
    let mut config = sample_config();
    config.access_lines[0].id = "line-a".to_string();
    config.access_lines[0].source_line_id = "line-a".to_string();
    config.access_lines[0].users[0].email = stats_user_email("line-a", "user-1");
    config.access_lines[0].users.push(AccessUser {
        xray_user_key: "user-1".to_string(),
        credential: "22222222-2222-2222-2222-222222222222".to_string(),
        email: second_email,
        level: 0,
    });
    let mut session_state = SessionState::default();

    let sessions =
        collect_session_snapshot(&settings, Some(&config), &mut session_state, 1_700_000_000);

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].access_line_id, "line-b");
    assert_eq!(sessions[0].xray_user_key, "user-1");
}

#[test]
fn test_merged_runtime_line_metrics_seed_all_binding_ids_from_stats_email() {
    let mut config = sample_config();
    config.access_lines[0].id = "line-a".to_string();
    config.access_lines[0].source_line_id = "line-a".to_string();
    config.access_lines[0].users[0].email = stats_user_email("line-a", "user-1");
    config.access_lines[0].users.push(AccessUser {
        xray_user_key: "user-1".to_string(),
        credential: "22222222-2222-2222-2222-222222222222".to_string(),
        email: stats_user_email("line-b", "user-1"),
        level: 0,
    });
    let mut metrics_state = MetricsState::default();

    let metrics = metrics_state.build_metrics("node-a", Some(&config), &[], &[], 1_700_000_000);
    let metric_ids = metrics
        .iter()
        .map(|metric| metric.access_line_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(metric_ids, vec!["line-a", "line-b"]);
    assert!(metrics.iter().all(|metric| metric.online_users == 0));
    assert!(metrics.iter().all(|metric| metric.active_connections == 0));
    assert!(metrics
        .iter()
        .all(|metric| metric.uplink_rate_bps == 0 && metric.downlink_rate_bps == 0));
}

#[test]
fn test_access_log_session_requires_exact_bracket_email_match() {
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_owned(), dir.path());
    fs::write(
        &settings.xray_access_log_path,
        "2026/05/18 00:00:00 from tcp:198.51.100.10:43210 accepted tcp:example.com:443 [not-user-1@xrayc]\n",
    )
    .expect("write access log");
    let mut session_state = SessionState::default();

    let sessions = collect_session_snapshot(
        &settings,
        Some(&sample_config()),
        &mut session_state,
        1_700_000_000,
    );

    assert!(sessions.is_empty());
}

#[test]
fn test_traffic_backlog_persists_failed_reports() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("traffic-backlog.json");
    let mut backlog = TrafficBacklog::load(&path);
    let snapshots = vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 10,
        downlink_bytes: 20,
        captured_at_unix: 1_700_000_000,
    }];

    backlog.save(&snapshots).expect("backlog saves");
    let restored = TrafficBacklog::load(&path);
    let merged = restored.merge_with_current(vec![TrafficSnapshot {
        access_line_id: Some("line-a".to_owned()),
        xray_user_key: "user-1".to_owned(),
        uplink_bytes: 30,
        downlink_bytes: 40,
        captured_at_unix: 1_700_000_060,
    }]);

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].uplink_bytes, 10);
    assert_eq!(merged[1].uplink_bytes, 30);
}
