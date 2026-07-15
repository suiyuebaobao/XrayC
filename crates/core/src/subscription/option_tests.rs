//! 本测试模块覆盖订阅选项的规则重写行为。
//! 它从主订阅测试文件拆出，避免单文件超过五百行。
//! 这些用例只验证 SubscriptionOptions 与默认代理组的兼容逻辑。
//! 旧版 PROXY 目标必须被重写到实际可见的分组。
//! default_rules 应优先于遗留 rules 字段，避免后台配置歧义。
//! 测试使用 MemoryStore，不访问数据库、网络或真实服务器。
//! 新增规则相关用例优先放在这里，保持关注点集中。
//! 文件内断言使用最终 V2 分组显示名。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;

#[test]
fn subscription_options_rewrites_legacy_proxy_targets_to_first_output_group() {
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "auto_test_enabled": false,
        "default_rules": [
            "DOMAIN-SUFFIX,example.test,PROXY",
            "IP-CIDR,192.0.2.1/32,PROXY,no-resolve",
            "MATCH,PROXY"
        ]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render with rewritten legacy proxy targets");

    assert!(yaml.contains("DOMAIN-SUFFIX,example.test,默认分组"));
    assert!(yaml.contains("IP-CIDR,192.0.2.1/32,默认分组,no-resolve"));
    assert!(yaml.contains("MATCH,默认分组"));
    assert!(!yaml.contains(",PROXY"));
}

#[test]
fn subscription_options_prefers_explicit_default_rules_over_legacy_rules() {
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "rules": ["MATCH,PROXY"],
        "default_rules": ["DOMAIN-SUFFIX,example.com,默认分组", "MATCH,默认分组"]
    }));

    assert_eq!(
        options.rules,
        vec![
            "DOMAIN-SUFFIX,example.com,默认分组".to_string(),
            "MATCH,默认分组".to_string()
        ]
    );
}

#[test]
fn subscription_options_default_keeps_unhealthy_lines_visible() {
    let default_options = SubscriptionOptions::from_json(&serde_json::json!({}));
    let strict_options = SubscriptionOptions::from_json(&serde_json::json!({
        "block_unhealthy_lines": true
    }));

    assert!(!default_options.block_unhealthy_lines);
    assert!(strict_options.block_unhealthy_lines);
}

#[test]
fn subscription_options_keeps_auto_test_group_name_unique() {
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "auto_test_enabled": true,
        "auto_test_name": "默认分组"
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let group_names = profile["proxy-groups"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(|group| group["name"].as_str())
        .collect::<Vec<_>>();
    let unique_names = group_names
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(group_names.len(), unique_names.len());
    assert!(group_names.contains(&"默认分组 分组"));
}

#[test]
fn subscription_options_customize_groups_and_rules() {
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "mixed_port": 7891,
        "allow_lan": true,
        "mode": "global",
        "log_level": "warning",
        "default_rules": ["DOMAIN-SUFFIX,example.com,默认分组"],
        "auto_test_enabled": true,
        "auto_test_name": "自动测速",
        "auto_test_url": "http://cp.cloudflare.com/generate_204",
        "auto_test_interval_seconds": 600
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render with custom options");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(profile["mixed-port"], 7891);
    assert_eq!(profile["allow-lan"], true);
    assert_eq!(profile["mode"], "global");
    assert_eq!(profile["log-level"], "warning");
    let proxy_groups = profile["proxy-groups"].as_sequence().unwrap();
    let auto_group = proxy_groups
        .iter()
        .find(|group| group["name"] == "自动测速")
        .expect("auto group should exist");
    let first_proxy = &profile["proxies"].as_sequence().unwrap()[0];
    let default_group = proxy_groups
        .iter()
        .find(|group| group["name"] == "默认分组")
        .expect("flat group should become subscription group");
    assert!(yaml.contains("香港 01"));
    assert!(yaml.contains("自动测速"));
    assert!(auto_group["icon"].is_null());
    assert!(default_group["icon"].is_null());
    assert!(first_proxy["icon"].is_null());
    assert!(yaml.contains("DOMAIN-SUFFIX,example.com,默认分组"));
    assert!(yaml.contains("MATCH,默认分组"));
}

#[test]
fn subscription_options_rejects_client_direct_mode() {
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "mode": "direct"
    }));

    assert_eq!(options.mode, "rule");
}

#[test]
fn subscription_options_rewrites_client_direct_rules_to_access_group() {
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "default_rules": ["DOMAIN-SUFFIX,example.test,DIRECT", "MATCH,DIRECT"]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render");

    assert!(yaml.contains("DOMAIN-SUFFIX,example.test,DIRECT"));
    assert!(yaml.contains("MATCH,默认分组"));
    assert!(!yaml.contains("MATCH,DIRECT"));
}

