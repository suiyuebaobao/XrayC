//! 本模块覆盖订阅生成的安全边界和可配置行为。
//! 测试重点是确保客户端只看到中转入口，并验证不同协议的订阅形状。
//! 这些用例随生产模块拆分移动，断言保持原有语义。
//! 分组测试验证单层分组直接输出已绑定中转入口。
//! 中转入口数量和自定义选项在这里集中覆盖。
//! 测试只使用内存 Store，不依赖数据库或网络。
//! 新增测试应避免写入外部状态，保持确定性。
//! 注释使用中文，符合仓库源码头部约束。
//! 本头部满足前十行中文注释约束。

use super::*;
use crate::MemoryStore;
use base64::Engine;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};

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
fn subscription_uses_access_line_and_hides_exit_endpoint() {
    let store = MemoryStore::seeded();
    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");

    assert!(yaml.contains("access.example.test"));
    assert!(yaml.contains("香港 01"));
    assert!(!yaml.contains("🇭🇰 香港 01"));
    assert!(yaml.contains("tls: false"));
    assert!(!yaml.contains("flow:"));
    assert!(!yaml.contains("client-fingerprint: chrome"));
    assert!(!yaml.contains("servername: www.cloudflare.com"));
    assert!(!yaml.contains("reality-opts:"));
    assert!(!yaml.contains("public-key: seeded-public-key"));
    assert!(!yaml.contains("short-id: a1b2c3d4"));
    assert!(!yaml.contains("198.51.100.10"));
    assert!(!yaml.contains("local-direct"));
    assert!(!yaml.contains("demo@example.test"));
}

#[test]
fn reality_subscription_uses_explicit_inbound_security() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.flow = "xtls-rprx-vision".to_string();
        line.udp_enabled = false;
        line.inbound_config = serde_json::json!({
            "security": "reality",
            "private_key": "server-private-key"
        });
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("reality subscription should render");

    assert!(yaml.contains("flow: xtls-rprx-vision"));
    assert!(yaml.contains("tls: true"));
    assert!(yaml.contains("client-fingerprint: chrome"));
    assert!(yaml.contains("servername: www.cloudflare.com"));
    assert!(yaml.contains("reality-opts:"));
    assert!(yaml.contains("public-key: seeded-public-key"));
    assert!(yaml.contains("short-id: a1b2c3d4"));
    assert!(!yaml.contains("server-private-key"));
}

#[test]
fn subscription_hides_reality_xudp_dirty_line() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.inbound_config = json!({
            "security": "reality",
            "private_key": "server-private-key"
        });
        line.udp_enabled = true;
        line.udp_packet_encoding = "xudp".to_string();
    });

    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("dirty Reality XUDP line should be hidden");

    assert_eq!(err, SubscriptionError::NoAvailableLines);
}

#[test]
fn subscription_hides_hy2_dirty_tcp_line() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "hysteria".to_string();
        line.transport = "tcp".to_string();
        line.udp_enabled = false;
        line.server_name = "hy2.example.test".to_string();
        line.inbound_config = json!({"security": "tls"});
    });

    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("dirty HY2 TCP line should be hidden");

    assert_eq!(err, SubscriptionError::NoAvailableLines);
}

#[test]
fn subscription_hides_arbitrary_third_party_endpoint_material() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let endpoint = &mut data.exit_pools.values_mut().next().unwrap().members[0];
        endpoint.resource_name = "third-party-secret-resource".to_string();
        endpoint.outbound_type = crate::EndpointType::Socks;
        endpoint.host = "sensitive-upstream.example.test".to_string();
        endpoint.port = 3010;
        endpoint.outbound_config = serde_json::json!({
            "username": "secret-user",
            "password": "secret-password",
            "raw_url": "socks5://secret-user:secret-password@sensitive-upstream.example.test:3010"
        });
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");

    assert!(yaml.contains("access.example.test"));
    assert!(!yaml.contains("sensitive-upstream.example.test"));
    assert!(!yaml.contains("secret-user"));
    assert!(!yaml.contains("secret-password"));
    assert!(!yaml.contains("third-party-secret-resource"));
    assert!(!yaml.contains("raw_url"));
}

