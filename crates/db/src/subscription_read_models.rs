//! 本模块生成用户订阅相关的只读 JSON 视图。
//! 逻辑只消费 StoreData 内存快照，不访问数据库。
//! 中转入口、套餐余量和订阅 token 在这里组合。
//! 出口可分配性判断复用 Xray 渲染协议 helper。
//! 探测状态只用于过滤不可用出口，不改变状态。
//! 函数通过 crate root 重新导出，保持调用路径兼容。
//! 本模块不包含 SQL、PgStore 方法或事务边界。
//! 新增用户订阅展示字段应优先放在这里。
//! 文件保持小于 500 行，避免再次形成巨型模块。
//! 所有输出字段名维持历史 API 兼容。

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use xrayc_core::{
    runtime_listen_port_for_user, AccessLine, ExitEndpoint, StoreData, SubscriptionToken,
    UserSubscription,
};

use crate::helpers::bytes_to_gb;
use crate::xray_protocol::xray_exit_protocol_for_endpoint;
use crate::xray_render::probe_state_blocks_endpoint;

pub fn user_subscription_json(data: &StoreData) -> serde_json::Value {
    let Some(token) = data
        .tokens
        .get("demo-token")
        .or_else(|| data.tokens.values().next())
    else {
        return json!({});
    };
    user_subscription_json_for_token(data, token, false)
}

pub fn user_subscription_json_for_user(data: &StoreData, user_id: Uuid) -> serde_json::Value {
    user_subscription_json_for_user_with_probe_policy(data, user_id, false)
}

pub fn user_subscription_json_for_user_with_probe_policy(
    data: &StoreData,
    user_id: Uuid,
    block_unhealthy_lines: bool,
) -> serde_json::Value {
    let Some(token) = data.tokens.values().find(|token| token.user_id == user_id) else {
        return json!({});
    };
    user_subscription_json_for_token(data, token, block_unhealthy_lines)
}

fn user_subscription_json_for_token(
    data: &StoreData,
    token: &SubscriptionToken,
    block_unhealthy_lines: bool,
) -> serde_json::Value {
    let Some(user) = data.users.get(&token.user_id) else {
        return json!({});
    };
    if user.disabled {
        return json!({});
    }
    let Some(user_subscription) = data.subscriptions.get(&token.user_id) else {
        return json!({});
    };
    let plan_name = data
        .plans
        .get(&user_subscription.plan_id)
        .map(|plan| plan.name.clone())
        .unwrap_or_else(|| "当前套餐".to_string());
    let mut visible_lines = Vec::new();
    if let Some(plan) = data.plans.get(&user_subscription.plan_id) {
        for group in data.resolved_plan_line_groups(plan) {
            for line_id in
                subscription_access_line_ids_for_group(data, token.user_id, group.line_group_id)
            {
                let Some(line) = data.access_lines.get(&line_id) else {
                    continue;
                };
                if !subscription_access_line_available(
                    data,
                    line,
                    user_subscription,
                    block_unhealthy_lines,
                ) {
                    continue;
                }
                visible_lines.push((line_id, group.line_group_id));
            }
        }
    }
    let access_lines = visible_lines
        .into_iter()
        .filter_map(|(line_id, group_id)| {
            data.access_lines.get(&line_id).map(|line| (line, group_id))
        })
        .filter_map(|(line, group_id)| {
            runtime_listen_port_for_user(data, line, token.user_id)
                .map(|listen_port| (line, group_id, listen_port))
        })
        .enumerate()
        .map(|(index, (line, group_id, listen_port))| {
            let group = data.line_groups.get(&group_id);
            let raw_name = {
                let line_name = line.name.trim();
                if line_name.is_empty() {
                    "未命名节点"
                } else {
                    line_name
                }
            };
            let display_name = raw_name.to_string();
            let line_group_name = group
                .map(|group| group.name.clone())
                .unwrap_or_else(|| raw_name.to_string());
            json!({
                "id": index + 1,
                "name": display_name,
                "line_group_id": group_id,
                "line_group_name": line_group_name,
                "region_code": line.region_code,
                "region_name": line.region_name,
                "region_flag": line.region_flag,
                "listen_host": line.listen_host,
                "listen_port": listen_port,
                "protocol": format!("{}+{}", line.protocol, line.transport),
                "udp_enabled": line.udp_enabled
            })
        })
        .collect::<Vec<_>>();

    json!({
        "plan_name": plan_name,
        "expires_at": user_subscription.expires_at,
        "traffic": {
            "used_gb": bytes_to_gb(user_subscription.used_bytes),
            "total_gb": if user_subscription.limit_bytes == u64::MAX { -1.0 } else { bytes_to_gb(user_subscription.limit_bytes) }
        },
        "token": token.token,
        "subscription_url": format!("/sub/{}", token.token),
        "access_lines": access_lines
    })
}

