//! 本模块负责把控制面下发的整体配置按 runtime core 拆分。
//! 当前节点只跑 xray 单内核，这里按线路 runtime_core 过滤出 xray 承载的
//! 接入线路、路由规则、出口端点、限速规则，剔除非 xray 的历史脏数据。
//! 路由过滤按完整 inbound_tag（access-{line_id}）精确匹配，杜绝子串误命中。
//! 本文件只做纯数据变换，不访问网络/磁盘，便于单元测试覆盖。
//! 由 apply 模块在渲染前调用，拆分结果再各自编译写入。
//! 拆分逻辑独立成文件以控制 apply 主流程文件长度。
//! 修改字段映射时需同步 xray-config 编译器与 db 渲染口径。
//! 本头部满足前十行中文注释约束。

use std::collections::HashSet;

use xrayc_xray_config::AccessConfig;

use crate::config::RuntimeCore;

pub(super) fn split_config_for_runtime(
    config: &AccessConfig,
    runtime_core: RuntimeCore,
) -> AccessConfig {
    let expected_core = runtime_core.as_str();
    let access_lines = config
        .access_lines
        .iter()
        .filter(|line| runtime_core_matches(&line.runtime_core, expected_core))
        .cloned()
        .collect::<Vec<_>>();
    let mut retained_line_ids = HashSet::new();
    let mut retained_users = HashSet::new();
    let mut outbound_tags = HashSet::new();
    for line in &access_lines {
        retained_line_ids.insert(line.id.clone());
        if !line.source_line_id.trim().is_empty() {
            retained_line_ids.insert(line.source_line_id.clone());
        }
        outbound_tags.insert(line.default_exit_tag.clone());
        for user in &line.users {
            retained_users.insert(user.xray_user_key.clone());
        }
    }
    let routing_rules = config
        .routing_rules
        .iter()
        .filter(|rule| {
            rule.inbound_tag
                .as_deref()
                .map(|tag| {
                    // 路由规则的 inbound_tag 由 access-{line_id} 精确生成（见
                    // db::xray_protocol::access_inbound_tag_for_id）。这里必须按完整 tag
                    // 精确比对，禁止用 contains 子串匹配：否则 line-1 会误命中 line-10
                    // 的 access-line-10 tag，把别的线路规则混进本内核配置，造成串线。
                    retained_line_ids
                        .iter()
                        .any(|line_id| routing_tag_matches_line(tag, line_id))
                })
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    for rule in &routing_rules {
        outbound_tags.insert(rule.outbound_tag.clone());
    }
    let exit_endpoints = config
        .exit_endpoints
        .iter()
        .filter(|endpoint| outbound_tags.contains(&endpoint.tag))
        .cloned()
        .collect::<Vec<_>>();
    let rate_limits = config
        .rate_limits
        .iter()
        .filter(|limit| retained_users.contains(&limit.xray_user_key))
        .cloned()
        .collect::<Vec<_>>();
    // 本机出口服务只由 xray 承载，当前唯一内核即 xray，直接保留。
    let local_exit_services = match runtime_core {
        RuntimeCore::Xray => config.local_exit_services.clone(),
    };

    AccessConfig {
        node_id: config.node_id.clone(),
        log_level: config.log_level,
        stats_enabled: config.stats_enabled,
        access_lines,
        local_exit_services,
        exit_endpoints,
        routing_rules,
        rate_limits,
    }
}

/// 判断路由规则的 inbound_tag 是否精确对应给定线路。
/// inbound_tag 形态固定为 `access-{line_id}`（带 access- 边界前缀），
/// 按完整字符串相等比较，避免 line-1 子串命中 line-10 的标签导致串线。
fn routing_tag_matches_line(tag: &str, line_id: &str) -> bool {
    tag == format!("access-{line_id}")
}

fn runtime_core_matches(value: &str, expected_core: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.is_empty() {
        expected_core == "xray"
    } else {
        normalized == expected_core
    }
}
