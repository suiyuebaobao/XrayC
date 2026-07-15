// 数据库测试拆分片段五十五。
// 本文件覆盖本机出口「要证书」协议的节点域名直连地址护栏。
// 规则:Trojan/HY2/VLESS-TLS 本机出口要求所属节点 cert_domain 非空,否则拒绝。
// 免证书协议(SOCKS5/HTTP/Shadowsocks/VLESS-Reality/none)不受此限。
// 真实 PostgreSQL 集成,缺 DATABASE_URL 时跳过且只输出脱敏原因。
// 只使用 example.test 占位与示例端口,不写真实服务器/凭据。
// 断言以 create_admin_local_exit_lines 的返回为准,符合跨层对齐红线。
// 父级 tests 模块通过 include! 引入本文件。
// 新增测试优先保持单文件低于源码长度上限。
// 本头部满足前十行中文注释约束。

    // 构造一条最小本机出口线路输入,协议与配置由调用方给定。
    fn local_exit_line_for_test(
        outbound_type: &str,
        outbound_config: serde_json::Value,
    ) -> AdminLocalExitLineInput {
        AdminLocalExitLineInput {
            resource_name: format!("cert-guard-{outbound_type}"),
            endpoint_name: format!("cert-guard-{outbound_type}-ep"),
            region_code: "TEST".to_string(),
            outbound_type: outbound_type.to_string(),
            network_mode: if outbound_type == "hysteria" {
                "udp".to_string()
            } else {
                "tcp".to_string()
            },
            host: "cert-guard.example.test".to_string(),
            port: 36_001,
            outbound_config,
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
            node_domain_id: None,
        }
    }

    // 建一个测试节点,cert_domain 由参数决定有无(无域名时只给 IP 直连地址)。
    async fn create_cert_guard_node(store: &PgStore, slug: &str, cert_domain: Option<&str>) -> Uuid {
        store
            .create_admin_access_node(AdminAccessNodeInput {
                name: format!("cert-guard-{slug}"),
                public_host: format!("{slug}.example.test"),
                public_port: 443,
                agent_token: format!("cert-guard-token-{slug}"),
                cert_domain: cert_domain.map(|value| value.to_string()),
                ip_direct_address: Some("192.0.2.50".to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_local_exit_trojan_requires_node_cert_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit trojan cert-domain guard test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // Trojan 本机出口的 outbound_config 带齐证书文件,确保不会被既有证书文件校验拦下;
        // 真正应当拦截的是「节点没有域名直连地址(cert_domain 为空)」。
        let trojan_config = json!({
            "password": "trojan-cert-guard-pass",
            "security": "tls",
            "server_name": "cert-guard.example.test",
            "certificate_file": "/etc/letsencrypt/live/cert-guard.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/cert-guard.example.test/privkey.pem",
        });

        // 无 cert_domain 的节点建 Trojan 本机出口 → 必须报错。
        let no_cert_node = create_cert_guard_node(&store, "trojan-nocert", None).await;
        let blocked = store
            .create_admin_local_exit_lines(
                no_cert_node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test("trojan", trojan_config.clone())],
                },
            )
            .await;
        assert!(
            blocked.is_err(),
            "无域名直连地址(cert_domain 空)的节点不应建出 Trojan 本机出口"
        );

        // 有 cert_domain 的节点建 Trojan 本机出口 → 应成功。
        let cert_node =
            create_cert_guard_node(&store, "trojan-cert", Some("cert-guard.example.test")).await;
        let allowed = store
            .create_admin_local_exit_lines(
                cert_node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test("trojan", trojan_config)],
                },
            )
            .await;
        assert!(
            allowed.is_ok(),
            "配了域名直连地址的节点应能建出 Trojan 本机出口: {allowed:?}"
        );
    }

    #[tokio::test]
    async fn test_local_exit_socks_http_allowed_without_cert() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit no-cert allow test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 无 cert_domain 节点上的免证书协议(SOCKS5/HTTP/Shadowsocks)都应放行。
        let node = create_cert_guard_node(&store, "nocert-allow", None).await;

        let socks = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test(
                        "socks",
                        json!({ "username": "u", "password": "p" }),
                    )],
                },
            )
            .await;
        assert!(socks.is_ok(), "SOCKS5 本机出口应免证书放行: {socks:?}");

        let http = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test(
                        "http",
                        json!({ "username": "u", "password": "p" }),
                    )],
                },
            )
            .await;
        assert!(http.is_ok(), "HTTP 本机出口应免证书放行: {http:?}");

        let shadowsocks = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test(
                        "shadowsocks",
                        json!({ "method": "2022-blake3-aes-128-gcm" }),
                    )],
                },
            )
            .await;
        assert!(
            shadowsocks.is_ok(),
            "Shadowsocks 本机出口应免证书放行: {shadowsocks:?}"
        );
    }

    #[tokio::test]
    async fn test_local_exit_blank_host_defaults_to_loopback() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit blank-host default test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 本机出口被同节点中转就地消费,host 留空时必须默认 127.0.0.1(本地、不过 CF),
        // 而不是节点 public_host(橙云节点的 public_host 是 CF 代理域名,中转连它会被 CF 拦下)。
        // 节点 public_host 刻意用 CF 风格占位域名,断言落库 host 不等于它、且为回环地址。
        let node = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "blank-host-default".to_string(),
                public_host: "orange-cloud.example.test".to_string(),
                public_port: 443,
                agent_token: "blank-host-default-token".to_string(),
                ip_direct_address: Some("192.0.2.60".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        // host 留空建一条免证书 SOCKS5 本机出口。
        let mut line = local_exit_line_for_test("socks", json!({ "username": "u", "password": "p" }));
        line.host = String::new();
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput { lines: vec![line] },
            )
            .await
            .expect("host 留空的免证书本机出口应能创建");

        let created_host = created["created_lines"][0]["host"]
            .as_str()
            .expect("返回里应带 host 字段");
        assert_eq!(
            created_host, "127.0.0.1",
            "host 留空应默认回环地址 127.0.0.1,实际: {created_host}"
        );
        assert_ne!(
            created_host, "orange-cloud.example.test",
            "host 留空绝不能回落到节点 public_host(橙云域名会被 CF 拦)"
        );
    }

    #[tokio::test]
    async fn test_local_exit_cert_guard_fires_before_protocol_field_validation() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit cert-guard ordering test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // "提前判断" 红线:无 cert_domain 节点 + 配置不完整的 Trojan(连 password 都没填)。
        // 必须先报「节点缺域名直连地址」,而不是先报「Trojan password 不能为空」这种误导字段错误;
        // 否则绕过前端直连 API 的管理员会被一路引着补字段,最后才发现该节点根本不能用 Trojan。
        let node = create_cert_guard_node(&store, "trojan-order", None).await;
        let err = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![local_exit_line_for_test("trojan", json!({}))],
                },
            )
            .await
            .expect_err("无证书节点 + 不完整 Trojan 配置应被节点证书护栏先拦下");
        let message = err.to_string();
        assert!(
            message.contains("域名直连地址"),
            "应优先报节点缺域名直连地址(提前判断),实际报错: {message}"
        );
    }