#[test]
fn subscription_options_default_hides_auto_test_group() {
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({}));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render with rewritten match rule");

    assert!(!yaml.contains("name: 自动选择"));
    assert!(!yaml.contains("type: url-test"));
    assert!(yaml.contains("MATCH,默认分组"));
    assert!(!yaml.contains("MATCH,PROXY"));
    for rule in [
        "DOMAIN-SUFFIX,chatgpt.com,默认分组",
        "DOMAIN-SUFFIX,claude.ai,默认分组",
        "DOMAIN-SUFFIX,grok.com,默认分组",
        "DOMAIN-SUFFIX,aistudio.google.com,默认分组",
    ] {
        assert!(yaml.contains(rule));
    }
    assert!(yaml.contains("IP-CIDR,192.168.0.0/16,DIRECT,no-resolve"));
    assert!(yaml.contains("DOMAIN-SUFFIX,cn,DIRECT"));
    assert!(yaml.contains("GEOSITE,CN,DIRECT"));
    assert!(yaml.contains("GEOIP,CN,DIRECT,no-resolve"));
    assert!(yaml.find("DOMAIN-SUFFIX,chatgpt.com,默认分组") < yaml.find("DOMAIN-SUFFIX,cn,DIRECT"));
}

#[test]
fn subscription_merges_group_dedicated_rules_before_common_rules() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        data.line_groups.get_mut(&group_id).unwrap().dedicated_rules =
            vec!["DOMAIN-SUFFIX,openai.com".to_string()];
    });
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "default_rules": [
            "DOMAIN-SUFFIX,cn,DIRECT",
            "MATCH,PROXY"
        ]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render with group dedicated rules");

    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,默认分组"));
    assert!(yaml.find("DOMAIN-SUFFIX,openai.com,默认分组") < yaml.find("DOMAIN-SUFFIX,cn,DIRECT"));
    assert!(yaml.contains("MATCH,默认分组"));
}

#[test]
fn subscription_merges_bound_rule_sets_for_each_visible_group() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = data.access_lines.keys().next().copied().unwrap();
        let second_group_id = uuid::Uuid::new_v4();
        let second_line_id = uuid::Uuid::new_v4();
        let second_endpoint_id = uuid::Uuid::new_v4();
        let second_pool_id = uuid::Uuid::new_v4();
        let shared_rule_set_id = uuid::Uuid::new_v4();

        data.line_groups.get_mut(&first_group_id).unwrap().name = "AI分组".to_string();
        data.line_groups
            .get_mut(&first_group_id)
            .unwrap()
            .rule_set_bindings = vec![crate::LineGroupRuleSetBinding {
            rule_set_id: shared_rule_set_id,
            rule_set_name: "AI规则".to_string(),
            enabled: true,
            position: 100,
            rules: vec!["DOMAIN-SUFFIX,openai.com,PROXY".to_string()],
        }];

        let mut second_line = data.access_lines[&first_line_id].clone();
        second_line.id = second_line_id;
        second_line.name = "游戏线路".to_string();
        second_line.exit_endpoint_id = Some(second_endpoint_id);
        second_line.exit_pool_id = second_pool_id;
        second_line.listen_port = 9443;
        data.access_lines.insert(second_line_id, second_line);

        let mut second_pool = data.exit_pools.values().next().unwrap().clone();
        second_pool.id = second_pool_id;
        second_pool.name = "游戏出口池".to_string();
        second_pool.members[0].id = second_endpoint_id;
        data.exit_pools.insert(second_pool_id, second_pool);

        data.line_groups.insert(
            second_group_id,
            crate::LineGroup {
                id: second_group_id,
                name: "游戏分组".to_string(),
                country_code: "US".to_string(),
                icon: String::new(),
                sort_weight: 200,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: Some(second_pool_id),
                line_ids: vec![second_endpoint_id],
                binding_node_ids: Vec::new(),
                dedicated_rules: Vec::new(),
                rule_set_bindings: vec![crate::LineGroupRuleSetBinding {
                    rule_set_id: shared_rule_set_id,
                    rule_set_name: "AI规则".to_string(),
                    enabled: true,
                    position: 100,
                    rules: vec!["DOMAIN-SUFFIX,openai.com,PROXY".to_string()],
                }],
            },
        );

        let plan = data.plans.values_mut().next().unwrap();
        plan.line_group_ids = vec![first_group_id, second_group_id];
        plan.line_groups = vec![
            crate::PlanLineGroup {
                line_group_id: first_group_id,
                billing_multiplier: 1.0,
            },
            crate::PlanLineGroup {
                line_group_id: second_group_id,
                billing_multiplier: 1.0,
            },
        ];
    });
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "default_rules": ["MATCH,PROXY"]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render bound rule sets");

    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,AI分组"));
    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,游戏分组"));
    assert!(!yaml.contains("DOMAIN-SUFFIX,openai.com,PROXY"));
}

