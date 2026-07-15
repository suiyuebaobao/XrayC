/// 数据库测试分片 45。
// 本文件覆盖入口管理 CDN 公布地址、兑换码错误文案与支付配置 write-only 行为。
// 单内核改造后，原 AnyTLS/sing-box 入口用例已随协议与字段删除清理。
// 测试只使用示例域名和 RFC 文档地址，不访问远端服务器。
// 父级 tests 模块提供公共导入、数据库锁和 helper。
// 断言聚焦协议、TLS 默认值与网络模式等可静态校验的口径。
// PostgreSQL 用例缺少 DATABASE_URL 时只输出脱敏跳过原因。
// 不在这里记录真实 token、代理 URL 或私有凭据。
// 后续新增静态校验用例可优先补在这里，避免旧大分片继续膨胀。
// 本头部满足前十行中文注释约束。

#[test]
fn test_prepare_access_entry_cdn_uses_cdn_hostname_as_listen_host() {
    // 开启 CDN 后，物化的 listen_host 应为 CDN 域名（订阅据此公布，客户端连 CDN 边缘代理），
    // 而不是节点真实 listen_host；xray 入站仍绑 0.0.0.0，listen_host 只作对外公布地址。
    let prepared = crate::store::routing_access_entries::prepare_access_entry(
        &AdminAccessEntryInput {
            access_node_id: Uuid::new_v4(),
            name: "cdn-vless-ws".to_string(),
            listen_host: "192.0.2.1".to_string(),
            listen_port: 8443,
            protocol: "vless".to_string(),
            transport: "ws".to_string(),
            // CF 护栏要求过 CF 的入口必须 VLESS-WS-TLS;此处补 tls 以表示合法 CF 入口,
            // 测试聚焦点仍是"CDN 域名物化为对外 listen_host"。
            security: "tls".to_string(),
            server_name: "api.example.test".to_string(),
            ws_path: "/cdnws".to_string(),
            ws_host: "api.example.test".to_string(),
            cdn_enabled: true,
            cdn_provider: "cloudflare".to_string(),
            cdn_hostname: "api.example.test".to_string(),
            cdn_server: "api.example.test".to_string(),
            enabled: true,
            sort_weight: 100,
                    node_domain_id: None,
            vless_quantum_encryption: false,
        },
        "node.example.test".to_string(),
        crate::store::routing_entry_cert::NodeCertAnchor::default(),
        crate::store::routing_entry_selected_domain::SelectedEntryDomain::default(),
    )
    .unwrap();

    assert_eq!(prepared.listen_host, "api.example.test");
}

#[test]
fn test_prepare_access_entry_direct_domain_uses_cert_domain_as_listen_host() {
    // 域名直连线(选中 kind=direct 的 node_domain)物化:订阅对外公布地址(listen_host)必须 = 证书域名,
    // 不能停在节点 IP/public_host(开发方案.md §2.2.1「域名直连线 server=cert_domain、sni=cert_domain」)。
    // 这是 bug #2 的回归守卫:此前域名直连只物化 server_name(SNI),listen_host 漏物化成 IP,订阅 server 泄露真实 IP。
    let prepared = crate::store::routing_access_entries::prepare_access_entry(
        &AdminAccessEntryInput {
            access_node_id: Uuid::new_v4(),
            name: "direct-hy2".to_string(),
            // 管理员/前端默认带节点 IP 直连地址;域名直连应被选中域名覆盖成证书域名。
            listen_host: "192.0.2.50".to_string(),
            listen_port: 24005,
            protocol: "hysteria".to_string(),
            // HY2 客户端网络模式只支持 udp(内部映射成 hysteria 传输),不能直接传 "hysteria"。
            transport: "udp".to_string(),
            security: "tls".to_string(),
            server_name: "direct.example.test".to_string(),
            ws_path: String::new(),
            ws_host: String::new(),
            // 非 CDN:直连域名走非 CF 路径,listen_host 不应回退节点 IP。
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            enabled: true,
            sort_weight: 100,
            node_domain_id: Some(Uuid::new_v4()),
            vless_quantum_encryption: false,
        },
        "node.example.test".to_string(),
        crate::store::routing_entry_cert::NodeCertAnchor {
            cert_domain: Some("direct.example.test"),
            cf_domain: None,
        },
        crate::store::routing_entry_selected_domain::SelectedEntryDomain {
            kind: Some("direct"),
            domain: Some("direct.example.test"),
            has_domain: true,
        },
    )
    .unwrap();

    // 修复前:listen_host 停在 192.0.2.50(IP),断言失败(应红)。
    assert_eq!(prepared.listen_host, "direct.example.test");
    // SNI 仍 = 证书域名(本就正确,守卫不回归)。
    assert_eq!(prepared.server_name, "direct.example.test");
}

