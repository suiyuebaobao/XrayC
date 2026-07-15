// 数据库测试分片 53。
// 本文件覆盖 CF 入口证书按节点 cf_cert_mode 锚定的分流(Phase 6)。
// dns01:CF 域名走 DNS-01 签自己的证书,CF 入口证书路径锚定 live/{cf_domain}/。
// reuse_direct(历史节点):CF 入口复用灰云证书,证书路径仍锚定 live/{cert_domain}/。
// 直连 TLS 入口与订阅 SNI 不在本文件改写,只在 part50 守卫(此处只验 CF 线)。
// cf_cert_mode 写入侧由 cf_domain 派生(有 CF 域名=dns01),reuse_direct 模拟历史落库态。
// 测试只用示例域名(example.test)、RFC 5737 文档 IP 和测试 UUID,不含真实资产。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
// 读回断言以 access_entries 落库 inbound_config 为唯一真相,符合跨层对齐红线。
// 本头部满足前十行中文注释约束。

/// 构造一个 CF VLESS-WS-TLS 入口输入,listen_host 给 IP,cdn_hostname=cf_domain。
/// 聚焦证书路径锚定,server_name(SNI)仍由物化层取 cf_domain。
fn cf_mode_vless_ws_entry_input(node_id: Uuid, name: &str, port: u16) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: "203.0.113.71".to_string(),
        listen_port: port,
        protocol: "vless".to_string(),
        transport: "ws".to_string(),
        security: "tls".to_string(),
        server_name: String::new(),
        ws_path: "/cfmode".to_string(),
        ws_host: "cf.example.test".to_string(),
        cdn_enabled: true,
        cdn_provider: "cloudflare".to_string(),
        cdn_hostname: "cf.example.test".to_string(),
        cdn_server: "cf.example.test".to_string(),
        enabled: true,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

#[tokio::test]
async fn test_cf_entry_cert_anchored_by_mode() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL CF cert mode anchoring test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // dns01 节点:配了 cf_domain → 写入侧派生 cf_cert_mode=dns01。
    // CF 入口证书必须锚定 CF 域名自己的 LE 路径 live/{cf_domain}/。
    let dns01_node = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-dns01-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.70".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-dns01-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("gray.example.test".to_string()),
            acme_email: Some("ops@example.test".to_string()),
            cf_enabled: true,
            cf_domain: Some("cf.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();
    // 默认派生已是 reuse_direct(token-less);本测试要验 dns01 锚定,故显式置 dns01(模拟运维显式选 DNS-01)。
    sqlx::query("UPDATE access_nodes SET cf_cert_mode = 'dns01' WHERE id = $1")
        .bind(dns01_node)
        .execute(store.pool())
        .await
        .unwrap();

    let dns01_entry = store
        .create_admin_access_entry(cf_mode_vless_ws_entry_input(dns01_node, "cf-dns01-ws", 443))
        .await
        .unwrap();

    let dns01_inbound: serde_json::Value =
        sqlx::query_scalar("SELECT inbound_config FROM access_entries WHERE id = $1")
            .bind(dns01_entry)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let dns01_cert = dns01_inbound["certificate_file"].as_str().unwrap();
    // dns01:证书锚定 CF 域名自己的 LE 路径,而不是灰云 cert_domain/IP。
    assert_eq!(
        dns01_cert,
        "/etc/letsencrypt/live/cf.example.test/fullchain.pem"
    );
    assert_eq!(
        dns01_inbound["key_file"].as_str().unwrap(),
        "/etc/letsencrypt/live/cf.example.test/privkey.pem"
    );
    assert!(!dns01_cert.contains("gray.example.test"));
    assert!(!dns01_cert.contains("203.0.113"));

    // reuse_direct 节点:模拟历史落库态(cf_domain 非空但 cf_cert_mode 仍 reuse_direct)。
    // 先建节点(派生 dns01),再直接改写列为 reuse_direct,验证 CF 入口仍锚 cf 域名自己的证书(不复用灰云)。
    let reuse_node = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-reuse-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.72".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-reuse-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("gray.example.test".to_string()),
            acme_email: Some("ops@example.test".to_string()),
            cf_enabled: true,
            cf_domain: Some("cf.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();
    sqlx::query("UPDATE access_nodes SET cf_cert_mode = 'reuse_direct' WHERE id = $1")
        .bind(reuse_node)
        .execute(store.pool())
        .await
        .unwrap();

    let reuse_entry = store
        .create_admin_access_entry(cf_mode_vless_ws_entry_input(reuse_node, "cf-reuse-ws", 443))
        .await
        .unwrap();

    let reuse_inbound: serde_json::Value =
        sqlx::query_scalar("SELECT inbound_config FROM access_entries WHERE id = $1")
            .bind(reuse_entry)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let reuse_cert = reuse_inbound["certificate_file"].as_str().unwrap();
    // CF 入口一律锚 cf 域名自己的证书(per-domain,不复用直连灰云 cert_domain;用户定:不复用别的域名证书)。
    assert_eq!(
        reuse_cert,
        "/etc/letsencrypt/live/cf.example.test/fullchain.pem"
    );
    assert!(!reuse_cert.contains("gray.example.test"));
    assert!(!reuse_cert.contains("203.0.113"));
}

/// token-less CF 节点「只有 cf 域名、无直连证书域名」:reuse_direct 应回退锚定 cf 域名自己的
/// HTTP-01 证书路径(agent 经 CF :80 自动签,免 token、免单独配直连域名),而不是空 cert_domain → 无证书被跳过。
#[tokio::test]
async fn test_cf_entry_reuse_direct_without_cert_domain_anchors_to_cf_domain() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping reuse_direct cf-domain anchor test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 只配 cf 域名(无 cert_domain、无 acme)→ cf_cert_mode 默认 reuse_direct。
    let node = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-onlycf-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.73".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-onlycf-token-{}", Uuid::new_v4().simple()),
            cert_domain: None,
            acme_email: None,
            cf_enabled: true,
            cf_domain: Some("onlycf.example.test".to_string()),
            ip_direct_address: Some("203.0.113.73".to_string()),
        })
        .await
        .unwrap();

    let entry = store
        .create_admin_access_entry(cf_mode_vless_ws_entry_input(node, "cf-onlycf-ws", 443))
        .await
        .unwrap();
    let inbound: serde_json::Value =
        sqlx::query_scalar("SELECT inbound_config FROM access_entries WHERE id = $1")
            .bind(entry)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let cert = inbound["certificate_file"].as_str().unwrap();
    assert_eq!(
        cert, "/etc/letsencrypt/live/onlycf.example.test/fullchain.pem",
        "无直连证书域名时 reuse_direct CF 入口应锚定 cf 域名自己的 HTTP-01 证书,实际: {cert}"
    );
}
