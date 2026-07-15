//! 流量快照计费服务。
//! 中转 agent 上报 Xray 累计计数器，本模块保存上一份快照。
//! 计费只对正向差值入账，并按用户剩余额度封顶。
//! 授权判断复用套餐分组展开逻辑，当前只按单层分组处理。
//! 套餐直接授权分组，已绑定线路按当前授权集合输出。
//! 本模块只处理内存 Store，不直接访问 PostgreSQL 或 HTTP。
//! 账本记录保留原始上下行差值和倍率后的扣费字节。
//! 配置刷新标记用于提示运行时移除未授权用户。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use crate::model::*;
use crate::store::MemoryStore;
// 复用订阅侧的唯一归属判定口径，避免计费授权与订阅可见性两份实现漂移。
use crate::subscription::access_line_belongs_to_group;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BillingError {
    #[error("中转入口不存在")]
    AccessLineNotFound,
    #[error("用户不存在")]
    UserNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficReport {
    pub access_line_id: Uuid,
    pub xray_user_key: String,
    pub uplink_total: u64,
    pub downlink_total: u64,
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrafficReportResult {
    pub accepted: bool,
    pub baseline_only: bool,
    pub delta_uplink: u64,
    pub delta_downlink: u64,
    pub billed_bytes: u64,
    pub config_refresh_required: bool,
}

#[derive(Debug, Clone)]
pub struct TrafficService {
    store: MemoryStore,
}

impl TrafficService {
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    pub fn apply_report(&self, report: TrafficReport) -> Result<TrafficReportResult, BillingError> {
        self.store.write(|data| {
            let line = data
                .access_lines
                .get(&report.access_line_id)
                .cloned()
                .ok_or(BillingError::AccessLineNotFound)?;
            let user = data
                .users
                .values()
                .find(|user| user.xray_user_key == report.xray_user_key)
                .cloned()
                .ok_or(BillingError::UserNotFound)?;
            let line_authorized = data
                .subscriptions
                .get(&user.id)
                .is_some_and(|subscription| {
                    user_can_use_line(data, &user, subscription, &line, report.collected_at)
                });

            let key = (report.access_line_id, report.xray_user_key.clone());
            let previous = data.snapshots.get(&key).cloned();
            let snapshot = TrafficSnapshot {
                access_line_id: report.access_line_id,
                xray_user_key: report.xray_user_key.clone(),
                uplink_total: report.uplink_total,
                downlink_total: report.downlink_total,
                collected_at: report.collected_at,
            };

            let Some(previous) = previous else {
                data.snapshots.insert(key, snapshot);
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: true,
                    delta_uplink: 0,
                    delta_downlink: 0,
                    billed_bytes: 0,
                    config_refresh_required: !line_authorized,
                });
            };

            if report.collected_at <= previous.collected_at {
                return Ok(TrafficReportResult {
                    accepted: false,
                    baseline_only: false,
                    delta_uplink: 0,
                    delta_downlink: 0,
                    billed_bytes: 0,
                    config_refresh_required: false,
                });
            }

            if report.uplink_total < previous.uplink_total
                || report.downlink_total < previous.downlink_total
            {
                data.snapshots.insert(key, snapshot);
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: true,
                    delta_uplink: 0,
                    delta_downlink: 0,
                    billed_bytes: 0,
                    config_refresh_required: false,
                });
            }

            data.snapshots.insert(key, snapshot);

            let delta_uplink = report.uplink_total - previous.uplink_total;
            let delta_downlink = report.downlink_total - previous.downlink_total;
            let delta_total = delta_uplink.saturating_add(delta_downlink);
            if delta_total == 0 || !line_authorized {
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: false,
                    delta_uplink,
                    delta_downlink,
                    billed_bytes: 0,
                    config_refresh_required: !line_authorized,
                });
            }

            let Some(subscription_view) = data.subscriptions.get(&user.id) else {
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: false,
                    delta_uplink,
                    delta_downlink,
                    billed_bytes: 0,
                    config_refresh_required: true,
                });
            };

            let Some(plan) = data.plans.get(&subscription_view.plan_id) else {
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: false,
                    delta_uplink,
                    delta_downlink,
                    billed_bytes: 0,
                    config_refresh_required: true,
                });
            };

            let multiplier = effective_billing_multiplier(data, plan, line.id);
            let Some(subscription) = data.subscriptions.get_mut(&user.id) else {
                return Ok(TrafficReportResult {
                    accepted: true,
                    baseline_only: false,
                    delta_uplink,
                    delta_downlink,
                    billed_bytes: 0,
                    config_refresh_required: true,
                });
            };
            let requested_bill = ((delta_total as f64) * multiplier).ceil() as u64;
            let remaining = subscription.remaining_bytes();
            let billed_bytes = requested_bill.min(remaining);
            subscription.add_billed(billed_bytes);
            let config_refresh_required = billed_bytes == remaining && remaining > 0;

            if billed_bytes > 0 {
                data.ledgers.push(UsageLedger {
                    access_line_id: report.access_line_id,
                    user_id: user.id,
                    xray_user_key: report.xray_user_key,
                    traffic_source: "access_line".to_string(),
                    delta_uplink,
                    delta_downlink,
                    billing_multiplier: multiplier,
                    billed_bytes,
                    collected_at: report.collected_at,
                });
            }

            Ok(TrafficReportResult {
                accepted: true,
                baseline_only: false,
                delta_uplink,
                delta_downlink,
                billed_bytes,
                config_refresh_required,
            })
        })
    }
}