fn subscription_access_line_ids_for_group(
    data: &StoreData,
    user_id: Uuid,
    line_group_id: Uuid,
) -> Vec<Uuid> {
    let mut line_ids = data
        .user_access_line_assignments
        .iter()
        .filter(|assignment| {
            assignment.user_id == user_id
                && assignment.line_group_id == line_group_id
                && data
                    .access_lines
                    .get(&assignment.access_line_id)
                    .is_some_and(|line| access_line_belongs_to_group(data, line, line_group_id))
        })
        .map(|assignment| assignment.access_line_id)
        .collect::<Vec<_>>();
    if line_ids.is_empty() && !data.user_access_line_assignments_authoritative {
        line_ids = data
            .access_lines
            .values()
            .filter(|line| access_line_belongs_to_group(data, line, line_group_id))
            .map(|line| line.id)
            .collect();
    }
    line_ids.sort_by(|left, right| {
        let left_line = data.access_lines.get(left);
        let right_line = data.access_lines.get(right);
        left_line
            .map(|line| (line.name.as_str(), line.listen_port))
            .cmp(&right_line.map(|line| (line.name.as_str(), line.listen_port)))
            .then_with(|| left.cmp(right))
    });
    line_ids.dedup();
    line_ids
}

fn access_line_belongs_to_group(data: &StoreData, line: &AccessLine, line_group_id: Uuid) -> bool {
    let Some(group) = data.line_groups.get(&line_group_id) else {
        return false;
    };
    if !group.binding_node_ids.is_empty() {
        return group.binding_node_ids.contains(&line.id)
            || line.line_group_id == Some(line_group_id);
    }
    if let Some(exit_endpoint_id) = line.exit_endpoint_id {
        return group.line_ids.contains(&exit_endpoint_id);
    }
    line.line_group_id == Some(line_group_id)
}

fn subscription_access_line_available(
    data: &StoreData,
    line: &AccessLine,
    subscription: &UserSubscription,
    block_unhealthy_lines: bool,
) -> bool {
    line.enabled
        && subscription.active
        && subscription.expires_at > Utc::now()
        && access_node_is_available(data, line.access_node_id)
        && subscription.remaining_bytes() > 0
        && line_has_assignable_exit_member(data, line, block_unhealthy_lines)
}

fn access_node_is_available(data: &StoreData, access_node_id: Uuid) -> bool {
    data.access_nodes.get(&access_node_id).is_some_and(|node| {
        !matches!(
            node.status.trim().to_ascii_lowercase().as_str(),
            "disabled" | "offline" | "unavailable"
        )
    })
}

fn line_has_assignable_exit_member(
    data: &StoreData,
    line: &AccessLine,
    block_unhealthy_lines: bool,
) -> bool {
    data.exit_pools
        .get(&line.exit_pool_id)
        .filter(|pool| pool.enabled)
        .is_some_and(|pool| {
            pool.members.iter().any(|endpoint| {
                endpoint.healthy
                    && line
                        .exit_endpoint_id
                        .map(|exit_endpoint_id| endpoint.id == exit_endpoint_id)
                        .unwrap_or(true)
                    && endpoint.allow_new_assignments
                    && endpoint_member_status_is_healthy(endpoint)
                    && (!block_unhealthy_lines
                        || !probe_state_blocks_endpoint(data, line.access_node_id, endpoint.id))
                    && xray_exit_protocol_for_endpoint(endpoint).is_some()
            })
        })
}

fn endpoint_member_status_is_healthy(endpoint: &ExitEndpoint) -> bool {
    endpoint.status.trim().is_empty() || endpoint.status.trim().eq_ignore_ascii_case("healthy")
}
