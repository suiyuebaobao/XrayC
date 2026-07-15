//! 本模块生成 access-agent 心跳响应读模型。
//! 函数只基于 StoreData 和调用参数组装 JSON。
//! 数据库写入仍由 PgStore::heartbeat_json 在 crate root 完成。
//! Xray 配置构建和哈希计算委托给 xray_render 模块。
//! 授权用户列表只展示 agent 启动所需的最小字段。
//! 探测任务列表仍由 PgStore 方法追加，不在本模块查询。
//! 公共 heartbeat_json 通过 crate root 重导出保持兼容。
//! 新增心跳响应字段应避免泄露出口凭据。
//! 本模块不得引入 SQL 或事务流程。
//! 文件行数保持低于 500 行，便于后续维护。

use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;
use xrayc_core::StoreData;

use crate::xray_render::{
    access_config_hash, build_access_config_with_probe_policy, empty_access_config_hash,
    healthy_supported_endpoint, user_has_active_subscription_token,
};

pub fn heartbeat_json(
    data: &StoreData,
    reported_node_id: Option<Uuid>,
    applied_config_hash: Option<&str>,
) -> serde_json::Value {
    heartbeat_json_with_probe_policy(data, reported_node_id, applied_config_hash, false)
}

pub fn heartbeat_json_with_probe_policy(
    data: &StoreData,
    reported_node_id: Option<Uuid>,
    applied_config_hash: Option<&str>,
    block_unhealthy_lines: bool,
) -> serde_json::Value {
    let node = reported_node_id.and_then(|id| data.access_nodes.get(&id));
    let access_config = build_access_config_with_probe_policy(
        data,
        node.map(|node| node.id),
        block_unhealthy_lines,
    );
    let desired_config_hash = node.map(|node| {
        access_config
            .as_ref()
            .map(access_config_hash)
            .unwrap_or_else(|| empty_access_config_hash(node.id))
    });
    let effective_applied = applied_config_hash.or_else(|| {
        // agent 未携带本地已应用版本时，必须按“未知状态”处理。
        // 否则重装或迁移到新服务器后会错误复用数据库旧 applied hash，
        // 导致新 agent 永远拿不到现有配置。
        if reported_node_id.is_none() {
            node.and_then(|node| node.applied_config_hash.as_deref())
        } else {
            None
        }
    });
    let config_hash = desired_config_hash.clone();
    let required = node
        .map(|node| {
            desired_config_hash
                .as_deref()
                .is_some_and(|hash| Some(hash) != effective_applied)
                || node.config_dirty
        })
        .unwrap_or(false);
    let authorized_users =
        authorized_users_for_node(data, node.map(|node| node.id), block_unhealthy_lines);
    let config = if required {
        access_config
            .and_then(|config| serde_json::to_value(config).ok())
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

    json!({
        "success": true,
        "accepted": true,
        "config_hash": config_hash,
        "desired_config_version": config_hash,
        "config_status": {
            "required": required,
            "reason": if required { "config_dirty" } else { "up_to_date" }
        },
        "authorized_users": authorized_users,
        "config": config
    })
}

fn authorized_users_for_node(
    data: &StoreData,
    node_id: Option<Uuid>,
    block_unhealthy_lines: bool,
) -> Vec<Value> {
    let Some(node_id) = node_id else {
        return Vec::new();
    };
    let now = Utc::now();
    let mut seen = HashSet::new();
    let mut users = data
        .user_exit_assignments
        .iter()
        .filter_map(|assignment| {
            let line = data.access_lines.get(&assignment.access_line_id)?;
            if line.access_node_id != node_id || !line.enabled {
                return None;
            }
            let user = data.users.get(&assignment.user_id)?;
            let subscription = data.subscriptions.get(&assignment.user_id)?;
            if user.disabled
                || !subscription.active
                || subscription.expires_at <= now
                || subscription.remaining_bytes() == 0
                || !user_has_active_subscription_token(data, user.id)
            {
                return None;
            }
            healthy_supported_endpoint(
                data,
                node_id,
                assignment.exit_pool_id,
                assignment.exit_endpoint_id,
                block_unhealthy_lines,
            )?;
            seen.insert(user.id).then(|| {
                json!({
                    "user_id": user.id,
                    "xray_user_key": user.xray_user_key,
                    "uuid": user.access_credential
                })
            })
        })
        .collect::<Vec<_>>();
    users.sort_by(|left, right| {
        left["xray_user_key"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["xray_user_key"].as_str().unwrap_or_default())
    });
    users
}
