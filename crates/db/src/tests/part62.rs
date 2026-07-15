/// 数据库测试分片 62。
// 本文件用纯函数单测钉死本机出口 host 的 CF 域名护栏(Bug ④ 回归测试)。
// 本机出口是中转节点同机自家出口、中转直接连它,绝不走 CF(橙云)边缘。
// 故本机出口 host 命中节点任一 CF 域名时必须被护栏拦下,而非放行后探测失败。
// 这些测试不连数据库,只校验纯函数 validate_local_exit_host_not_cf 的判定。
// 同时覆盖:host 留空走回环默认、IP 直连地址放行、direct 域名放行。
// 测试只用 RFC 2606/5737 示例域名/IP,不写任何真实服务器地址。
// 维护时继续保持中文注释与文件长度限制。
// 本头部满足前十行中文注释约束。
use crate::store::routing_local_exits::{validate_local_exit_host_not_cf, LOCAL_EXIT_DEFAULT_HOST};

#[test]
fn test_local_exit_host_cf_domain_is_rejected() {
    // host 命中节点 CF 域名时必须被拦:中转连 CF 域名会经 CF 边缘连不到本机出口端口。
    let cf_domains = vec!["cf.example.com".to_string()];
    let result = validate_local_exit_host_not_cf("cf.example.com", &cf_domains);
    assert!(
        result.is_err(),
        "本机出口 host 命中 CF 域名应被护栏拦下,实际放行了"
    );
}

#[test]
fn test_local_exit_host_cf_domain_rejected_case_insensitive() {
    // 域名大小写不敏感:大写/混合大小写的 CF 域名同样要被拦,不能靠改大小写绕过。
    let cf_domains = vec!["cf.example.com".to_string()];
    assert!(validate_local_exit_host_not_cf("CF.Example.COM", &cf_domains).is_err());
    // 前后空白也要归一后比对,避免" cf.example.com "绕过。
    assert!(validate_local_exit_host_not_cf("  cf.example.com  ", &cf_domains).is_err());
}

#[test]
fn test_local_exit_host_ip_direct_address_is_allowed() {
    // host = IP 直连地址(非 CF 域名)正常放行:这是 host 留空时的正确回退目标。
    let cf_domains = vec!["cf.example.com".to_string()];
    assert!(validate_local_exit_host_not_cf("192.0.2.10", &cf_domains).is_ok());
}

#[test]
fn test_local_exit_host_direct_domain_is_allowed() {
    // host = 直连域名(灰云)放行:本机出口允许域名直连,只禁 CF(橙云)。
    let cf_domains = vec!["cf.example.com".to_string()];
    assert!(validate_local_exit_host_not_cf("direct.example.com", &cf_domains).is_ok());
}

#[test]
fn test_local_exit_host_loopback_default_is_allowed() {
    // host 留空时回退的回环默认地址(127.0.0.1)永远放行:这是同机自家出口的正路。
    let cf_domains = vec!["cf.example.com".to_string()];
    assert!(validate_local_exit_host_not_cf(LOCAL_EXIT_DEFAULT_HOST, &cf_domains).is_ok());
    assert_eq!(LOCAL_EXIT_DEFAULT_HOST, "127.0.0.1");
}

#[test]
fn test_local_exit_host_no_cf_domains_allows_any_host() {
    // 节点没有任何 CF 域名时,护栏对任意 host 放行(纯 IP/纯直连节点不受影响)。
    let cf_domains: Vec<String> = Vec::new();
    assert!(validate_local_exit_host_not_cf("anything.example.com", &cf_domains).is_ok());
    assert!(validate_local_exit_host_not_cf("192.0.2.10", &cf_domains).is_ok());
}

#[tokio::test]
async fn test_pg_local_exit_rejects_cf_domain_host_when_database_url_is_set() {
    // 端到端钉死:真实 create_admin_local_exit_lines 路径里,host=节点 CF 域名时必须被护栏拦下(Bug ④)。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL local exit CF host guard test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 建一个带 CF(橙云)域名的节点:cf_domain 落库后由护栏读出比对。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "local-exit-cf-guard".to_string(),
            public_host: "cdn.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-cf-guard-token".to_string(),
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: Some("192.0.2.77".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    // host 显式填该节点的 CF 域名:本机出口绝不走 CF,必须被护栏拒。
    let err = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "cf-host-line".to_string(),
                    endpoint_name: "cf-host-line".to_string(),
                    region_code: "LOCAL".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "cdn.example.test".to_string(),
                    port: 39_001,
                    outbound_config: json!({
                        "username": "u",
                        "password": "p",
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect_err("CF 域名 host 必须被本机出口护栏拒绝");
    let message = format!("{err}");
    assert!(
        message.contains("CF") && message.contains("本机出口"),
        "拒绝原因应说明本机出口不能用 CF 域名,实际: {message}"
    );

    // 同节点用 IP 直连地址做 host 应正常创建(护栏只拦 CF 域名,不误伤 IP 直连)。
    store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "ip-host-line".to_string(),
                    endpoint_name: "ip-host-line".to_string(),
                    region_code: "LOCAL".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "192.0.2.77".to_string(),
                    port: 39_002,
                    outbound_config: json!({
                        "username": "u",
                        "password": "p",
                    }),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .expect("IP 直连地址做 host 应正常创建");
}
