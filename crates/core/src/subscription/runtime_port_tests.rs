//! 本模块覆盖严格用户级限速的确定性 fwmark 派生与端口口径。
//! 管理员仍只维护一条逻辑中转入口、守真实监听端口。
//! 限速已从"每用户运行时端口"退役,改为"每用户 Xray fwmark"。
//! 同线路全部用户(限速/不限速)共享同一入站、凭据区分。
//! 限速用户的入口端口必须仍等于线路真实监听端口。
//! 同节点不同限速用户的 mark 必须互不相撞且落区间 [0x10000,0xFFFFF]。
//! mark 由 hash(user_id)+按节点去冲突确定性派生,不读外部状态。
//! 测试只使用内存 Store,不访问数据库或网络。
//! 订阅不得暴露出口资源信息。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::{
    effective_user_rate_limit_down_bps, effective_user_rate_limit_up_bps, rate_limit_mark_for_user,
    rate_limit_marks_for_node, runtime_listen_port_for_user, AccessLine, MemoryStore,
};
use chrono::Utc;

// 用户级限速 fwmark 区间下界:与生产实现保持一致,测试仅断言"落区间"不固定具体哈希值。
const RATE_LIMIT_MARK_START: u32 = 0x1_0000;
const RATE_LIMIT_MARK_END: u32 = 0xF_FFFF;

#[test]
fn directional_rate_limit_resolves_user_over_plan_over_symmetric() {
    let store = MemoryStore::seeded();
    let user_id = store.read(|data| data.tokens["demo-token"].user_id);
    store.write(|data| {
        let plan_id = data.subscriptions[&user_id].plan_id;
        let plan = data.plans.get_mut(&plan_id).unwrap();
        plan.rate_limit_bps = 10_000_000; // 对称默认 10mbit
        plan.rate_limit_up_bps = Some(3_000_000); // 套餐上行覆盖 3mbit
        plan.rate_limit_down_bps = None;
        let user = data.users.get_mut(&user_id).unwrap();
        user.rate_limit_bps = None;
        user.rate_limit_up_bps = None; // 上行回退到套餐覆盖 3mbit
        user.rate_limit_down_bps = Some(8_000_000); // 用户下行覆盖 8mbit,优先于套餐
    });
    let up = store.read(|data| effective_user_rate_limit_up_bps(data, user_id));
    let down = store.read(|data| effective_user_rate_limit_down_bps(data, user_id));
    assert_eq!(up, Some(3_000_000), "上行应取套餐方向覆盖");
    assert_eq!(down, Some(8_000_000), "下行应取用户方向覆盖(优先于套餐)");
}

#[test]
fn directional_rate_limit_falls_back_to_symmetric_when_no_override() {
    let store = MemoryStore::seeded();
    let user_id = store.read(|data| data.tokens["demo-token"].user_id);
    store.write(|data| {
        let plan_id = data.subscriptions[&user_id].plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 6_000_000;
    });
    // 上下行都没设方向覆盖 → 回退对称 6mbit(向后兼容)。
    let up = store.read(|data| effective_user_rate_limit_up_bps(data, user_id));
    let down = store.read(|data| effective_user_rate_limit_down_bps(data, user_id));
    assert_eq!(up, Some(6_000_000), "无方向覆盖上行回退对称值");
    assert_eq!(down, Some(6_000_000), "无方向覆盖下行回退对称值");
}

