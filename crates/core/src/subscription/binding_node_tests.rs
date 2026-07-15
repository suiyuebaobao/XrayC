//! 本模块验证入口出口绑定节点的订阅语义。
//! 分组应选择绑定节点，而不是直接选择出口端点。
//! 一个绑定节点必须渲染成一个客户端代理节点。
//! 多入口共用同一出口时，订阅不能被出口端点去重。
//! 单入口绑定多个出口时，订阅也必须保留多条节点。
//! 多条节点共享入口地址时，需要使用绑定级用户凭证区分路由。
//! WebSocket 入口必须输出客户端可用的 ws-opts。
//! 测试只使用内存 Store，不触碰数据库和远端节点。
//! 新增断言优先覆盖用户可见订阅形状。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::{EndpointType, ExitEndpoint, ExitPool, MemoryStore};
use serde_json::json;
use uuid::Uuid;

#[test]
fn binding_node_two_entries_one_exit_emit_two_subscription_nodes() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = data.access_lines.keys().next().copied().unwrap();
        let first_line = data.access_lines[&first_line_id].clone();
        let second_line_id = Uuid::new_v4();
        let mut second_line = first_line.clone();
        second_line.id = second_line_id;
        second_line.name = "入口 B + 出口 1".to_string();
        second_line.listen_port = first_line.listen_port + 1;
        data.access_lines.insert(second_line_id, second_line);

        let group = data.line_groups.get_mut(&group_id).unwrap();
        group.line_ids.clear();
        group.binding_node_ids = vec![first_line_id, second_line_id];
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("binding nodes should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = profile["proxies"].as_sequence().unwrap();
    let names = proxies
        .iter()
        .filter_map(|proxy| proxy["name"].as_str())
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(proxies.len(), 2);
    assert!(names.contains("香港 01"));
    assert!(names.contains("入口 B + 出口 1"));
}

#[test]
fn binding_node_one_entry_two_exits_emit_distinct_credentials() {
    let store = MemoryStore::seeded();
    let base_credential = store.read(|data| {
        data.users
            .values()
            .next()
            .unwrap()
            .access_credential
            .clone()
    });
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = data.access_lines.keys().next().copied().unwrap();
        let first_line = data.access_lines[&first_line_id].clone();
        let second_endpoint_id = Uuid::new_v4();
        let second_pool_id = Uuid::new_v4();
        let second_line_id = Uuid::new_v4();

        let endpoint = ExitEndpoint {
            id: second_endpoint_id,
            resource_name: "second-test-exit".to_string(),
            ownership: "third_party".to_string(),
            owner_access_node_id: None,
            outbound_type: EndpointType::Socks,
            host: "203.0.113.20".to_string(),
            port: 1080,
            outbound_config: json!({}),
            stream_config: json!({}),
            weight: 100,
            priority: 100,
            allow_new_assignments: true,
            healthy: true,
            status: "healthy".to_string(),
        };
        data.exit_pools.insert(
            second_pool_id,
            ExitPool {
                id: second_pool_id,
                name: "第二出口池".to_string(),
                region_code: "US".to_string(),
                strategy: "priority".to_string(),
                enabled: true,
                members: vec![endpoint],
            },
        );

        let mut second_line = first_line.clone();
        second_line.id = second_line_id;
        second_line.name = "入口 A + 出口 2".to_string();
        second_line.exit_pool_id = second_pool_id;
        second_line.exit_endpoint_id = Some(second_endpoint_id);
        data.access_lines.insert(second_line_id, second_line);

        let group = data.line_groups.get_mut(&group_id).unwrap();
        group.line_ids.clear();
        group.binding_node_ids = vec![first_line_id, second_line_id];
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("binding nodes should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = profile["proxies"].as_sequence().unwrap();
    let first_uuid = proxies[0]["uuid"].as_str().unwrap();
    let second_uuid = proxies[1]["uuid"].as_str().unwrap();

    assert_eq!(proxies.len(), 2);
    assert_ne!(first_uuid, second_uuid);
    assert_ne!(first_uuid, base_credential);
    assert_ne!(second_uuid, base_credential);
}

#[test]
fn binding_node_vless_tls_websocket_emits_ws_options() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.listen_host = "cdn-entry.example.test".to_string();
        line.transport = "ws".to_string();
        line.server_name = "cdn-entry.example.test".to_string();
        line.xhttp_path = "/vless-ws".to_string();
        line.xhttp_host = "cdn-entry.example.test".to_string();
        line.inbound_config = json!({"security": "tls"});
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("websocket binding node should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["type"], "vless");
    assert_eq!(proxy["server"], "cdn-entry.example.test");
    assert_eq!(proxy["port"], 443);
    assert_eq!(proxy["network"], "ws");
    assert_eq!(proxy["tls"], true);
    assert_eq!(proxy["servername"], "cdn-entry.example.test");
    assert_eq!(proxy["ws-opts"]["path"], "/vless-ws");
    assert_eq!(
        proxy["ws-opts"]["headers"]["Host"],
        "cdn-entry.example.test"
    );
    assert!(proxy["xhttp-opts"].is_null());
}

// 本机出口(ownership=self_hosted)是中转节点同机自家出口、中转直接连它(本地/内网),
// 按 CLAUDE.md §4 与开发方案口径天然可指派、不需探测就绪。回归探测死锁:
// 本机出口被探测标 unhealthy(healthy=false)后,绑定它的入口本应仍可渲染——
// 否则会陷入"要监听才能探测健康、要健康才下发监听"的死锁(CF 节点 0 inbound 真根因)。
#[test]
fn self_hosted_exit_remains_assignable_when_probe_marks_unhealthy() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        // 把种子里唯一出口改成"本机自建"出口,并模拟探测把它标成不健康(同机出口探测死锁场景)。
        let pool = data.exit_pools.values_mut().next().unwrap();
        let endpoint = pool.members.first_mut().unwrap();
        endpoint.ownership = "self_hosted".to_string();
        endpoint.healthy = false;
        // member 行状态保持 healthy(种子默认),只有 resource 探测态把 endpoint.healthy 拉成 false。
    });

    // 修复前:本机出口被探测态过滤,订阅整单无可用线路 -> 这里会 panic(应红)。
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("self_hosted exit should stay assignable despite probe-unhealthy");
    assert!(
        yaml.contains("香港 01"),
        "本机出口线路应仍出现在订阅里:\n{yaml}"
    );
}
