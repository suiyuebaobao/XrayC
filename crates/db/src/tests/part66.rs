// 数据库测试分片 66:CF 域名 cf_cert_mode 默认 reuse_direct(token-less 免 token,正经配 CF)。
// 规则:写入侧节点列一律默认 reuse_direct;acme 邮箱不再当 dns01 判据(它是直连 HTTP-01 也要用的邮箱、
// 不是 DNS-01 凭据信号——DNS-01 给 cf_domain 签证书需 CF API token)。有 cf_domain 即 reuse_direct——
// CF 入口复用直连灰云证书,CF Full 非 strict 回源,订阅公布 cf_domain 藏源站 IP,不再把 CF 域名当直连域名绕。
// dns01 只在运维于 cf 域名上显式传 cf_cert_mode='dns01' 时才落(经 add_node_domain),不由 acme 自动推断。
// 真实 PostgreSQL,缺 DATABASE_URL 跳过;只用 example.test 占位与 RFC 5737 文档 IP。
// 父 tests 模块 include! 引入,所有测试串行(--test-threads=1),pg_test_guard 串行化共享库。
// 读回断言以写入侧落库(access_nodes 列 / access_entries inbound_config)为唯一真相。
// 不在此保存任何真实服务器地址、域名或凭据。
// 本头部满足前十行中文注释约束。

/// 建 CF 节点但不给 acme(token-less):cf_cert_mode 应派生 reuse_direct(而非旧的强制 dns01),
/// cf_enabled 仍由 cf_domain 派生为 true。这是「CF 就是 CF、免 token」的核心契约。
#[tokio::test]
async fn test_create_cf_node_without_acme_derives_reuse_direct() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping token-less CF reuse_direct derive test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // cf_domain 非空、acme_email 为空 → token-less,应落 reuse_direct。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-tokenless-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.80".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-tokenless-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("gray.example.test".to_string()),
            acme_email: None,
            cf_enabled: true,
            cf_domain: Some("cf.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(
        fields.cf_cert_mode, "reuse_direct",
        "CF 域名 + 无 acme/token 应派生 reuse_direct(免 token 正经配 CF),实际: {}",
        fields.cf_cert_mode
    );
    // cf_enabled 仍由 cf_domain 派生 true(token-less 不影响 CF 启用)。
    assert!(
        fields.cf_enabled,
        "有 cf_domain 时 cf_enabled 应派生为 true"
    );
    assert_eq!(fields.cf_domain.as_deref(), Some("cf.example.test"));
    // node_domains 的 cf 主行 cf_cert_mode 与节点列一致(reuse_direct)。
    let domains = store.list_node_domains(node_id).await.unwrap();
    let cf = domains
        .iter()
        .find(|d| d.domain == "cf.example.test")
        .expect("cf 主域名行应存在");
    assert_eq!(
        cf.cf_cert_mode.as_deref(),
        Some("reuse_direct"),
        "cf 主域名行 cf_cert_mode 应与节点列一致为 reuse_direct"
    );
}

/// 关键修正回归:CF 节点即便有 acme 邮箱(直连证书 HTTP-01 要用),cf_cert_mode 仍应 reuse_direct。
/// acme 不是 DNS-01 凭据信号,不能据此误判 dns01——否则会锚到签不出的 cf_domain DNS-01 路径、被 BUG-D 跳过。
#[tokio::test]
async fn test_create_cf_node_with_acme_still_reuse_direct() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping CF acme-not-dns01 regression test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-dns01-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.81".to_string(),
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

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(
        fields.cf_cert_mode, "reuse_direct",
        "有 acme 也不应误判 dns01:acme 是直连 HTTP-01 邮箱、非 DNS-01 凭据信号,应仍 reuse_direct"
    );
}

