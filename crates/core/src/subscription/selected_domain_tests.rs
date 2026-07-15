//! 本模块锁定「入口选中域名 → 订阅按该域名出 server/SNI」的渲染契约。
//! 多域名 Phase 5.2:入口可选一个 node_domain;写入侧把选中域名物化进
//! access_lines.server_name / listen_host,订阅渲染层必须忠实反映该物化值。
//! 选了 domainB(CF/直连)→ 订阅 server 与 sni(servername)都必须是 domainB。
//! 未选域名(物化保持节点主域名/IP)→ 订阅沿用节点主域名/IP,行为同今天。
//! 关键防泄露回归:选中 domainB 时订阅绝不能回退/泄露节点的另一(主)域名。
//! 这些用例只用内存 Store 与示例域名/RFC 文档 IP,不访问数据库或网络。
//! 选中域名 → server_name/listen_host 的物化映射由 db 层 PostgreSQL 集成测试覆盖。
//! 本模块只验证订阅渲染对已物化字段的忠实反映,与 cert_sni_tests 互补。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;
use serde_json::json;

/// 取订阅渲染后的第一个代理节点,供断言 server/sni/network。
fn first_proxy(store: &MemoryStore) -> serde_yaml::Value {
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    profile["proxies"].as_sequence().unwrap()[0].clone()
}

#[test]
fn test_subscription_uses_selected_cf_domain_b_for_server_and_sni() {
    // 入口选中 CF domainB:写入侧已把 listen_host/server_name/cdn_hostname 物化成 domainB。
    // 订阅必须 server=domainB、servername=domainB、network=ws,客户端连 domainB 的 CF 边缘。
    // 节点主域名是 domainA(direct.example.test):订阅里绝不能出现 domainA。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "vless".to_string();
        line.transport = "ws".to_string();
        // 选中 domainB 物化:对外公布地址与 SNI 都是 domainB。
        line.listen_host = "domain-b.example.test".to_string();
        line.server_name = "domain-b.example.test".to_string();
        line.xhttp_host = "domain-b.example.test".to_string();
        line.flow.clear();
        line.udp_enabled = false;
        line.inbound_config = json!({
            "security": "tls",
            "server_name": "domain-b.example.test"
        });
    });

    let proxy = first_proxy(&store);
    assert_eq!(proxy["type"], "vless");
    assert_eq!(proxy["server"], "domain-b.example.test");
    assert_eq!(proxy["servername"], "domain-b.example.test");
    assert_eq!(proxy["network"], "ws");
    assert_eq!(proxy["tls"], true);
    // 防泄露/防回退:订阅绝不能出现节点主域名 domainA。
    let rendered = serde_yaml::to_string(&proxy).unwrap();
    assert!(!rendered.contains("direct.example.test"));
}

#[test]
fn test_subscription_switch_selected_domain_a_to_b_moves_traffic() {
    // 防封轮换语义:入口选中从 domainA 切到 domainB(写入侧重物化),
    // 订阅 server/SNI 随之从 domainA 变成 domainB,客户端流量迁到 domainB。
    let store = MemoryStore::seeded();
    // 先选 domainA。
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "trojan".to_string();
        line.transport = "tcp".to_string();
        line.listen_host = "203.0.113.7".to_string();
        line.server_name = "domain-a.example.test".to_string();
        line.inbound_config = json!({"security": "tls", "server_name": "domain-a.example.test"});
    });
    let proxy_a = first_proxy(&store);
    assert_eq!(proxy_a["sni"], "domain-a.example.test");

    // 切到 domainB(写入侧把 server_name 重物化成 domainB)。
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.server_name = "domain-b.example.test".to_string();
        line.inbound_config = json!({"security": "tls", "server_name": "domain-b.example.test"});
    });
    let proxy_b = first_proxy(&store);
    assert_eq!(proxy_b["sni"], "domain-b.example.test");
    assert_eq!(proxy_b["servername"], "domain-b.example.test");
    // 切换后绝不能再泄露旧域名 domainA。
    let rendered_b = serde_yaml::to_string(&proxy_b).unwrap();
    assert!(!rendered_b.contains("domain-a.example.test"));
}

#[test]
fn test_subscription_no_selected_domain_falls_back_to_node_primary() {
    // 未选 node_domain:写入侧物化保持节点主域名/IP(行为同今天)。
    // 直连 TLS 入口 listen_host=IP、server_name=节点主域名 → 订阅 server=IP、sni=主域名。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "trojan".to_string();
        line.transport = "tcp".to_string();
        // 节点主域名/IP:未选域名时的现状物化。
        line.listen_host = "198.51.100.20".to_string();
        line.server_name = "node-primary.example.test".to_string();
        line.inbound_config = json!({
            "security": "tls",
            "server_name": "node-primary.example.test"
        });
    });

    let proxy = first_proxy(&store);
    assert_eq!(proxy["type"], "trojan");
    // 未选域名 → server=节点直连 IP、sni=节点主域名,与今天一致。
    assert_eq!(proxy["server"], "198.51.100.20");
    assert_eq!(proxy["sni"], "node-primary.example.test");
    // 关键回归:SNI 绝不能回退成 IP。
    assert_ne!(proxy["sni"], "198.51.100.20");
}
