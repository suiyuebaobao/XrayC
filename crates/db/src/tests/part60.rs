// 数据库测试分片 60:多域名证书状态回报刷新 node_domains.cert_status(修 #60)。
// 规则:agent 心跳上报的每条 tls_certificate 按 domain 匹配本节点 node_domains 行,
// 把该行 cert_status 刷成上报状态;不在上报里的域名 cert_status 保持原值(不清成 unknown)。
// 真实 PostgreSQL,缺 DATABASE_URL 跳过;只用 example.test 占位,断言以 store 返回为准。
// 父 tests 模块 include! 引入,所有测试串行(--test-threads=1),pg_test_guard 串行化共享库。
// 状态映射:上报 status(valid/invalid/missing/...)直接落 cert_status,与读模型/前端展示一致。
// 不在此保存任何真实服务器地址或凭据。
// 单文件低于 550 行。
// 维护时优先保持本分片只覆盖证书状态回报相关行为。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_record_agent_tls_status_updates_node_domains_cert_status() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 建一个有两个 node_domains 的节点;再加第三个不出现在上报里的域名验证不被动。
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "md-certstatus".into(),
            public_host: "md-certstatus.example.test".into(),
            public_port: 443,
            agent_token: "md-certstatus-token".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    for domain in [
        "domain1.example.test",
        "domain2.example.test",
        "untouched.example.test",
    ] {
        store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: domain.into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    // 初始三行 cert_status 均为默认 unknown。
    let before = store.list_node_domains(node_id).await.unwrap();
    assert!(
        before.iter().all(|d| d.cert_status == "unknown"),
        "新增域名 cert_status 初始应为 unknown,实际: {before:?}"
    );

    // 喂一份心跳:domain1 valid(days=80),domain2 invalid;untouched 不在上报里。
    let certificates = vec![
        AgentTlsCertificateReport {
            domain: "domain1.example.test".into(),
            status: "valid".into(),
            not_before: Some("2026-06-01T00:00:00Z".into()),
            not_after: Some("2026-08-30T00:00:00Z".into()),
            days_remaining: Some(80),
            error_summary: String::new(),
        },
        AgentTlsCertificateReport {
            domain: "domain2.example.test".into(),
            status: "invalid".into(),
            not_before: None,
            not_after: None,
            days_remaining: None,
            error_summary: "certificate file is missing".into(),
        },
    ];
    store
        .record_agent_tls_status(node_id, certificates, None)
        .await
        .unwrap();

    // 断言:两行各自刷成对应状态;不在上报里的域名保持 unknown。
    let after = store.list_node_domains(node_id).await.unwrap();
    let status_of = |domain: &str| {
        after
            .iter()
            .find(|d| d.domain == domain)
            .map(|d| d.cert_status.as_str())
            .unwrap_or("<missing>")
            .to_string()
    };
    assert_eq!(status_of("domain1.example.test"), "valid");
    assert_eq!(status_of("domain2.example.test"), "invalid");
    assert_eq!(
        status_of("untouched.example.test"),
        "unknown",
        "未在上报里的域名 cert_status 不应被改动"
    );
}

// per-domain 渲染层 cert-readiness 兜底(纯内存,不需 DATABASE_URL):
// CF 入口锚定的 cf 域名证书未就绪(cert_status=missing),即便节点另有 valid 直连(sslip)证书,
// 也绝不复用其它域名证书(用户定:每域名各自一张)。控制面渲染须 Skip 该单条 inbound、
// 等 agent 给该 cf 域名签好自己的证书后自然渲染,Skip 不拖垮整份 apply。
#[test]
fn test_cf_inbound_skipped_when_cf_cert_missing_no_reuse() {
    use crate::xray_render_cert::{resolve_tls_cert_paths, TlsCertResolution};
    use xrayc_core::NodeDomainView;

    let cf_domain = "api.coordcode.test";
    let direct_domain = "86-53-110-123.sslip.test";
    let store = xrayc_core::MemoryStore::seeded();
    let (data, node_id) = store.write(|data| {
        let node_id = *data.access_nodes.keys().next().unwrap();
        // 节点域名读模型:CF 域名 cert_status=missing(没签),直连域名 cert_status=valid。
        let node = data.access_nodes.get_mut(&node_id).unwrap();
        node.domains = vec![
            NodeDomainView {
                id: uuid("00000000-0000-0000-0000-0000000000d1"),
                domain: direct_domain.to_string(),
                kind: "direct".to_string(),
                is_primary: true,
                cert_status: "valid".to_string(),
            },
            NodeDomainView {
                id: uuid("00000000-0000-0000-0000-0000000000c1"),
                domain: cf_domain.to_string(),
                kind: "cf".to_string(),
                is_primary: true,
                cert_status: "missing".to_string(),
            },
        ];
        // 唯一入口物化成 CF VLESS-WS-TLS:server_name=cf_domain,证书路径(写侧锚定)落到未签的 cf_domain LE 路径。
        let line = data.access_lines.values_mut().next().unwrap();
        line.protocol = "vless".to_string();
        line.transport = "ws".to_string();
        line.flow.clear();
        line.udp_enabled = false;
        line.listen_host = cf_domain.to_string();
        line.server_name = cf_domain.to_string();
        line.xhttp_host = cf_domain.to_string();
        line.inbound_config = json!({
            "security": "tls",
            "server_name": cf_domain,
            "certificate_file": format!("/etc/letsencrypt/live/{cf_domain}/fullchain.pem"),
            "key_file": format!("/etc/letsencrypt/live/{cf_domain}/privkey.pem"),
        });
        (data.clone(), node_id)
    });

    let line = data
        .access_lines
        .values()
        .find(|line| line.server_name == cf_domain)
        .expect("应能取回设好的 CF 线");
    let node_domains = data.access_nodes[&node_id].domains.as_slice();
    // 语义反转:cf 域名证书 missing 时不再复用 valid 直连证书,应 Skip 该单条 inbound(per-domain,不复用)。
    assert!(
        matches!(
            resolve_tls_cert_paths(line, node_domains),
            TlsCertResolution::Skip
        ),
        "CF 证书 missing 时必须 Skip,绝不复用 {direct_domain} 等其它域名证书"
    );
}
