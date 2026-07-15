// 数据库测试分片 50。
// 本文件覆盖入口证书锚定与直连/CF 线 SNI 物化分流(per-domain)。
// 直连 TLS 入口的 inbound 证书路径必须指向节点 cert_domain 的 LE 路径,
// 而不是 listen_host/cdn_hostname/IP;直连 TLS 入口 server_name 必须 = cert_domain。
// CF 入口 server_name 保持 cf_domain(订阅 sni=cf_domain),证书一律锚 cf_domain 自己的 LE 路径(不复用 cert_domain)。
// Reality/SS 不吃证书,不受影响,作为负向回归守卫。
// 测试只用示例域名(example.test)、RFC 5737 文档 IP 和测试 UUID。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
// 读回断言以 access_entries/access_lines 落库为唯一真相,符合跨层对齐红线。
// 本头部满足前十行中文注释约束。

use crate::store::routing_access_entries::prepare_access_entry;
use crate::store::routing_entry_cert::NodeCertAnchor;
use crate::store::routing_entry_selected_domain::SelectedEntryDomain;

/// 构造一个最小直连 TLS 入口输入(Trojan),listen_host 给 IP,聚焦证书/SNI 锚定。
fn direct_trojan_entry_input(node_id: Uuid, name: &str, port: u16) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: "203.0.113.50".to_string(),
        listen_port: port,
        protocol: "trojan".to_string(),
        transport: "tcp".to_string(),
        security: "tls".to_string(),
        // 故意不传 server_name:由物化层用节点 cert_domain 锚定。
        server_name: String::new(),
        ws_path: String::new(),
        ws_host: String::new(),
        cdn_enabled: false,
        cdn_provider: String::new(),
        cdn_hostname: String::new(),
        cdn_server: String::new(),
        enabled: true,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

/// 构造一个 CF VLESS-WS-TLS 入口输入,listen_host 真实给 IP,cdn_hostname=cf_domain。
fn cf_vless_ws_entry_input(node_id: Uuid, name: &str, port: u16) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: "203.0.113.51".to_string(),
        listen_port: port,
        protocol: "vless".to_string(),
        transport: "ws".to_string(),
        security: "tls".to_string(),
        server_name: String::new(),
        ws_path: "/cfws".to_string(),
        ws_host: "cdn.example.test".to_string(),
        cdn_enabled: true,
        cdn_provider: "cloudflare".to_string(),
        cdn_hostname: "cdn.example.test".to_string(),
        cdn_server: "cdn.example.test".to_string(),
        enabled: true,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

#[test]
fn test_direct_tls_entry_cert_anchored_to_cert_domain() {
    // 直连 TLS 入口(listen_host=IP):证书路径与 server_name(SNI)都必须 = cert_domain。
    let prepared = prepare_access_entry(
        &direct_trojan_entry_input(Uuid::new_v4(), "direct-trojan", 8443),
        "203.0.113.50".to_string(),
        NodeCertAnchor {
            cert_domain: Some("direct.example.test"),
            ..Default::default()
        },
        SelectedEntryDomain::default(),
    )
    .unwrap();

    // server_name 锚定 cert_domain,而不是 IP listen_host。
    assert_eq!(prepared.server_name, "direct.example.test");
    let cert = prepared.inbound_config["certificate_file"].as_str().unwrap();
    let key = prepared.inbound_config["key_file"].as_str().unwrap();
    assert_eq!(
        cert,
        "/etc/letsencrypt/live/direct.example.test/fullchain.pem"
    );
    assert_eq!(key, "/etc/letsencrypt/live/direct.example.test/privkey.pem");
    // 证书路径绝不能含 IP。
    assert!(!cert.contains("203.0.113.50"));
}

#[test]
fn test_cf_entry_cert_anchored_to_node_cert_domain() {
    // CF VLESS-WS 入口:listen_host 物化为 cf_domain(cdn_hostname)。
    // 本用例节点无 cf_domain(NodeCertAnchor 只给 cert_domain):per-domain 下 CF 入口缺 cf_domain 时
    // 回退锚定节点灰云 cert_domain(cf_domain 缺省兜底,回归守卫),证书路径含 live/{cert_domain}/、不含 cf_domain/IP。
    let prepared = prepare_access_entry(
        &cf_vless_ws_entry_input(Uuid::new_v4(), "cf-vless-ws", 443),
        "203.0.113.51".to_string(),
        NodeCertAnchor {
            cert_domain: Some("direct.example.test"),
            ..Default::default()
        },
        SelectedEntryDomain::default(),
    )
    .unwrap();

    // CF 入口对外公布 cf_domain。
    assert_eq!(prepared.listen_host, "cdn.example.test");
    // server_name(订阅 SNI)= cf_domain。
    assert_eq!(prepared.server_name, "cdn.example.test");
    // 证书锚定 cert_domain,不是 cf_domain/IP。
    let cert = prepared.inbound_config["certificate_file"].as_str().unwrap();
    assert_eq!(
        cert,
        "/etc/letsencrypt/live/direct.example.test/fullchain.pem"
    );
    assert!(!cert.contains("cdn.example.test"));
    assert!(!cert.contains("203.0.113.51"));
}