fn effective_billing_multiplier(data: &crate::store::StoreData, plan: &Plan, line_id: Uuid) -> f64 {
    let mut multiplier = normalized_billing_multiplier(plan.billing_multiplier);
    let Some(line) = data.access_lines.get(&line_id) else {
        return multiplier;
    };
    for resolved in data.resolved_plan_line_groups(plan) {
        if access_line_belongs_to_group(data, line, resolved.line_group_id) {
            multiplier = multiplier.max(normalized_billing_multiplier(resolved.billing_multiplier));
        }
    }
    multiplier
}

fn normalized_billing_multiplier(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

fn user_can_use_line(
    data: &crate::store::StoreData,
    user: &User,
    subscription: &UserSubscription,
    line: &AccessLine,
    now: DateTime<Utc>,
) -> bool {
    if user.disabled
        || !line.enabled
        || !subscription.active
        || subscription.expires_at <= now
        || subscription.remaining_bytes() == 0
    {
        return false;
    }

    let Some(plan) = data.plans.get(&subscription.plan_id) else {
        return false;
    };

    for group in data.resolved_plan_line_groups(plan) {
        if access_line_belongs_to_group(data, line, group.line_group_id) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn seeded_report(store: &MemoryStore, uplink_total: u64, downlink_total: u64) -> TrafficReport {
        let (line_id, key) = store.read(|data| {
            let line_id = *data.access_lines.keys().next().unwrap();
            let key = data.users.values().next().unwrap().xray_user_key.clone();
            (line_id, key)
        });
        TrafficReport {
            access_line_id: line_id,
            xray_user_key: key,
            uplink_total,
            downlink_total,
            collected_at: Utc::now(),
        }
    }

    #[test]
    fn first_report_is_baseline_and_not_billed() {
        let store = MemoryStore::seeded();
        let service = TrafficService::new(store.clone());

        let result = service
            .apply_report(seeded_report(&store, 100, 200))
            .unwrap();

        assert!(result.baseline_only);
        assert_eq!(result.billed_bytes, 0);
        assert_eq!(store.read(|data| data.ledgers.len()), 0);
    }

    #[test]
    fn positive_delta_is_billed_once() {
        let store = MemoryStore::seeded();
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 100, 200);
        service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 400;
        second.downlink_total = 600;
        second.collected_at += Duration::seconds(10);

        let result = service.apply_report(second).unwrap();

        assert_eq!(result.delta_uplink, 300);
        assert_eq!(result.delta_downlink, 400);
        assert_eq!(result.billed_bytes, 700);
        assert_eq!(store.read(|data| data.ledgers.len()), 1);
    }

    #[test]
    fn plan_group_multiplier_uses_highest_plan_or_binding_scope() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let plan = data.plans.values_mut().next().unwrap();
            plan.billing_multiplier = 1.25;
            plan.line_groups[0].billing_multiplier = 3.0;
        });
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 10, 20);
        service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 110;
        second.downlink_total = 220;
        second.collected_at += Duration::seconds(10);

        let result = service.apply_report(second).unwrap();

        assert_eq!(result.billed_bytes, 900);
        let ledger = store.read(|data| data.ledgers.last().cloned().unwrap());
        assert_eq!(ledger.billing_multiplier, 3.0);
        assert_eq!(ledger.billed_bytes, 900);
    }

    #[test]
    fn line_removed_from_group_refreshes_without_billing_even_with_legacy_group_snapshot() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let line_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
            let line = data.access_lines.values().next().unwrap();
            let exit_endpoint_id = line.exit_endpoint_id.unwrap();
            data.line_groups
                .get_mut(&line_group_id)
                .unwrap()
                .line_ids
                .retain(|id| *id != exit_endpoint_id);
            for line in data.access_lines.values_mut() {
                line.line_group_id = Some(line_group_id);
            }
        });
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 100, 200);
        let first_result = service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 400;
        second.downlink_total = 600;
        second.collected_at += Duration::seconds(10);

        let result = service.apply_report(second).unwrap();

        assert!(first_result.baseline_only);
        assert!(first_result.config_refresh_required);
        assert_eq!(result.delta_uplink, 300);
        assert_eq!(result.delta_downlink, 400);
        assert_eq!(result.billed_bytes, 0);
        assert!(result.config_refresh_required);
        assert_eq!(store.read(|data| data.ledgers.len()), 0);
    }

    /// 回归「能连但不计费」免费流量缺口：分组迁到绑定节点模式（`binding_node_ids` 非空），
    /// 某线路未进绑定集合、但仍靠 legacy 字段 `line_group_id` 归属该分组。
    /// 统一口径后，订阅必须可见（用户能连）且计费必须授权（按差值扣费），两者一致。
    #[test]
    fn legacy_group_membership_under_binding_mode_is_both_visible_and_billed() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let line_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
            let line_id = *data.access_lines.keys().next().unwrap();
            // 切到绑定节点模式：放一个无关 id 占位，使 binding_node_ids 非空但不含该线路。
            let group = data.line_groups.get_mut(&line_group_id).unwrap();
            group.binding_node_ids = vec![Uuid::new_v4()];
            group.line_ids.clear();
            // 该线路只靠 legacy line_group_id 归属，没进 binding_node_ids。
            data.access_lines.get_mut(&line_id).unwrap().line_group_id = Some(line_group_id);
        });

        // 订阅可见性：该入口应被渲染（用户能连）。
        let yaml = store
            .read(|data| crate::generate_clash_yaml(data, "demo-token"))
            .expect("legacy 归属的入口在绑定模式下应仍然可见");
        assert!(yaml.contains("香港 01"));

        // 计费授权：同一线路必须授权并按差值扣费，不能出现免费流量。
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 100, 200);
        let first_result = service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 400;
        second.downlink_total = 600;
        second.collected_at += Duration::seconds(10);
        let result = service.apply_report(second).unwrap();

        // 基线已授权：不需要因未授权而触发配置刷新。
        assert!(first_result.baseline_only);
        assert!(!first_result.config_refresh_required);
        assert_eq!(result.delta_uplink, 300);
        assert_eq!(result.delta_downlink, 400);
        assert_eq!(result.billed_bytes, 700);
        assert!(!result.config_refresh_required);
        assert_eq!(store.read(|data| data.ledgers.len()), 1);
    }

    /// 反向一致性：绑定模式下既不在 binding_node_ids、又没有 legacy line_group_id 的线路，
    /// 订阅不可见、计费也不授权——「不可见即不计费」。
    #[test]
    fn unbound_line_without_legacy_group_under_binding_mode_is_neither_visible_nor_billed() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let line_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
            let line_id = *data.access_lines.keys().next().unwrap();
            let group = data.line_groups.get_mut(&line_group_id).unwrap();
            group.binding_node_ids = vec![Uuid::new_v4()];
            group.line_ids.clear();
            // 既不进绑定集合，也没有 legacy 归属字段。
            data.access_lines.get_mut(&line_id).unwrap().line_group_id = None;
        });

        // 订阅不可见：没有任何可输出线路。
        let render = store.read(|data| crate::generate_clash_yaml(data, "demo-token"));
        assert!(matches!(
            render,
            Err(crate::SubscriptionError::NoAvailableLines)
        ));

        // 计费不授权：正向差值也不扣费，并提示移除未授权用户。
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 100, 200);
        service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 400;
        second.downlink_total = 600;
        second.collected_at += Duration::seconds(10);
        let result = service.apply_report(second).unwrap();

        assert_eq!(result.delta_uplink, 300);
        assert_eq!(result.billed_bytes, 0);
        assert!(result.config_refresh_required);
        assert_eq!(store.read(|data| data.ledgers.len()), 0);
    }

    #[test]
    fn disabled_or_inactive_subscription_report_refreshes_without_billing() {
        let store = MemoryStore::seeded();
        let user_id = store.read(|data| *data.users.keys().next().unwrap());
        store.write(|data| {
            data.users.get_mut(&user_id).unwrap().disabled = true;
        });
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 0, 0);
        service.apply_report(first.clone()).unwrap();
        let mut second = first.clone();
        second.uplink_total = 100;
        second.collected_at += Duration::seconds(10);

        let disabled_result = service.apply_report(second).unwrap();
        assert_eq!(disabled_result.billed_bytes, 0);
        assert!(disabled_result.config_refresh_required);

        store.write(|data| {
            data.users.get_mut(&user_id).unwrap().disabled = false;
            data.subscriptions.get_mut(&user_id).unwrap().active = false;
        });
        let mut third = first;
        third.uplink_total = 200;
        third.collected_at += Duration::seconds(20);

        let inactive_result = service.apply_report(third).unwrap();
        assert_eq!(inactive_result.billed_bytes, 0);
        assert!(inactive_result.config_refresh_required);
        assert_eq!(store.read(|data| data.ledgers.len()), 0);
    }

    #[test]
    fn duplicate_or_old_report_is_ignored() {
        let store = MemoryStore::seeded();
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 100, 200);
        service.apply_report(first.clone()).unwrap();

        let result = service.apply_report(first).unwrap();

        assert!(!result.accepted);
        assert_eq!(result.billed_bytes, 0);
    }

    #[test]
    fn counter_reset_rebuilds_baseline_without_billing() {
        let store = MemoryStore::seeded();
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 1000, 1000);
        service.apply_report(first.clone()).unwrap();
        let mut reset = first;
        reset.uplink_total = 10;
        reset.downlink_total = 20;
        reset.collected_at += Duration::seconds(10);

        let result = service.apply_report(reset).unwrap();

        assert!(result.baseline_only);
        assert_eq!(result.billed_bytes, 0);
        assert!(!result.config_refresh_required);
    }

    #[test]
    fn billing_is_capped_by_remaining_pool() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let subscription = data.subscriptions.values_mut().next().unwrap();
            subscription.limit_bytes = 500;
            subscription.used_bytes = 450;
        });
        let service = TrafficService::new(store.clone());
        let first = seeded_report(&store, 0, 0);
        service.apply_report(first.clone()).unwrap();
        let mut second = first;
        second.uplink_total = 1000;
        second.collected_at += Duration::seconds(10);

        let result = service.apply_report(second).unwrap();

        assert_eq!(result.billed_bytes, 50);
        assert!(result.config_refresh_required);
    }
}