/// 构造一条最小 IP 直连入口输入(未选 node_domain、非 CDN、listen_host 由参数给),
/// 供 default_listen_host_for_entry 兜底口径用例复用,避免每个用例重复堆 20 个字段。
fn ip_direct_entry_input(listen_host: &str) -> AdminAccessEntryInput {
    AdminAccessEntryInput {
        access_node_id: Uuid::new_v4(),
        name: "ip-direct-vless".to_string(),
        listen_host: listen_host.to_string(),
        listen_port: 24010,
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
        // IP 直连:不选 node_domain。
        node_domain_id: None,
        vless_quantum_encryption: false,
    }
}

#[test]
fn test_default_listen_host_ip_direct_uses_ip_direct_address_not_public_host() {
    // BUG-F 回归守卫:IP 直连入口(未选域名、非 CDN、admin 未填 listen_host)且 public_host 是域名、
    // 节点单独配了 ip_direct_address(裸 IP)时,默认兜底必须用裸 IP,使订阅 server=ip_direct_address
    // (开发方案.md §196「IP直连线 server=ip_direct_address」),不能停在 public_host 的域名。
    let input = ip_direct_entry_input("");
    let default_host = crate::store::routing_access_entries::default_listen_host_for_entry(
        &input,
        // public_host 是域名(模拟节点用域名做 SSH/管理地址)。
        "manage.example.test",
        // 单独配的 IP 直连地址(裸 IP)。
        "192.0.2.132",
    );
    // 修复前:返回 public_host 域名 "manage.example.test",断言失败(应红)。
    assert_eq!(default_host, "192.0.2.132");
}

#[test]
fn test_default_listen_host_ip_direct_respects_explicit_listen_host() {
    // 对照:default_listen_host_for_entry 只产出"未填时用哪个地址";admin 显式填了 listen_host 时,
    // 由 prepare_access_entry 优先尊重该显式值,兜底不参与覆盖(与域名直连强制覆盖不同)。
    // 这里直接验证 prepare_access_entry:显式 listen_host=裸 IP …200 应原样保留,不被 ip_direct_address 改写。
    let input = ip_direct_entry_input("192.0.2.200");
    let prepared = crate::store::routing_access_entries::prepare_access_entry(
        &input,
        // 兜底给出 ip_direct_address(…132),但因 admin 显式填了 listen_host,不应被用上。
        crate::store::routing_access_entries::default_listen_host_for_entry(
            &input,
            "manage.example.test",
            "192.0.2.132",
        ),
        crate::store::routing_entry_cert::NodeCertAnchor::default(),
        crate::store::routing_entry_selected_domain::SelectedEntryDomain::default(),
    )
    .unwrap();
    // 显式 listen_host 原样保留。
    assert_eq!(prepared.listen_host, "192.0.2.200");
}

#[test]
fn test_default_listen_host_pure_ip_node_unchanged() {
    // 纯 IP 节点(public_host==IP、ip_direct_address 为空)行为不变:仍回落 public_host。
    let input = ip_direct_entry_input("");
    let with_empty_ip = crate::store::routing_access_entries::default_listen_host_for_entry(
        &input,
        "192.0.2.10",
        "",
    );
    assert_eq!(with_empty_ip, "192.0.2.10");
    // public_host==ip_direct_address(都是同一裸 IP)时,结果仍是该裸 IP(等价、无差异)。
    let with_same_ip = crate::store::routing_access_entries::default_listen_host_for_entry(
        &input,
        "192.0.2.10",
        "192.0.2.10",
    );
    assert_eq!(with_same_ip, "192.0.2.10");
}