#[test]
fn test_reality_entry_cert_unaffected_by_cert_domain() {
    // 负向回归:Reality 入口不吃证书,传 cert_domain 也不应注入证书路径,server_name 仍借用 dest。
    let input = AdminAccessEntryInput {
        access_node_id: Uuid::new_v4(),
        name: "reality-entry".to_string(),
        listen_host: "203.0.113.52".to_string(),
        listen_port: 8443,
        protocol: "vless".to_string(),
        transport: "tcp".to_string(),
        security: "reality".to_string(),
        server_name: String::new(),
        ws_path: String::new(),
        ws_host: String::new(),
        cdn_enabled: false,
        cdn_provider: String::new(),
        cdn_hostname: String::new(),
        cdn_server: String::new(),
        enabled: true,
        sort_weight: 100,
            node_domain_id: None,
        vless_quantum_encryption: false,
    };
    let prepared = prepare_access_entry(
        &input,
        "203.0.113.52".to_string(),
        NodeCertAnchor {
            cert_domain: Some("direct.example.test"),
            ..Default::default()
        },
        SelectedEntryDomain::default(),
    )
    .unwrap();

    // Reality 不应注入证书文件。
    assert!(prepared.inbound_config.get("certificate_file").is_none());
    assert!(prepared.inbound_config.get("key_file").is_none());
    // server_name 仍是 Reality 借用的 dest 默认值,不会被 cert_domain 覆盖。
    assert_ne!(prepared.server_name, "direct.example.test");
}

#[tokio::test]
async fn test_create_direct_tls_entry_materializes_cert_domain_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL direct TLS cert anchoring test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 节点带 cert_domain,纯直连(cf_enabled=false)。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("direct-cert-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.60".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("direct-cert-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("ops@example.test".to_string()),
            cf_enabled: false,
            cf_domain: None,
            ip_direct_address: None,
        })
        .await
        .unwrap();

    let entry_id = store
        .create_admin_access_entry(direct_trojan_entry_input(node_id, "direct-trojan", 8443))
        .await
        .unwrap();

    // access_entries 落库的 server_name 与证书路径都锚定 cert_domain。
    let (server_name, inbound): (String, serde_json::Value) =
        sqlx::query_as("SELECT server_name, inbound_config FROM access_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(server_name, "direct.example.test");
    assert_eq!(
        inbound["certificate_file"].as_str().unwrap(),
        "/etc/letsencrypt/live/direct.example.test/fullchain.pem"
    );
    assert!(!inbound["certificate_file"]
        .as_str()
        .unwrap()
        .contains("203.0.113.50"));
}

#[tokio::test]
async fn test_create_cf_entry_materializes_cert_domain_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL CF cert anchoring test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 节点开启 CF,cert_domain=灰云域名、cf_domain=橙云域名。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-cert-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.61".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-cert-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("ops@example.test".to_string()),
            cf_enabled: true,
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();

    let entry_id = store
        .create_admin_access_entry(cf_vless_ws_entry_input(node_id, "cf-vless-ws", 443))
        .await
        .unwrap();

    let (listen_host, server_name, inbound): (String, String, serde_json::Value) = sqlx::query_as(
        "SELECT listen_host, server_name, inbound_config FROM access_entries WHERE id = $1",
    )
    .bind(entry_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    // CF 入口对外公布 cf_domain,SNI=cf_domain;证书一律锚 cf_domain 自己的 LE 路径
    // (per-domain,token-less 经 CF :80 HTTP-01 自动签该 cf 域名真证书),绝不复用直连灰云 cert_domain。
    assert_eq!(listen_host, "cdn.example.test");
    assert_eq!(server_name, "cdn.example.test");
    assert_eq!(
        inbound["certificate_file"].as_str().unwrap(),
        "/etc/letsencrypt/live/cdn.example.test/fullchain.pem"
    );
    assert!(!inbound["certificate_file"]
        .as_str()
        .unwrap()
        .contains("direct.example.test"));
}