#[test]
fn vless_xhttp_subscription_keeps_selected_transport() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.transport = "xhttp".to_string();
        line.xhttp_path.clear();
        line.xhttp_mode.clear();
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["network"], "xhttp");
    assert!(proxy["packet-encoding"].is_null());
    assert_eq!(proxy["xhttp-opts"]["path"], "/xrayc");
    assert_eq!(proxy["xhttp-opts"]["mode"], "stream-one");
    assert!(!yaml.contains("198.51.100.10"));
    assert!(!yaml.contains("local-direct"));
}

#[test]
fn vless_grpc_subscription_emits_grpc_options() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.transport = "grpc".to_string();
        line.xhttp_path = "xrayc-grpc".to_string();
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["network"], "grpc");
    assert_eq!(proxy["grpc-opts"]["grpc-service-name"], "xrayc-grpc");
}

#[test]
fn vless_xudp_subscription_emits_packet_encoding() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.udp_packet_encoding = "xudp".to_string();
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["network"], "tcp");
    assert_eq!(proxy["packet-encoding"], "xudp");
}

#[test]
fn vless_subscription_reflects_udp_disabled() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.udp_enabled = false;
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["type"], "vless");
    assert_eq!(proxy["network"], "tcp");
    assert_eq!(proxy["udp"], false);
}

#[test]
fn trojan_subscription_uses_password_shape() {
    let store = MemoryStore::seeded();
    let expected_credential = store.read(|data| {
        let line = data.access_lines.values().next().unwrap();
        let user = data.users.values().next().unwrap();
        binding_credential("trojan", &user.access_credential, line.id)
    });
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "trojan".to_string();
        line.udp_enabled = false;
        line.server_name = "trojan.example.test".to_string();
        line.inbound_config = json!({"security": "tls"});
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("trojan subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["type"], "trojan");
    assert_eq!(proxy["password"], expected_credential.as_str());
    assert_eq!(proxy["tls"], true);
    assert_eq!(proxy["sni"], "trojan.example.test");
    assert_eq!(proxy["udp"], false);
    assert!(proxy["uuid"].is_null());
    assert!(proxy["flow"].is_null());
    assert!(proxy["reality-opts"].is_null());
    assert!(!yaml.contains("seeded-public-key"));
}

#[test]
fn trojan_subscription_without_tls_is_hidden() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "trojan".to_string();
        line.inbound_config = json!({"security": "none"});
        line.server_name.clear();
    });

    let error = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("cleartext trojan line should not render");
    assert_eq!(error, SubscriptionError::NoAvailableLines);
}

#[test]
fn legacy_shadowsocks_subscription_is_hidden() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "shadowsocks".to_string();
        line.inbound_config = serde_json::json!({
            "method": "aes-256-gcm",
            "password": "line-root-secret"
        });
    });

    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("legacy shadowsocks cannot be user-billed");

    assert_eq!(err, SubscriptionError::NoAvailableLines);
}

#[test]
fn shadowsocks_2022_subscription_uses_user_scoped_password() {
    let store = MemoryStore::seeded();
    let expected_credential = store.read(|data| {
        let line = data.access_lines.values().next().unwrap();
        let user = data.users.values().next().unwrap();
        binding_credential("shadowsocks", &user.access_credential, line.id)
    });
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "shadowsocks".to_string();
        line.inbound_config = serde_json::json!({
            "method": "2022-blake3-aes-128-gcm",
            "password": "line-root-secret"
        });
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("ss2022 subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy = &profile["proxies"].as_sequence().unwrap()[0];

    assert_eq!(proxy["type"], "ss");
    assert_eq!(proxy["cipher"], "2022-blake3-aes-128-gcm");
    let expected_key = base64::engine::general_purpose::STANDARD
        .encode(&Sha256::digest(expected_credential.as_bytes())[..16]);
    assert_eq!(
        proxy["password"],
        format!("line-root-secret:{expected_key}").as_str()
    );
    assert!(proxy["uuid"].is_null());
    assert!(proxy["flow"].is_null());
}

#[test]
fn shadowsocks_2022_subscription_requires_server_password() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "shadowsocks".to_string();
        line.inbound_config = serde_json::json!({
            "method": "2022-blake3-aes-128-gcm"
        });
    });

    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("incomplete ss2022 line should not render");

    assert_eq!(err, SubscriptionError::NoAvailableLines);
}

