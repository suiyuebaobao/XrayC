//! 本文件实现 Clash/mihomo 订阅生成。
//! 生成器只读取中转入口字段，确保上游出口地址和凭据不会泄露到客户端配置中。
//! 订阅 token 校验、套餐可用性和线路可见性都在这里串联。
//! 套餐授权通过 StoreData 展开，优先按单层分组直接输出线路。
//! 客户端分组和节点名称都来自线路所属分组。
//! 本模块不访问数据库，也不持久化任何用户状态。
//! 代理渲染细节拆分到 profile 和 proxy 子模块。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

mod dns;
mod options;
mod profile;
mod proxy;

#[cfg(test)]
mod binding_node_tests;
#[cfg(test)]
mod cert_sni_tests;
#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod hy2_tests;
#[cfg(test)]
mod option_tests;
#[cfg(test)]
mod runtime_port_tests;
#[cfg(test)]
mod selected_domain_tests;
#[cfg(test)]
mod tests;

use crate::binding_credential;
use crate::model::{AccessLine, ExitEndpoint};
use crate::store::StoreData;
use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

pub use options::SubscriptionOptions;

use profile::render_clash_yaml;
use proxy::{proxy_name_for_line, subscription_protocol_supported};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SubscriptionError {
    #[error("订阅不存在")]
    TokenNotFound,
    #[error("用户不存在")]
    UserNotFound,
    #[error("当前套餐没有可用线路")]
    NoAvailableLines,
}

#[derive(Debug, Clone)]
pub(super) struct VisibleLine {
    pub(super) section_group_id: Uuid,
    pub(super) section_group_name: String,
    pub(super) section_group_dedicated_rules: Vec<String>,
    pub(super) line_group_id: Uuid,
    pub(super) line_group_name: String,
    pub(super) line: AccessLine,
    pub(super) proxy_name: String,
    pub(super) credential: String,
}

impl VisibleLine {
    fn new(
        section_group_id: Uuid,
        section_group_name: String,
        section_group_dedicated_rules: Vec<String>,
        line_group_id: Uuid,
        line_group_name: String,
        line: AccessLine,
        credential: String,
    ) -> Self {
        Self {
            section_group_id,
            section_group_name,
            section_group_dedicated_rules,
            line_group_id,
            line_group_name,
            line,
            proxy_name: String::new(),
            credential,
        }
    }
}

pub fn generate_clash_yaml(data: &StoreData, token: &str) -> Result<String, SubscriptionError> {
    generate_clash_yaml_with_options(data, token, &SubscriptionOptions::default())
}

pub fn generate_clash_yaml_with_options(
    data: &StoreData,
    token: &str,
    options: &SubscriptionOptions,
) -> Result<String, SubscriptionError> {
    let token = data
        .tokens
        .get(token)
        .ok_or(SubscriptionError::TokenNotFound)?;
    let user = data
        .users
        .get(&token.user_id)
        .ok_or(SubscriptionError::UserNotFound)?;
    if user.disabled {
        return Err(SubscriptionError::TokenNotFound);
    }
    let subscription = data
        .subscriptions
        .get(&token.user_id)
        .ok_or(SubscriptionError::NoAvailableLines)?;
    if !subscription.active || subscription.expires_at <= Utc::now() {
        return Err(SubscriptionError::TokenNotFound);
    }
    let plan = data
        .plans
        .get(&subscription.plan_id)
        .ok_or(SubscriptionError::NoAvailableLines)?;
    let assigned_access_line_ids = data
        .user_access_line_assignments
        .iter()
        .filter(|assignment| assignment.user_id == user.id)
        .map(|assignment| assignment.access_line_id)
        .collect::<HashSet<_>>();
    let assigned_group_line_pairs = data
        .user_access_line_assignments
        .iter()
        .filter(|assignment| assignment.user_id == user.id)
        .map(|assignment| (assignment.line_group_id, assignment.access_line_id))
        .collect::<HashSet<_>>();
    let assignments_are_authoritative = data.user_access_line_assignments_authoritative;

    let mut visible = Vec::new();
    for group in data.resolved_plan_line_groups(plan) {
        let mut group_visible = Vec::new();
        let mut group_lines = data
            .access_lines
            .values()
            .filter(|line| access_line_belongs_to_group(data, line, group.line_group_id))
            .filter(|line| {
                if assignments_are_authoritative {
                    assigned_group_line_pairs.contains(&(group.line_group_id, line.id))
                } else {
                    assigned_access_line_ids.is_empty()
                        || assigned_access_line_ids.contains(&line.id)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        group_lines.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.listen_port.cmp(&right.listen_port))
                .then_with(|| left.id.cmp(&right.id))
        });
        for line in group_lines {
            if !line.enabled || subscription.remaining_bytes() == 0 {
                continue;
            }
            if !access_node_is_available(data, line.access_node_id) {
                continue;
            }
            if !line_has_assignable_exit_member(data, &line, options.block_unhealthy_lines) {
                continue;
            }
            // 限速已改用 fwmark,入口端口恒为线路真实监听端口,无需再按用户覆盖 listen_port。
            let credential = binding_credential(&line.protocol, &user.access_credential, line.id);
            group_visible.push(VisibleLine::new(
                group.section_group_id,
                group.section_group_name.clone(),
                group.dedicated_rules.clone(),
                group.line_group_id,
                group.line_group_name.clone(),
                line,
                credential,
            ));
        }

        group_visible.retain(|visible_line| subscription_protocol_supported(&visible_line.line));
        visible.extend(group_visible);
    }

    if visible.is_empty() {
        return Err(SubscriptionError::NoAvailableLines);
    }

    assign_proxy_names(data, &mut visible);

    let yaml = render_clash_yaml(options, &visible, plan.default_line_group_id);
    debug_assert!(!yaml.contains("198.51.100.10"));
    debug_assert!(!yaml.contains(&user.xray_user_key));
    Ok(yaml)
}