#[test]
fn test_default_listen_host_cdn_and_domain_branches_keep_public_host() {
    // CF 入口(cdn_enabled)与域名直连入口(选了 node_domain)即便配了 ip_direct_address,
    // 兜底仍回落 public_host:这两支在 prepare_access_entry 里另有 cdn_hostname/cert_domain 物化覆盖,
    // 不走 IP 直连兜底,本函数不得抢先把它们改成裸 IP。
    let mut cdn_input = ip_direct_entry_input("");
    cdn_input.cdn_enabled = true;
    let cdn_host = crate::store::routing_access_entries::default_listen_host_for_entry(
        &cdn_input,
        "manage.example.test",
        "192.0.2.132",
    );
    assert_eq!(cdn_host, "manage.example.test");

    let mut domain_input = ip_direct_entry_input("");
    domain_input.node_domain_id = Some(Uuid::new_v4());
    let domain_host = crate::store::routing_access_entries::default_listen_host_for_entry(
        &domain_input,
        "manage.example.test",
        "192.0.2.132",
    );
    assert_eq!(domain_host, "manage.example.test");
}

#[test]
fn test_redeem_code_errors_surface_clean_user_messages() {
    // 兑换码错误透出给终端用户,消息必须干净、可读,
    // 不得带 "Agent 上报数据无效:" 这类内部前缀(回归守卫)。
    let cases = [
        (crate::DbError::RedeemCodeInvalid, "兑换码无效"),
        (crate::DbError::RedeemCodeNotFound, "兑换码不存在"),
        (crate::DbError::RedeemCodeUsed, "兑换码已使用"),
        (crate::DbError::RedeemCodeExpired, "兑换码已过期"),
    ];
    for (error, expected) in cases {
        let message = error.to_string();
        assert_eq!(message, expected);
        assert!(
            !message.contains("Agent"),
            "兑换码消息不应包含内部前缀: {message}"
        );
    }
}

#[test]
fn test_default_payment_setting_shape() {
    // 支付默认结构必须给出关闭态、空默认通道和三个 provider 对象，
    // 公开输出必须把四个 write-only 凭据字段脱敏成布尔 <field>_set，
    // 默认空凭据 → false，且对象里不再保留明文字符串字段。
    let default = crate::default_payment_setting();
    assert_eq!(default["enabled"], false);
    assert_eq!(default["default_channel"], "");

    let providers = &default["providers"];
    assert!(providers["alipay"].is_object());
    assert!(providers["epay"].is_object());
    assert!(providers["wechat"].is_object());

    let public = crate::public_payment_settings(crate::default_payment_setting());
    let public_providers = &public["providers"];

    // 四个 write-only 字段在公开输出里被替换成 <field>_set 布尔，且无明文。
    assert_eq!(public_providers["alipay"]["app_private_key_set"], false);
    assert!(public_providers["alipay"].get("app_private_key").is_none());

    assert_eq!(public_providers["epay"]["key_set"], false);
    assert!(public_providers["epay"].get("key").is_none());

    assert_eq!(public_providers["wechat"]["api_v3_key_set"], false);
    assert!(public_providers["wechat"].get("api_v3_key").is_none());

    assert_eq!(public_providers["wechat"]["private_key_set"], false);
    assert!(public_providers["wechat"].get("private_key").is_none());
}

#[test]
fn test_preserve_write_only_at() {
    // 通用 write-only 保留：新值该路径为空时用 current 旧值兜底，
    // 非空时按新值覆盖（write-only 字段不被空值清掉）。
    let paths: [&[&str]; 1] = [&["providers", "alipay", "app_private_key"]];

    // 新值为空 → 保留 current 里的 "OLD"。
    let value = json!({"providers": {"alipay": {"app_private_key": ""}}});
    let current = json!({"providers": {"alipay": {"app_private_key": "OLD"}}});
    let preserved = crate::store::json_util::preserve_write_only_at(value, &current, &paths);
    assert_eq!(
        preserved["providers"]["alipay"]["app_private_key"],
        json!("OLD")
    );

    // 新值非空 → 保留新值 "NEW"，不被旧值覆盖。
    let value = json!({"providers": {"alipay": {"app_private_key": "NEW"}}});
    let current = json!({"providers": {"alipay": {"app_private_key": "OLD"}}});
    let preserved = crate::store::json_util::preserve_write_only_at(value, &current, &paths);
    assert_eq!(
        preserved["providers"]["alipay"]["app_private_key"],
        json!("NEW")
    );
}

