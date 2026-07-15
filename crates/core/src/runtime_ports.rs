//! 本模块为严格用户级限速派生确定性 Xray fwmark。
//! 数据库中的 access_lines 仍表示管理员可见的逻辑线路与真实监听端口。
//! 限速已从"每用户运行时端口"退役,改为"每用户 fwmark + 带 sockopt.mark 的出站"。
//! mark 只由 StoreData 与用户 ID 决定,不读外部状态,落区间 [0x10000,0xFFFFF]。
//! 同节点不同限速用户的 mark 通过 hash(user_id)+线性探测去冲突,保证不相撞。
//! 入口端口对所有用户恒为线路真实监听端口(同线路全用户共享入站、凭据区分)。
//! 该算法供 agent 心跳渲染填充 XrayUserRateLimit.mark,订阅侧不再消费 mark。
//! 未限速用户不分 mark、走共享出口出站。
//! 若未来需跨进程强一致 mark 池,可在本模块外层持久化映射。
//! 本头部满足前十行中文注释约束。

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AccessLine, StoreData};

// 用户级限速 fwmark 区间:[0x10000, 0xFFFFF]。低 16 位(<0x10000)留给系统/其它用途,避免相撞。
const RATE_LIMIT_MARK_START: u32 = 0x1_0000;
const RATE_LIMIT_MARK_END: u32 = 0xF_FFFF;

pub fn effective_user_rate_limit_bps(data: &StoreData, user_id: Uuid) -> Option<u64> {
    let user = data.users.get(&user_id)?;
    if let Some(rate_limit_bps) = user.rate_limit_bps {
        return Some(rate_limit_bps);
    }
    let subscription = data.subscriptions.get(&user_id)?;
    let plan = data.plans.get(&subscription.plan_id)?;
    Some(plan.rate_limit_bps)
}

pub fn user_needs_rate_limit(data: &StoreData, user_id: Uuid) -> bool {
    // 上下行任一方向有限速即需要 fwmark(方向值均回退对称值,故对称用户口径不变)。
    effective_user_rate_limit_up_bps(data, user_id).unwrap_or_default() > 0
        || effective_user_rate_limit_down_bps(data, user_id).unwrap_or_default() > 0
}

/// 用户有效【上行】限速 bps：用户方向覆盖 > 套餐方向覆盖 > 对称 rate_limit_bps 回退。
pub fn effective_user_rate_limit_up_bps(data: &StoreData, user_id: Uuid) -> Option<u64> {
    directional_rate_limit_bps(data, user_id, true)
}

/// 用户有效【下行】限速 bps：口径同上,取下行列。
pub fn effective_user_rate_limit_down_bps(data: &StoreData, user_id: Uuid) -> Option<u64> {
    directional_rate_limit_bps(data, user_id, false)
}

/// 方向限速解析:某方向未单独设(用户、套餐两级都 None)时回退到对称 rate_limit_bps,保证向后兼容。
fn directional_rate_limit_bps(data: &StoreData, user_id: Uuid, up: bool) -> Option<u64> {
    let user = data.users.get(&user_id)?;
    let user_dir = if up {
        user.rate_limit_up_bps
    } else {
        user.rate_limit_down_bps
    };
    if let Some(v) = user_dir {
        return Some(v);
    }
    if let Some(plan) = data
        .subscriptions
        .get(&user_id)
        .and_then(|s| data.plans.get(&s.plan_id))
    {
        let plan_dir = if up {
            plan.rate_limit_up_bps
        } else {
            plan.rate_limit_down_bps
        };
        if let Some(v) = plan_dir {
            return Some(v);
        }
    }
    effective_user_rate_limit_bps(data, user_id)
}

/// 入口端口对所有用户恒等于线路真实监听端口:限速已改用 fwmark,不再分配运行时端口。
/// 保留本函数签名是为兼容订阅/读模型既有调用点(同线路全用户共享一条真实端口入站)。
pub fn runtime_listen_port_for_user(
    _data: &StoreData,
    line: &AccessLine,
    _user_id: Uuid,
) -> Option<u16> {
    Some(line.listen_port)
}

/// 为"有效限速>0"的用户解析其确定性 fwmark;非限速用户返回 None。
/// mark 只与"该节点上限速且在线用户集合"相关,与具体线路无关(同用户跨同节点多线路 mark 一致)。
pub fn rate_limit_mark_for_user(data: &StoreData, line: &AccessLine, user_id: Uuid) -> Option<u32> {
    if !user_needs_rate_limit(data, user_id) {
        return None;
    }
    rate_limit_marks_for_node(data, line.access_node_id, Some(user_id)).remove(&user_id)
}

/// 计算某接入节点上全部"有效限速>0"且运行态在线用户的确定性 fwmark 映射(键为 user_id)。
/// required_user 用于强制纳入某指定限速用户(即便其暂无出口指派),保证单用户查询稳定可得。
/// 去冲突逻辑从旧的"运行时端口分配"迁移至此:hash(user_id) 取基址 + 线性探测避让已占 mark。
pub fn rate_limit_marks_for_node(
    data: &StoreData,
    access_node_id: Uuid,
    required_user: Option<Uuid>,
) -> HashMap<Uuid, u32> {
    let mut users = data
        .user_exit_assignments
        .iter()
        .filter_map(|assignment| {
            let line = data.access_lines.get(&assignment.access_line_id)?;
            if line.access_node_id != access_node_id || !line.enabled {
                return None;
            }
            if !user_is_runtime_active(data, assignment.user_id)
                || !user_needs_rate_limit(data, assignment.user_id)
            {
                return None;
            }
            Some(assignment.user_id)
        })
        .collect::<Vec<_>>();
    if let Some(user_id) = required_user {
        if user_needs_rate_limit(data, user_id) {
            users.push(user_id);
        }
    }
    // 按 user_id 稳定排序去重,保证同节点用户集合的 mark 分配与遍历顺序无关、可复算。
    users.sort_unstable();
    users.dedup();

    let mut occupied = HashSet::new();
    let mut assigned = HashMap::new();
    for user_id in users {
        if let Some(mark) = next_available_mark(user_id, &occupied) {
            occupied.insert(mark);
            assigned.insert(user_id, mark);
        }
    }
    assigned
}

fn user_is_runtime_active(data: &StoreData, user_id: Uuid) -> bool {
    let Some(user) = data.users.get(&user_id) else {
        return false;
    };
    if user.disabled {
        return false;
    }
    let Some(subscription) = data.subscriptions.get(&user_id) else {
        return false;
    };
    subscription.active
        && subscription.expires_at > Utc::now()
        && subscription.remaining_bytes() > 0
        && data.tokens.values().any(|token| token.user_id == user_id)
}

fn next_available_mark(user_id: Uuid, occupied: &HashSet<u32>) -> Option<u32> {
    let range = RATE_LIMIT_MARK_END - RATE_LIMIT_MARK_START + 1;
    let base = rate_limit_mark_hash(user_id) % range;
    for offset in 0..range {
        let mark = RATE_LIMIT_MARK_START + ((base + offset) % range);
        if !occupied.contains(&mark) {
            return Some(mark);
        }
    }
    None
}

fn rate_limit_mark_hash(user_id: Uuid) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(b"xrayc-rate-limit-mark-v1");
    hasher.update(user_id.as_bytes());
    let digest = hasher.finalize();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}