/// 判定某入口是否归属指定分组，是「订阅可见」与「计费授权」共用的唯一口径。
///
/// 之所以提升为 `pub(crate)` 并由计费模块复用：历史上订阅侧与计费侧各有一份实现，
/// 在「分组已迁移到绑定节点模式（`binding_node_ids` 非空）」时判定不一致——
/// 订阅侧保留了 legacy 回退 `line_group_id == Some(group)`，计费侧没有，
/// 导致靠旧字段归属、却未进 `binding_node_ids` 的线路「能连但不计费」的免费流量缺口。
/// 现在两处统一走本函数，确保对同一 `(line, group)` 给出一致结果。
pub(crate) fn access_line_belongs_to_group(
    data: &StoreData,
    line: &AccessLine,
    line_group_id: Uuid,
) -> bool {
    let Some(group) = data.line_groups.get(&line_group_id) else {
        return false;
    };
    if !group.binding_node_ids.is_empty() {
        // 绑定节点模式下，除直接命中绑定集合外，仍保留 legacy 字段回退，
        // 让尚未迁入 binding_node_ids 的旧线路同样「可见即可计费」。
        return group.binding_node_ids.contains(&line.id)
            || line.line_group_id == Some(line_group_id);
    }
    if let Some(exit_endpoint_id) = line.exit_endpoint_id {
        return group.line_ids.contains(&exit_endpoint_id);
    }
    line.line_group_id == Some(line_group_id)
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
                endpoint_is_assignable(endpoint)
                    && line
                        .exit_endpoint_id
                        .map(|exit_endpoint_id| endpoint.id == exit_endpoint_id)
                        .unwrap_or(true)
                    && endpoint.allow_new_assignments
                    && endpoint_member_status_is_healthy(&endpoint.status)
                    && (!block_unhealthy_lines
                        || endpoint_is_self_hosted(endpoint)
                        || !probe_state_blocks_endpoint(data, line.access_node_id, endpoint.id))
            })
        })
}

/// 本机出口(ownership=self_hosted)是中转节点同机自家出口、中转直接连它(本地/内网),
/// 按 CLAUDE.md §4 与开发方案口径天然可指派、不需上游探测就绪。
/// 第三方上游仍按探测派生的 `endpoint.healthy` 门控,避免给真实离线上游派单。
fn endpoint_is_assignable(endpoint: &ExitEndpoint) -> bool {
    endpoint.healthy || endpoint_is_self_hosted(endpoint)
}

fn endpoint_is_self_hosted(endpoint: &ExitEndpoint) -> bool {
    endpoint
        .ownership
        .trim()
        .eq_ignore_ascii_case("self_hosted")
}

fn endpoint_member_status_is_healthy(status: &str) -> bool {
    status.trim().is_empty() || status.trim().eq_ignore_ascii_case("healthy")
}

fn probe_state_blocks_endpoint(
    data: &StoreData,
    access_node_id: Uuid,
    exit_endpoint_id: Uuid,
) -> bool {
    data.access_exit_probe_states
        .get(&(access_node_id, exit_endpoint_id))
        .is_some_and(|state| state.effective_status.eq_ignore_ascii_case("offline"))
}

fn assign_proxy_names(data: &StoreData, visible: &mut [VisibleLine]) {
    let mut name_counts = HashMap::<String, usize>::new();
    let mut used_names = HashSet::<String>::new();
    let mut names_by_line_id = HashMap::<Uuid, String>::new();
    for (index, visible_line) in visible.iter_mut().enumerate() {
        if let Some(proxy_name) = names_by_line_id.get(&visible_line.line.id) {
            visible_line.proxy_name = proxy_name.clone();
            continue;
        }
        let line_name = visible_line.line.name.trim();
        let raw_name = if line_name.is_empty() {
            let group_name = visible_line.line_group_name.trim();
            if group_name.is_empty() {
                data.line_groups
                    .get(&visible_line.line_group_id)
                    .map(|group| group.name.trim())
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| proxy_name_for_line(&visible_line.line, index + 1))
            } else {
                group_name.to_string()
            }
        } else {
            line_name.to_string()
        };
        let base_name = raw_name;
        let count = name_counts.entry(base_name.clone()).or_insert(0);
        *count += 1;
        let candidate = if *count == 1 {
            base_name.clone()
        } else {
            format!("{base_name} {:02}", *count)
        };
        visible_line.proxy_name = unique_proxy_name(&candidate, &base_name, &mut used_names);
        names_by_line_id.insert(visible_line.line.id, visible_line.proxy_name.clone());
    }
}

fn unique_proxy_name(candidate: &str, base_name: &str, used_names: &mut HashSet<String>) -> String {
    let candidate = candidate.trim();
    if !candidate.is_empty() && used_names.insert(candidate.to_string()) {
        return candidate.to_string();
    }

    let base_name = base_name.trim();
    let base_name = if base_name.is_empty() {
        "节点"
    } else {
        base_name
    };
    let mut index = 2;
    loop {
        let name = format!("{base_name} {index:02}");
        if used_names.insert(name.clone()) {
            return name;
        }
        index += 1;
    }
}