#[test]
fn unsupported_access_protocols_are_not_rendered_as_vless() {
    for protocol in ["http", "socks", "vmess"] {
        let store = MemoryStore::seeded();
        store.write(|data| {
            data.access_lines.values_mut().next().unwrap().protocol = protocol.to_string();
        });

        let err = store
            .read(|data| generate_clash_yaml(data, "demo-token"))
            .expect_err("unsupported protocol should not render");

        assert_eq!(err, SubscriptionError::NoAvailableLines);
    }
}

#[test]
fn subscription_filters_exhausted_pool() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let subscription = data.subscriptions.values_mut().next().unwrap();
        subscription.used_bytes = subscription.limit_bytes;
    });

    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("exhausted pool should hide line");

    assert_eq!(err, SubscriptionError::NoAvailableLines);
}

#[test]
fn subscription_rejects_disabled_or_inactive_user() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        data.users.values_mut().next().unwrap().disabled = true;
    });
    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("disabled user should not render subscription");
    assert_eq!(err, SubscriptionError::TokenNotFound);

    store.write(|data| {
        data.users.values_mut().next().unwrap().disabled = false;
        let subscription = data.subscriptions.values_mut().next().unwrap();
        subscription.active = false;
    });
    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("inactive subscription should not render subscription");
    assert_eq!(err, SubscriptionError::TokenNotFound);

    store.write(|data| {
        let subscription = data.subscriptions.values_mut().next().unwrap();
        subscription.active = true;
        subscription.expires_at = Utc::now() - chrono::Duration::seconds(1);
    });
    let err = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect_err("expired subscription should not render subscription");
    assert_eq!(err, SubscriptionError::TokenNotFound);
}

#[test]
fn subscription_uses_one_line_across_plan_line_groups() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = first_line_id_for_group(data, first_group_id);
        let first_pool_id = data.access_lines[&first_line_id].exit_pool_id;
        let second_group_id = uuid::Uuid::new_v4();
        let second_line_id = uuid::Uuid::new_v4();
        let second_pool_id = uuid::Uuid::new_v4();
        let second_endpoint_id = uuid::Uuid::new_v4();
        let mut second_line = data.access_lines[&first_line_id].clone();
        second_line.id = second_line_id;
        second_line.name = "日本 01".to_string();
        second_line.line_group_id = None;
        second_line.exit_endpoint_id = Some(second_endpoint_id);
        second_line.exit_pool_id = second_pool_id;
        second_line.listen_port = 8443;
        second_line.region_code = "JP".to_string();
        second_line.region_name = "日本".to_string();
        second_line.region_flag = "🇯🇵".to_string();
        data.access_lines.insert(second_line_id, second_line);
        let mut second_pool = data.exit_pools[&first_pool_id].clone();
        second_pool.id = second_pool_id;
        second_pool.name = "日本高速".to_string();
        second_pool.members[0].id = second_endpoint_id;
        data.exit_pools.insert(second_pool_id, second_pool);
        data.line_groups.insert(
            second_group_id,
            crate::LineGroup {
                id: second_group_id,
                name: "日本线路组".to_string(),
                country_code: "JP".to_string(),
                icon: "🇯🇵".to_string(),
                sort_weight: 100,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: Some(second_pool_id),
                line_ids: vec![second_endpoint_id],
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
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(profile["proxies"].as_sequence().unwrap().len(), 2);
    assert!(yaml.contains("香港 01"));
    assert!(yaml.contains("日本 01"));
    assert!(!yaml.contains("🇭🇰 香港 01"));
    assert!(!yaml.contains("🇯🇵 日本 01"));
}

#[test]
fn subscription_keeps_one_access_line_from_same_exit_pool() {
    let store = MemoryStore::seeded();
    store.write(|data| {
        let group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let first_line_id = first_line_id_for_group(data, group_id);
        let second_line_id = uuid::Uuid::new_v4();
        let mut second_line = data.access_lines[&first_line_id].clone();
        second_line.id = second_line_id;
        second_line.name = "香港备用入口".to_string();
        second_line.listen_port = 8444;
        data.access_lines.insert(second_line_id, second_line);
    });

    let yaml = store
        .read(|data| generate_clash_yaml(data, "demo-token"))
        .expect("subscription should render");
    let profile: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(profile["proxies"].as_sequence().unwrap().len(), 2);
    assert!(yaml.contains("香港 01"));
    assert!(yaml.contains("香港备用入口"));
    assert!(!yaml.contains("🇭🇰 香港 01"));
    assert!(!yaml.contains("🇭🇰 香港备用入口"));
}