/// 重绑(rebind)路径同样:CF 节点无 acme 时 cf_cert_mode 落 reuse_direct,cf_enabled 派生 true。
#[tokio::test]
async fn test_rebind_cf_node_without_acme_derives_reuse_direct() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping rebind token-less CF reuse_direct test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = Uuid::new_v4();
    store
        .rebind_admin_installed_access_node(
            node_id,
            AdminAccessNodeInput {
                name: format!("cf-rebind-{}", node_id.simple()),
                public_host: "203.0.113.82".to_string(),
                public_port: 443,
                remark: String::new(),
                agent_token: format!("cf-rebind-token-{}", node_id.simple()),
                cert_domain: Some("gray.example.test".to_string()),
                acme_email: None,
                cf_enabled: true,
                cf_domain: Some("cf.example.test".to_string()),
                ip_direct_address: None,
            },
        )
        .await
        .unwrap();

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(
        fields.cf_cert_mode, "reuse_direct",
        "rebind 的 token-less CF 节点应派生 reuse_direct"
    );
    assert!(
        fields.cf_enabled,
        "rebind 后有 cf_domain 时 cf_enabled 应为 true"
    );
}

/// 端到端正路:token-less CF 节点(cf_domain + cert_domain,无 acme)自然落 reuse_direct,
/// 在其上建 CF 橙云入口(cdn_enabled),渲染应:
///  ① CF 入口证书锚到 cf 域名自己的 LE 路径(per-domain,不复用直连灰云 cert_domain、不锚 IP;
///     token-less 时 agent 经 CF :80 HTTP-01 给该 cf 域名签自己的真证书,实测穿 CF 能签出),
///  ② 订阅 server_name(SNI)== cf_domain(公布 CF 橙云域名、藏源站 IP)。
/// 替代旧的「把 CF 域名当 direct 绕」与「raw SQL 强改 cf_cert_mode」两条歪路。
#[tokio::test]
async fn test_tokenless_cf_entry_anchors_own_cf_cert_and_publishes_cf_sni() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping token-less CF entry anchoring test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 不给 acme → 自然派生 reuse_direct(不再需要 raw SQL 强改列)。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("cf-e2e-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.83".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("cf-e2e-token-{}", Uuid::new_v4().simple()),
            cert_domain: Some("gray.example.test".to_string()),
            acme_email: None,
            cf_enabled: true,
            cf_domain: Some("cf.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();

    // 先坐实自然落库即 reuse_direct(无任何 raw SQL 干预)。
    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(fields.cf_cert_mode, "reuse_direct");

    // 建一条 CF VLESS-WS-TLS 橙云入口(cdn_enabled),listen_host 给 IP、cdn_hostname=cf_domain。
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "cf-e2e-ws".to_string(),
            listen_host: "203.0.113.83".to_string(),
            listen_port: 443,
            protocol: "vless".to_string(),
            transport: "ws".to_string(),
            security: "tls".to_string(),
            server_name: String::new(),
            ws_path: "/cfe2e".to_string(),
            ws_host: "cf.example.test".to_string(),
            cdn_enabled: true,
            cdn_provider: "cloudflare".to_string(),
            cdn_hostname: "cf.example.test".to_string(),
            cdn_server: "cf.example.test".to_string(),
            enabled: true,
            sort_weight: 100,
            node_domain_id: None,
            vless_quantum_encryption: false,
        })
        .await
        .unwrap();

    let (server_name, inbound): (String, serde_json::Value) =
        sqlx::query_as("SELECT server_name, inbound_config FROM access_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    // ② 订阅 SNI/server == cf_domain(公布 CF 橙云域名)。
    assert_eq!(
        server_name, "cf.example.test",
        "CF 入口订阅 server_name 应公布 cf_domain"
    );

    // ① CF 入口证书锚到 cf 域名自己的 LE 路径(per-domain,不复用直连灰云 cert_domain、也非 IP)。
    let cert = inbound["certificate_file"].as_str().unwrap();
    assert_eq!(
        cert, "/etc/letsencrypt/live/cf.example.test/fullchain.pem",
        "token-less CF 入口证书必须锚到 cf 域名自己的 LE 路径(agent 经 CF :80 HTTP-01 自签该 cf 域名真证书)"
    );
    assert!(
        !cert.contains("gray.example.test"),
        "CF 入口不复用直连灰云证书(per-domain,每域名各自一张;用户定:不复用别的域名证书)"
    );
    assert!(!cert.contains("203.0.113"), "证书不应锚到 IP 路径");
    assert_eq!(
        inbound["key_file"].as_str().unwrap(),
        "/etc/letsencrypt/live/cf.example.test/privkey.pem"
    );
}
