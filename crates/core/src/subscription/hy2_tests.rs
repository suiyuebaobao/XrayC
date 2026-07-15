//! 本模块覆盖 HY2 用户入口订阅渲染。
//! HY2 入口必须只暴露中转节点地址和用户凭据。
//! 上游出口地址、密码、UUID 和代理 URL 不得进入订阅。
//! 这些测试独立成文件，避免主订阅测试文件继续膨胀。
//! 测试只使用内存 Store，不访问真实网络或数据库。
//! HY2 节点要求 TLS/SNI，否则不能作为用户入口下发。
//! 订阅字段按 mihomo 的 hysteria2 形态断言。
//! 新增字段必须保持可被客户端导入。
//! 注释使用中文，符合仓库规则。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;
use serde_json::json;

#[test]
fn hy2_subscription_uses_hysteria2_shape() {
    let store = MemoryStore::seeded();
    let expected_credential = store.read(|data| {
        let line = data.access_lines.values().next().unwrap();
        let user = data.users.values().next().unwrap();
        binding_credential("hysteria", &user.access_credential, line.id)
    });
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "hysteria".to_string();
        line.transport = "hysteria".to_string();
        line.server_name = "hy2.example.test".to_string();
        line.inbound_config = json!({"security": "tls"});
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("hy2 subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["type"], "hysteria2");
    assert_eq!(proxy["password"], expected_credential.as_str());
    assert_eq!(proxy["sni"], "hy2.example.test");
    assert_eq!(proxy["alpn"].as_sequence().unwrap()[0], "h3");
    assert_eq!(proxy["skip-cert-verify"], false);
    assert_eq!(proxy["server"], "access.example.test");
    assert!(proxy["uuid"].is_null());
    assert!(proxy["reality-opts"].is_null());
    assert!(!yaml.contains("198.51.100.10"));
}

#[test]
fn hy2_subscription_without_tls_is_hidden() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "hy2".to_string();
        line.transport = "hysteria".to_string();
        line.inbound_config = json!({"security": "none"});
        line.server_name.clear();
    });

    let error = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("hy2 without tls should not render");

    assert_eq!(error, SubscriptionError::NoAvailableLines);
}