#[test]
fn subscription_preserves_dedicated_direct_and_reject_actions() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        data.line_groups.get_mut(&group_id).unwrap().dedicated_rules = vec![
            "DOMAIN-SUFFIX,baidu.com,DIRECT".to_string(),
            "DOMAIN-SUFFIX,blocked.example,REJECT".to_string(),
            "IP-CIDR,203.0.113.0/24,DIRECT,no-resolve".to_string(),
            "DOMAIN-SUFFIX,openai.com,PROXY".to_string(),
        ];
    });
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "default_rules": ["MATCH,PROXY"]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render dedicated direct and reject rules");

    assert!(yaml.contains("DOMAIN-SUFFIX,baidu.com,DIRECT"));
    assert!(yaml.contains("DOMAIN-SUFFIX,blocked.example,REJECT"));
    assert!(yaml.contains("IP-CIDR,203.0.113.0/24,DIRECT,no-resolve"));
    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,默认分组"));
    assert!(!yaml.contains("DOMAIN-SUFFIX,baidu.com,默认分组"));
    assert!(!yaml.contains("DOMAIN-SUFFIX,blocked.example,默认分组"));
}

#[test]
fn subscription_match_uses_plan_default_group_when_visible() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = data.access_lines.keys().next().copied().unwrap();
        let second_group_id = uuid::Uuid::new_v4();
        let second_line_id = uuid::Uuid::new_v4();
        let second_endpoint_id = uuid::Uuid::new_v4();
        let second_pool_id = uuid::Uuid::new_v4();

        let mut second_line = data.access_lines[&first_line_id].clone();
        second_line.id = second_line_id;
        second_line.name = "视频线路".to_string();
        second_line.exit_endpoint_id = Some(second_endpoint_id);
        second_line.exit_pool_id = second_pool_id;
        second_line.listen_port = 8443;
        data.access_lines.insert(second_line_id, second_line);

        let mut second_pool = data.exit_pools.values().next().unwrap().clone();
        second_pool.id = second_pool_id;
        second_pool.name = "视频出口池".to_string();
        second_pool.members[0].id = second_endpoint_id;
        data.exit_pools.insert(second_pool_id, second_pool);

        data.line_groups.insert(
            second_group_id,
            crate::LineGroup {
                id: second_group_id,
                name: "视频分组".to_string(),
                country_code: "US".to_string(),
                icon: String::new(),
                sort_weight: 200,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: Some(second_pool_id),
                line_ids: vec![second_endpoint_id],
                binding_node_ids: Vec::new(),
                dedicated_rules: vec!["DOMAIN-SUFFIX,youtube.com".to_string()],
                rule_set_bindings: Vec::new(),
            },
        );

        let plan = data.plans.values_mut().next().unwrap();
        plan.line_group_ids = vec![first_group_id, second_group_id];
        plan.line_groups = vec![
            crate::PlanLineGroup {
                line_group_id: first_group_id,
                billing_multiplier: 1.0,
            },
            crate::PlanLineGroup {
                line_group_id: second_group_id,
                billing_multiplier: 1.0,
            },
        ];
        plan.default_line_group_id = Some(second_group_id);
    });
    let options = SubscriptionOptions::from_json(&serde_json::json!({
        "default_rules": ["DOMAIN-SUFFIX,cn,DIRECT"]
    }));

    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render with plan default group");

    assert!(yaml.contains("DOMAIN-SUFFIX,youtube.com,视频分组"));
    assert!(yaml.contains("MATCH,视频分组"));
    assert!(!yaml.contains("MATCH,默认分组"));
}

#[test]
fn subscription_emits_fake_ip_dns_for_anti_pollution() {
    // 真机踩到的 bug:订阅不带 dns 段时,手机 FLClash(TUN)会回落系统/运营商 DNS,
    // 被墙域名(Telegram 等)被 DNS 污染 → 连不上;桌面客户端有自己的 DNS 兜底故正常。
    // 修复:订阅内置 fake-ip + 抗污染 fallback 的 dns 段,被墙域名改由国外 DNS 解析。
    let store = MemoryStore::seeded();
    let options = SubscriptionOptions::from_json(&serde_json::json!({}));
    let yaml = store
        .read(|data| generate_clash_yaml_with_options(data, "demo-token", &options))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let dns = &profile["dns"];
    assert_eq!(dns["enable"], true, "dns.enable 应为 true");
    assert_eq!(dns["enhanced-mode"], "fake-ip", "应为 fake-ip 模式");
    assert!(
        dns["fallback"]
            .as_sequence()
            .map(|list| !list.is_empty())
            .unwrap_or(false),
        "应有境外 fallback DNS 做抗污染"
    );
    assert_eq!(dns["fallback-filter"]["geoip-code"], "CN");
    assert!(
        dns["fallback-filter"]["geosite"]
            .as_sequence()
            .map(|list| list.iter().any(|item| item == "gfw"))
            .unwrap_or(false),
        "fallback-filter 应含 gfw geosite,把被墙域名判给国外 DNS 解析"
    );
}
