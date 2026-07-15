//! 本测试模块覆盖订阅生成的分组展开规则。
//! 当前主模型是一层分组，分组直接包含线路。
//! 客户端节点名来自线路，客户端分组名来自分组。
//! 测试使用 MemoryStore，避免依赖数据库和外部网络。
//! 这里不验证具体协议渲染，只验证扁平分组展开后的客户端分组形状。
//! 如果后续调整套餐授权模型，应优先更新本文件断言。
//! 文件拆分用于满足单文件不超过五百行的仓库规则。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;

fn first_line_id_for_group(data: &crate::StoreData, group_id: uuid::Uuid) -> uuid::Uuid {
    let group = data.line_groups.get(&group_id).unwrap();
    data.access_lines
        .values()
        .find(|line| {
            line.exit_endpoint_id
                .is_some_and(|endpoint_id| group.line_ids.contains(&endpoint_id))
                || line.line_group_id == Some(group_id)
        })
        .unwrap()
        .id
}

#[test]
fn subscription_uses_one_line_from_flat_group_members() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = first_line_id_for_group(data, group_id);
        let second_line_id = uuid::Uuid::new_v4();
        let mut second_line = data.access_lines[&first_line_id].clone();
        second_line.id = second_line_id;
        second_line.name = "香港 02".to_string();
        second_line.listen_port = 8443;
        data.access_lines.insert(second_line_id, second_line);

        let group = data.line_groups.get_mut(&group_id).unwrap();
        group.name = "GPT 分组".to_string();
        group.icon = "🤖".to_string();
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render flat group lines");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = profile["proxies"].as_sequence().unwrap();
    let group = profile["proxy-groups"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|group| group["type"] == "select")
        .unwrap();

    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0]["name"], "香港 01");
    assert!(proxies[0]["icon"].is_null());
    assert_eq!(group["name"], "GPT 分组");
    assert!(group["icon"].is_null());
    assert_eq!(group["proxies"].as_sequence().unwrap().len(), 2);
    assert_eq!(group["proxies"][0], "香港 01");
    assert!(yaml.contains("香港 02"));
    assert!(!yaml.contains("🤖"));
}

#[test]
fn subscription_uses_exit_endpoint_membership_for_line_bound_entries() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let line_id = data.access_lines.keys().next().copied().unwrap();
        let endpoint_id = data.access_lines[&line_id].exit_endpoint_id.unwrap();
        assert_eq!(data.access_lines[&line_id].line_group_id, None);
        assert!(data.line_groups[&group_id].line_ids.contains(&endpoint_id));
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("line endpoint membership should authorize subscription line");

    assert!(yaml.contains("香港 01"));
    assert!(!yaml.contains("🇭🇰 香港 01"));
}

#[test]
fn subscription_keeps_all_authorized_groups_when_groups_share_same_line() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let endpoint_id = data.line_groups[&first_group_id].line_ids[0];
        let second_group_id = uuid::Uuid::new_v4();
        data.line_groups.insert(
            second_group_id,
            crate::LineGroup {
                id: second_group_id,
                name: "GPT 分组".to_string(),
                country_code: "US".to_string(),
                icon: String::new(),
                sort_weight: 200,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: data.line_groups[&first_group_id].exit_pool_id,
                line_ids: vec![endpoint_id],
                binding_node_ids: Vec::new(),
                dedicated_rules: Vec::new(),
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
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render shared line in both groups");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = profile["proxies"].as_sequence().unwrap();
    let select_groups = profile["proxy-groups"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter(|group| group["type"] == "select")
        .collect::<Vec<_>>();

    assert_eq!(proxies.len(), 1);
    assert_eq!(select_groups.len(), 2);
    assert!(select_groups
        .iter()
        .any(|group| group["name"] == "默认分组"));
    assert!(select_groups
        .iter()
        .any(|group| group["name"] == "GPT 分组"));
    let expected_proxy_names = vec![proxies[0]["name"].clone()];
    assert!(select_groups
        .iter()
        .all(|group| group["proxies"].as_sequence().unwrap() == &expected_proxy_names));
}

#[test]
fn subscription_omits_icons_from_proxy_and_group_names() {
    let store = MemoryStore::seeded();
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let first_proxy = &profile["proxies"].as_sequence().unwrap()[0];
    let first_group = profile["proxy-groups"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|group| group["type"] == "select")
        .unwrap();

    assert_eq!(first_proxy["name"], "香港 01");
    assert!(first_proxy["icon"].is_null());
    assert_eq!(first_group["name"], "默认分组");
    assert!(first_group["icon"].is_null());
    assert_eq!(first_group["proxies"][0], "香港 01");
}

#[test]
fn subscription_does_not_fallback_to_default_icon() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let line_id = first_line_id_for_group(data, group_id);
        data.access_lines
            .get_mut(&line_id)
            .unwrap()
            .region_flag
            .clear();
        data.line_groups.get_mut(&group_id).unwrap().icon.clear();
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let first_proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert!(first_proxy["icon"].is_null());
}

#[test]
fn subscription_proxy_and_group_names_do_not_collide() {
    let store = MemoryStore::seeded();
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = profile["proxies"].as_sequence().unwrap();
    let groups = profile["proxy-groups"].as_sequence().unwrap();
    let proxy_names = proxies
        .iter()
        .filter_map(|proxy| proxy["name"].as_str())
        .collect::<std::collections::HashSet<_>>();

    for group in groups {
        let Some(group_name) = group["name"].as_str() else {
            continue;
        };
        assert!(
            !proxy_names.contains(group_name),
            "proxy group name must not equal a proxy name: {group_name}"
        );
    }
}

#[test]
fn subscription_proxy_names_stay_unique_when_suffix_names_exist() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = first_line_id_for_group(data, first_group_id);
        for name in ["香港 01", "香港 01"] {
            let line_id = uuid::Uuid::new_v4();
            let pool_id = uuid::Uuid::new_v4();
            let mut line = data.access_lines[&first_line_id].clone();
            line.id = line_id;
            line.name = name.to_string();
            line.exit_pool_id = pool_id;
            line.listen_port += data.access_lines.len() as u16 + 1;
            data.access_lines.insert(line_id, line);
            let mut pool = data.exit_pools.values().next().unwrap().clone();
            pool.id = pool_id;
            data.exit_pools.insert(pool_id, pool);
        }
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let names = profile["proxies"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(|proxy| proxy["name"].as_str())
        .collect::<Vec<_>>();
    let unique_names = names
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(names.len(), unique_names.len());
}
