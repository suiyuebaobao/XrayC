// 数据库测试分片 70(VLESS 量子加密:生成后量子密钥串 + Reality 不叠量子)。
// 覆盖两条口径(详见 docs/superpowers/specs/2026-06-28-vless-quantum-encryption-and-sni-merge-design.md):
// ① 非 Reality 的 VLESS(普通 none)+ vless_quantum_encryption=true → inbound_config 落库
//    vless_decryption(mlkem768x25519plus.native.600s.<私钥>)+ vless_encryption(...0rtt.<公钥>)。
// ② Reality + vless_quantum_encryption=true → 不生成量子串(Reality 走后量子 dest、不叠;评估结论)。
// 测试只用 RFC 5737 文档 IP 与隔离 PostgreSQL,不访问远端节点;缺 DATABASE_URL 只脱敏跳过、不算通过。
// 通过 create_admin_access_entry 公开写入口,读回 access_entries.inbound_config 落库为唯一真相。
// 节点/入口创建复用 PgStore 公开写入口,不造不存在的 helper。
// 本头部满足前十行中文注释约束。

/// 构造一条 VLESS 入口输入(IP 直连),security 与量子开关由调用方给。
fn vless_quantum_entry_input(
    node_id: Uuid,
    name: &str,
    security: &str,
    quantum: bool,
) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: String::new(),
        listen_port: 28_443,
        protocol: "vless".to_string(),
        transport: "tcp".to_string(),
        security: security.to_string(),
        server_name: if security == "reality" {
            "www.cloudflare.com".to_string()
        } else {
            String::new()
        },
        ws_path: String::new(),
        ws_host: String::new(),
        cdn_enabled: false,
        cdn_provider: String::new(),
        cdn_hostname: String::new(),
        cdn_server: String::new(),
        enabled: true,
        sort_weight: 100,
        node_domain_id: None,
        vless_quantum_encryption: quantum,
    }
}

/// 构造一条 Shadowsocks 入口输入(IP 直连免证书)。
fn shadowsocks_entry_input(node_id: Uuid, name: &str) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: node_id,
        name: name.to_string(),
        listen_host: String::new(),
        listen_port: 28_443,
        protocol: "shadowsocks".to_string(),
        transport: "tcp".to_string(),
        security: String::new(),
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

async fn quantum_test_node(store: &PgStore) -> Uuid {
    store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("qe-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.90".to_string(),
            public_port: 443,
            agent_token: format!("qe-token-{}", Uuid::new_v4().simple()),
            ip_direct_address: Some("203.0.113.90".to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
}

async fn entry_inbound_config(store: &PgStore, name: &str) -> Value {
    sqlx::query_scalar("SELECT inbound_config FROM access_entries WHERE name = $1 LIMIT 1")
        .bind(name)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn test_vless_quantum_encryption_generates_keys() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping vless quantum keygen test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = quantum_test_node(&store).await;

    let name = format!("qe-none-on-{}", Uuid::new_v4().simple());
    store
        .create_admin_access_entry(vless_quantum_entry_input(node_id, &name, "", true))
        .await
        .unwrap();

    let inbound = entry_inbound_config(&store, &name).await;
    let dec = inbound
        .get("vless_decryption")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let enc = inbound
        .get("vless_encryption")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        dec.starts_with("mlkem768x25519plus.native.600s."),
        "服务端 decryption 串前缀应为 mlkem768x25519plus.native.600s.,实际: {dec}"
    );
    assert!(
        enc.starts_with("mlkem768x25519plus.native.0rtt."),
        "客户端 encryption 串前缀应为 mlkem768x25519plus.native.0rtt.,实际: {enc}"
    );
    assert_eq!(
        inbound.get("vless_quantum_encryption").and_then(Value::as_bool),
        Some(true),
        "量子标记应落库为 true"
    );
}

#[tokio::test]
async fn test_vless_quantum_not_applied_to_reality() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping vless quantum reality guard test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = quantum_test_node(&store).await;

    // Reality + 量子开 → Reality 走后量子 dest,不叠量子串。
    let name = format!("qe-reality-on-{}", Uuid::new_v4().simple());
    store
        .create_admin_access_entry(vless_quantum_entry_input(node_id, &name, "reality", true))
        .await
        .unwrap();

    let inbound = entry_inbound_config(&store, &name).await;
    assert!(
        inbound.get("vless_decryption").is_none(),
        "Reality 入口不应生成量子 decryption 串,实际: {inbound}"
    );
    assert_eq!(
        inbound.get("vless_quantum_encryption").and_then(Value::as_bool),
        Some(false),
        "Reality 入口量子标记应被清成 false"
    );
}

/// 回归测试(1ebb430 量子提交引入):建 Shadowsocks 入口后,inbound_config 必须注入
/// 2022 method + server_password。量子标记若被无条件写入会让 inbound_config 非空、
/// 使 routing_entry_cert.rs 的 is_empty() 默认注入门跳过 SS 凭据,导致订阅过滤掉 SS 线路(422)。
#[tokio::test]
async fn test_shadowsocks_entry_injects_2022_credentials() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping shadowsocks entry credential test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let node_id = quantum_test_node(&store).await;

    let name = format!("ss-entry-{}", Uuid::new_v4().simple());
    store
        .create_admin_access_entry(shadowsocks_entry_input(node_id, &name))
        .await
        .unwrap();

    let inbound = entry_inbound_config(&store, &name).await;
    let method = inbound
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = inbound
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(
        method, "2022-blake3-aes-128-gcm",
        "SS 入口 inbound_config 应注入 2022 method,实际: {inbound}"
    );
    assert!(
        !password.is_empty(),
        "SS 入口 inbound_config 应注入 server_password(PSK),实际: {inbound}"
    );
}
