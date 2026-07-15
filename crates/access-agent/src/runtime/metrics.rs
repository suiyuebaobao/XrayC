//! 本模块负责中转入口指标聚合。
//! 它将 Xray 用户流量快照和访问日志会话合并为控制面需要的线路
//! 级在线用户、连接数、客户端数与上下行速率。

use std::collections::{BTreeMap, HashMap, HashSet};

use xrayc_xray_config::{parse_stats_user_email, AccessConfig};

use crate::client::{AccessLineMetric, AccessUserSession, TrafficSnapshot};

#[derive(Debug, Default)]
pub(super) struct MetricsState {
    previous: HashMap<String, PreviousTrafficCounter>,
}

#[derive(Debug, Clone)]
struct PreviousTrafficCounter {
    uplink_bytes: u64,
    downlink_bytes: u64,
    captured_at_unix: i64,
}

impl MetricsState {
    /// 配置刷新后按新配置裁剪速率基线，而不是整体清空。
    /// 速率 = (本次累计 - 上次累计) / 间隔，依赖 `previous` 里保存的上一次累计值。
    /// 历史实现只要心跳带配置（`replace_active_config`）就把整个 MetricsState 重置，
    /// 导致每次刷新后第一拍速率读为 0，/metrics 实时速率被清零，看起来像断流。
    /// 这里只丢弃"新配置里已不存在的用户/线路"对应的过期基线（这些基线确实失效了），
    /// 仍在配置中的用户保留基线，确保速率连续。返回是否真的删过基线，便于上层判断。
    pub(super) fn retain_for_active_config(
        &mut self,
        active_config: Option<&AccessConfig>,
    ) -> bool {
        let valid_keys = configured_counter_keys(active_config);
        let before = self.previous.len();
        // 没有任何配置用户时（例如下发空配置剔除全部用户），基线全部失效，直接清空。
        self.previous.retain(|key, _| valid_keys.contains(key));
        before != self.previous.len()
    }

    pub(super) fn build_metrics(
        &mut self,
        access_node_id: &str,
        active_config: Option<&AccessConfig>,
        snapshots: &[TrafficSnapshot],
        sessions: &[AccessUserSession],
        collected_at_unix: i64,
    ) -> Vec<AccessLineMetric> {
        let mut by_line = BTreeMap::<String, LineMetricAccumulator>::new();
        seed_configured_lines(&mut by_line, active_config);
        apply_session_metrics(&mut by_line, sessions);

        for snapshot in snapshots {
            let Some(access_line_id) = snapshot.access_line_id.as_deref() else {
                continue;
            };
            let key = metric_counter_key(access_line_id, &snapshot.xray_user_key);
            let previous = self.previous.get(&key);
            let elapsed = previous
                .map(|counter| {
                    collected_at_unix
                        .saturating_sub(counter.captured_at_unix)
                        .max(1) as u64
                })
                .unwrap_or(1);
            let uplink_delta = previous
                .map(|counter| snapshot.uplink_bytes.saturating_sub(counter.uplink_bytes))
                .unwrap_or(0);
            let downlink_delta = previous
                .map(|counter| {
                    snapshot
                        .downlink_bytes
                        .saturating_sub(counter.downlink_bytes)
                })
                .unwrap_or(0);

            let accumulator = by_line.entry(access_line_id.to_owned()).or_default();
            accumulator.uplink_rate_bps += bytes_per_second_to_bits(uplink_delta, elapsed);
            accumulator.downlink_rate_bps += bytes_per_second_to_bits(downlink_delta, elapsed);

            self.previous.insert(
                key,
                PreviousTrafficCounter {
                    uplink_bytes: snapshot.uplink_bytes,
                    downlink_bytes: snapshot.downlink_bytes,
                    captured_at_unix: collected_at_unix,
                },
            );
        }

        by_line
            .into_iter()
            .map(|(access_line_id, accumulator)| AccessLineMetric {
                access_node_id: access_node_id.to_owned(),
                access_line_id,
                online_users: accumulator.online_users,
                active_connections: accumulator.active_connections,
                unique_client_ips: accumulator.seen_client_ips.len() as u64,
                uplink_rate_bps: accumulator.uplink_rate_bps,
                downlink_rate_bps: accumulator.downlink_rate_bps,
                collected_at_unix,
            })
            .collect()
    }
}

#[derive(Debug, Default)]
struct LineMetricAccumulator {
    seen_users: HashSet<String>,
    seen_client_ips: HashSet<String>,
    online_users: u64,
    active_connections: u64,
    uplink_rate_bps: u64,
    downlink_rate_bps: u64,
}

fn seed_configured_lines(
    by_line: &mut BTreeMap<String, LineMetricAccumulator>,
    active_config: Option<&AccessConfig>,
) {
    if let Some(config) = active_config {
        for line in &config.access_lines {
            by_line.entry(line.report_line_id().to_owned()).or_default();
            for user in &line.users {
                if let Some((access_line_id, _)) = parse_stats_user_email(&user.email) {
                    by_line.entry(access_line_id).or_default();
                }
            }
        }
    }
}

fn metric_counter_key(access_line_id: &str, xray_user_key: &str) -> String {
    format!("{access_line_id}\n{xray_user_key}")
}

/// 从当前活动配置推导出所有仍然有效的速率基线 key。
/// 基线 key 与流量快照一致：由 `parse_stats_user_email(user.email)` 还原出的
/// `(line_id, xray_user_key)` 拼成 `{line_id}\n{xray_user_key}`。配置里没有的
/// 用户即视为已被剔除，其基线属于过期数据，刷新后应丢弃。
fn configured_counter_keys(active_config: Option<&AccessConfig>) -> HashSet<String> {
    let mut keys = HashSet::new();
    if let Some(config) = active_config {
        for line in &config.access_lines {
            for user in &line.users {
                if let Some((access_line_id, xray_user_key)) = parse_stats_user_email(&user.email) {
                    keys.insert(metric_counter_key(&access_line_id, &xray_user_key));
                }
            }
        }
    }
    keys
}

fn bytes_per_second_to_bits(bytes: u64, seconds: u64) -> u64 {
    bytes.saturating_mul(8) / seconds.max(1)
}

fn apply_session_metrics(
    by_line: &mut BTreeMap<String, LineMetricAccumulator>,
    sessions: &[AccessUserSession],
) {
    for session in sessions {
        let accumulator = by_line.entry(session.access_line_id.clone()).or_default();
        if accumulator.seen_users.insert(session.xray_user_key.clone()) {
            accumulator.online_users += 1;
        }
        if !session.client_ip_hash.is_empty() {
            accumulator
                .seen_client_ips
                .insert(session.client_ip_hash.clone());
        }
        accumulator.active_connections = accumulator
            .active_connections
            .saturating_add(session.active_connection_count.max(1));
    }
}