/// 在同一逻辑线路上播两个限速且运行态在线的用户,返回 (节点 id, 线路, 第一用户, 第二用户)。
fn seed_two_limited_users(store: &MemoryStore) -> (uuid::Uuid, AccessLine, uuid::Uuid, uuid::Uuid) {
    store.write(|data| {
        let line = data.access_lines.values().next().unwrap().clone();
        let endpoint = data
            .exit_pools
            .get(&line.exit_pool_id)
            .unwrap()
            .members
            .first()
            .unwrap()
            .clone();
        let plan_id = data.subscriptions.values().next().unwrap().plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;

        let first_user_id = data.tokens["demo-token"].user_id;
        let second_user_id = uuid::Uuid::new_v4();
        data.users.insert(
            second_user_id,
            crate::User {
                id: second_user_id,
                email: "second@example.test".to_string(),
                xray_user_key: format!("u-{}@xrayc.local", second_user_id.simple()),
                access_credential: uuid::Uuid::new_v4().to_string(),
                disabled: false,
                rate_limit_bps: None,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        );
        data.subscriptions.insert(
            second_user_id,
            crate::UserSubscription {
                user_id: second_user_id,
                plan_id,
                active: true,
                expires_at: Utc::now() + chrono::Duration::days(30),
                used_bytes: 0,
                limit_bytes: 10 * 1024 * 1024 * 1024,
            },
        );
        data.tokens.insert(
            "fixture-visible-subscription".to_string(),
            crate::SubscriptionToken {
                token: String::new(),
                user_id: second_user_id,
            },
        );
        for user_id in [first_user_id, second_user_id] {
            data.user_exit_assignments.push(crate::UserExitAssignment {
                user_id,
                access_line_id: line.id,
                exit_pool_id: line.exit_pool_id,
                exit_endpoint_id: endpoint.id,
                assigned_at: Utc::now(),
                failover_reason: "unit-test".to_string(),
            });
        }
        (line.access_node_id, line, first_user_id, second_user_id)
    })
}

#[test]
fn runtime_listen_port_for_user_is_always_real_listen_port_even_when_limited() {
    let store = MemoryStore::seeded();
    let (_node_id, line, first_user_id, second_user_id) = seed_two_limited_users(&store);
    // 限速用户的入口端口必须恒等于线路真实监听端口(不再按用户拆运行时端口)。
    for user_id in [first_user_id, second_user_id] {
        let port = store.read(|data| runtime_listen_port_for_user(data, &line, user_id));
        assert_eq!(
            port,
            Some(line.listen_port),
            "限速用户入口端口必须仍是线路真实监听端口"
        );
    }
}

#[test]
fn rate_limit_mark_is_deterministic_and_falls_in_range() {
    let store = MemoryStore::seeded();
    let (node_id, line, first_user_id, _second_user_id) = seed_two_limited_users(&store);
    let mark_a = store.read(|data| rate_limit_mark_for_user(data, &line, first_user_id));
    let mark_b = store.read(|data| rate_limit_mark_for_user(data, &line, first_user_id));
    let mark = mark_a.expect("限速用户必须分到 fwmark");
    assert_eq!(mark_a, mark_b, "同一用户的 mark 必须确定性可复算");
    assert!(
        (RATE_LIMIT_MARK_START..=RATE_LIMIT_MARK_END).contains(&mark),
        "mark 必须落区间 [0x10000,0xFFFFF],实际 {mark:#x}"
    );
    // 节点级映射与单用户查询必须一致。
    let node_mark = store.read(|data| {
        rate_limit_marks_for_node(data, node_id, None)
            .get(&first_user_id)
            .copied()
    });
    assert_eq!(node_mark, Some(mark), "节点级 mark 映射应与单用户查询一致");
}

#[test]
fn distinct_limited_users_on_same_node_get_distinct_marks() {
    let store = MemoryStore::seeded();
    let (node_id, _line, first_user_id, second_user_id) = seed_two_limited_users(&store);
    let marks = store.read(|data| rate_limit_marks_for_node(data, node_id, None));
    let first = *marks.get(&first_user_id).expect("第一用户应有 mark");
    let second = *marks.get(&second_user_id).expect("第二用户应有 mark");
    assert_ne!(first, second, "同节点不同限速用户的 mark 不得相撞");
    for mark in [first, second] {
        assert!(
            (RATE_LIMIT_MARK_START..=RATE_LIMIT_MARK_END).contains(&mark),
            "mark 必须落区间 [0x10000,0xFFFFF],实际 {mark:#x}"
        );
    }
}

#[test]
fn unlimited_user_gets_no_rate_limit_mark() {
    let store = MemoryStore::seeded();
    // 默认套餐/用户均无限速 → 不应派 mark。
    let (line, user_id) = store.read(|data| {
        let line = data.access_lines.values().next().unwrap().clone();
        (line, data.tokens["demo-token"].user_id)
    });
    let mark = store.read(|data| rate_limit_mark_for_user(data, &line, user_id));
    assert_eq!(mark, None, "不限速用户不应分到 fwmark");
}

#[test]
fn limited_users_on_same_line_share_real_listen_port() {
    let store = MemoryStore::seeded();
    let base_port = store.read(|data| data.access_lines.values().next().unwrap().listen_port);
    let (_node_id, _line, _first, _second) = seed_two_limited_users(&store);

    let first_yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("first subscription should render");
    let second_yaml = store
        .read(|data| generate_clash_yaml(data, "fixture-visible-subscription"))
        .expect("second subscription should render");
    let first_profile: serde_yaml::Value = serde_yaml::from_str(&first_yaml).unwrap();
    let second_profile: serde_yaml::Value = serde_yaml::from_str(&second_yaml).unwrap();
    let first_port = first_profile["proxies"][0]["port"].as_i64().unwrap();
    let second_port = second_profile["proxies"][0]["port"].as_i64().unwrap();

    // 限速用户订阅端口必须等于线路真实监听端口(两用户同端口、共享入站)。
    assert_eq!(first_port, i64::from(base_port));
    assert_eq!(second_port, i64::from(base_port));
    assert_eq!(first_profile["proxies"].as_sequence().unwrap().len(), 1);
    assert_eq!(second_profile["proxies"].as_sequence().unwrap().len(), 1);
}