#[test]
fn test_preserve_write_only_smtp_password_behavior_unchanged() {
    // 守卫：SMTP 密码改用通用 preserve_write_only_at 后，三类历史行为必须不变。
    let current = json!({"email_verification": {"smtp_password": "OLD-SMTP"}});

    // 空新密码 → 保留旧密码。
    let value = json!({"email_verification": {"enabled": true, "smtp_password": ""}});
    let out = crate::store::json_util::preserve_write_only_smtp_password(value, &current);
    assert_eq!(out["email_verification"]["smtp_password"], json!("OLD-SMTP"));

    // 占位符 `***` → 等同空，保留旧密码。
    let value = json!({"email_verification": {"smtp_password": "***"}});
    let out = crate::store::json_util::preserve_write_only_smtp_password(value, &current);
    assert_eq!(out["email_verification"]["smtp_password"], json!("OLD-SMTP"));

    // 显式 clear_smtp_password=true → 清空密码，并移除控制字段。
    let value =
        json!({"email_verification": {"smtp_password": "", "clear_smtp_password": true}});
    let out = crate::store::json_util::preserve_write_only_smtp_password(value, &current);
    assert_eq!(out["email_verification"]["smtp_password"], json!(""));
    assert!(out["email_verification"].get("clear_smtp_password").is_none());

    // 新密码非空 → 覆盖旧密码。
    let value = json!({"email_verification": {"smtp_password": "NEW-SMTP"}});
    let out = crate::store::json_util::preserve_write_only_smtp_password(value, &current);
    assert_eq!(out["email_verification"]["smtp_password"], json!("NEW-SMTP"));
}

#[tokio::test]
async fn test_pg_payment_settings_roundtrip_and_write_only() {
    // 支付配置读写闭环 + write-only 保留（需要真实 PostgreSQL）：
    // 写入凭据 → 读回为原值；再用空值更新 → 凭据仍是旧值（不被清空）；
    // 公开输出里凭据脱敏成 _set:true 且不回显明文。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL payment settings roundtrip test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 首次写入两个凭据 + 开启支付。
    let updated = store
        .update_payment_settings_json(json!({
            "enabled": true,
            "providers": {
                "alipay": {"app_private_key": "P1"},
                "epay": {"key": "E1"}
            }
        }))
        .await
        .unwrap();
    assert_eq!(updated["enabled"], true);
    assert_eq!(updated["providers"]["alipay"]["app_private_key"], json!("P1"));
    assert_eq!(updated["providers"]["epay"]["key"], json!("E1"));

    // 读回应保持凭据原值。
    let read_back = store.payment_settings_json().await.unwrap();
    assert_eq!(read_back["providers"]["alipay"]["app_private_key"], json!("P1"));
    assert_eq!(read_back["providers"]["epay"]["key"], json!("E1"));

    // 用空值更新这两个凭据 → write-only 保留旧值，不被清空。
    store
        .update_payment_settings_json(json!({
            "providers": {
                "alipay": {"app_private_key": ""},
                "epay": {"key": ""}
            }
        }))
        .await
        .unwrap();
    let after_blank = store.payment_settings_json().await.unwrap();
    assert_eq!(after_blank["providers"]["alipay"]["app_private_key"], json!("P1"));
    assert_eq!(after_blank["providers"]["epay"]["key"], json!("E1"));

    // 公开输出：凭据脱敏成 _set:true，无明文字段。
    let public = crate::public_payment_settings(after_blank);
    assert_eq!(public["providers"]["alipay"]["app_private_key_set"], json!(true));
    assert!(public["providers"]["alipay"].get("app_private_key").is_none());
    assert_eq!(public["providers"]["epay"]["key_set"], json!(true));
    assert!(public["providers"]["epay"].get("key").is_none());
}
