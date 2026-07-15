//! 本模块覆盖 CF 入口与直连 TLS 入口的订阅 server/SNI 分流。
//! CF 入口订阅必须公布 cf_domain 作为 server 与 sni,network=ws。
//! 直连 TLS 入口(Trojan/HY2/VLESS-WS-TLS)订阅 sni 必须是 cert_domain。
//! listen_host 是 IP 时 sni 也必须是 cert_domain,不能回退成 IP。
//! Reality 借用 dest 不变,Shadowsocks 无 sni 不变,作为负向回归守卫。
//! 这些用例直接渲染订阅,断言订阅层忠实反映物化后的 server_name/listen_host。
//! 测试只使用内存 Store 与示例域名/RFC 文档 IP,不访问数据库或网络。
//! cert_domain→server_name 的物化映射由 db 层 PostgreSQL 集成测试覆盖。
//! 新增分流字段必须同步补订阅断言,防止 SNI 误回退泄露 IP。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;
use serde_json::json;

/// 取订阅渲染后的第一个代理节点,供断言 server/sni/network。
fn first_proxy_yaml(store: &MemoryStore) -> serde_yaml::Value {
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    profile["proxies"].as_sequence().unwrap()[0].clone()
}

#[test]
fn test_subscription_cf_line_uses_cf_domain() {
    // CF VLESS-WS-TLS 入口:物化后 listen_host=cdn_hostname=cf_domain、server_name=cf_domain。
    // 订阅必须 server=cf_domain、sni(servername)=cf_domain、network=ws,客户端连 CF 边缘代理。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "vless".to_string();
        line.transport = "ws".to_string();
        line.listen_host = "cdn.example.test".to_string();
        line.server_name = "cdn.example.test".to_string();
        line.flow.clear();
        line.udp_enabled = false;
        line.xhttp_host = "cdn.example.test".to_string();
        line.inbound_config = json!({
            "security": "tls",
            "server_name": "cdn.example.test",
            "certificate_file": "/etc/letsencrypt/live/direct.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/direct.example.test/privkey.pem"
        });
    });

    let proxy = first_proxy_yaml(&store);
    assert_eq!(proxy["type"], "vless");
    assert_eq!(proxy["server"], "cdn.example.test");
    assert_eq!(proxy["servername"], "cdn.example.test");
    assert_eq!(proxy["network"], "ws");
    assert_eq!(proxy["tls"], true);
}

#[test]
fn test_subscription_direct_line_sni_is_cert_domain() {
    // 直连 Trojan(TLS)入口:listen_host 是 IP,但物化后 server_name=cert_domain。
    // 订阅 server=IP(直连地址)、sni=cert_domain(关键:不能回退成 IP)。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "trojan".to_string();
        line.transport = "tcp".to_string();
        line.listen_host = "203.0.113.7".to_string();
        line.server_name = "direct.example.test".to_string();
        line.inbound_config = json!({
            "security": "tls",
            "server_name": "direct.example.test",
            "certificate_file": "/etc/letsencrypt/live/direct.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/direct.example.test/privkey.pem"
        });
    });

    let proxy = first_proxy_yaml(&store);
    assert_eq!(proxy["type"], "trojan");
    assert_eq!(proxy["server"], "203.0.113.7");
    assert_eq!(proxy["sni"], "direct.example.test");
    assert_eq!(proxy["servername"], "direct.example.test");
    // 关键回归:sni 绝不能是 IP。
    assert_ne!(proxy["sni"], "203.0.113.7");
}

#[test]
fn test_subscription_direct_hy2_line_sni_is_cert_domain() {
    // 直连 HY2 入口:listen_host 是 IP,server_name=cert_domain,订阅 sni=cert_domain。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "hysteria".to_string();
        line.transport = "hysteria".to_string();
        line.listen_host = "203.0.113.8".to_string();
        line.server_name = "direct.example.test".to_string();
        line.udp_enabled = true;
        line.udp_packet_encoding.clear();
        line.inbound_config = json!({"security": "tls"});
    });

    let proxy = first_proxy_yaml(&store);
    assert_eq!(proxy["type"], "hysteria2");
    assert_eq!(proxy["server"], "203.0.113.8");
    assert_eq!(proxy["sni"], "direct.example.test");
    assert_ne!(proxy["sni"], "203.0.113.8");
}

#[test]
fn test_subscription_reality_line_sni_unchanged_borrows_dest() {
    // 负向回归:Reality 入口 sni 仍借用 dest(server_name=www.cloudflare.com),不受 cert_domain 影响。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "vless".to_string();
        line.transport = "tcp".to_string();
        line.flow = "xtls-rprx-vision".to_string();
        line.udp_enabled = false;
        line.server_name = "www.cloudflare.com".to_string();
        line.inbound_config = json!({
            "security": "reality",
            "private_key": "server-private-key"
        });
    });

    let proxy = first_proxy_yaml(&store);
    assert_eq!(proxy["type"], "vless");
    assert_eq!(proxy["servername"], "www.cloudflare.com");
    assert!(proxy.get("sni").map(|v| v.is_null()).unwrap_or(true));
}

#[test]
fn test_subscription_shadowsocks_line_has_no_sni() {
    // 负向回归:Shadowsocks 入口订阅无 sni/servername 字段,不受 cert_domain 影响。
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "shadowsocks".to_string();
        line.transport = "tcp".to_string();
        line.server_name = "direct.example.test".to_string();
        line.inbound_config = json!({
            "method": "2022-blake3-aes-128-gcm",
            "password": "c2VydmVyLXBhc3N3b3Jk"
        });
    });

    let proxy = first_proxy_yaml(&store);
    assert_eq!(proxy["type"], "ss");
    assert!(proxy.get("sni").map(|v| v.is_null()).unwrap_or(true));
    assert!(proxy.get("servername").map(|v| v.is_null()).unwrap_or(true));
}
