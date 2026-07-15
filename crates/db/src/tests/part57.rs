// 数据库测试片段五十七:多域名 Phase 2 护栏按选中域名 + socks/http 凭据自动生成。
// A:socks/http 本机出口空配置自动补 username+password,否则节点渲染不出本机服务。
// B:本机出口护栏按选中 node_domain 的 kind(direct 放 Trojan,cf 只放 ws/grpc/xhttp,无选回退节点级)。
// C:入口护栏按选中 node_domain(cf 域名放 VLESS/Trojan+ws/grpc/xhttp+tls,direct 放 Trojan,None 回退)。
// 真实 PostgreSQL 集成,缺 DATABASE_URL 时跳过且只输出脱敏原因。
// 只使用 example.test 占位与示例端口/RFC5737 IP,不写真实服务器/凭据。
// 断言以 store 返回 / 真实库读回为准,符合跨层对齐红线(写侧为准)。
// 父级 tests 模块通过 include! 引入本文件,共享 pg_test_guard 串行化。
// 新增测试优先保持单文件低于源码长度上限。
// 本头部满足前十行中文注释约束。

    // 建一个测试节点,可选直连/CF 域名;免证书路径默认带 IP 直连地址。
    async fn create_md_guard_node(
        store: &PgStore,
        slug: &str,
        cert_domain: Option<&str>,
        cf_domain: Option<&str>,
    ) -> Uuid {
        store
            .create_admin_access_node(AdminAccessNodeInput {
                name: format!("md-guard-{slug}"),
                public_host: format!("{slug}.example.test"),
                public_port: 443,
                agent_token: format!("md-guard-token-{slug}"),
                cert_domain: cert_domain.map(|value| value.to_string()),
                cf_domain: cf_domain.map(|value| value.to_string()),
                ip_direct_address: Some("192.0.2.60".to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    // 构造一条本机出口输入,支持指定选中域名 id(None=不选)。
    fn md_local_exit_line(
        outbound_type: &str,
        outbound_config: serde_json::Value,
        node_domain_id: Option<Uuid>,
    ) -> AdminLocalExitLineInput {
        AdminLocalExitLineInput {
            resource_name: format!("md-le-{outbound_type}"),
            endpoint_name: format!("md-le-{outbound_type}-ep"),
            region_code: "TEST".to_string(),
            outbound_type: outbound_type.to_string(),
            network_mode: if outbound_type == "hysteria" {
                "udp".to_string()
            } else {
                "tcp".to_string()
            },
            host: "md-le.example.test".to_string(),
            port: 37_001,
            outbound_config,
            stream_config: json!({}),
            probe_config: json!({}),
            enabled: true,
            node_domain_id,
        }
    }

    // 读回某 exit_endpoint 的 outbound_config,验证写侧落库的真实凭据。
    async fn read_endpoint_outbound_config(store: &PgStore, endpoint_id: Uuid) -> serde_json::Value {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT outbound_config FROM exit_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
    }

    /// A:socks 空配置建本机出口 → 返回的 outbound_config 含非空 username+password。
    #[tokio::test]
    async fn test_local_exit_socks_auto_generates_credentials() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping socks credential auto-gen test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "socks-auto", None, None).await;
        // 完全不带 username/password 的 socks 出口:护栏免证书放行,fill 应自动补齐凭据。
        let result = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("socks", json!({}), None)],
                },
            )
            .await
            .expect("socks 空配置应可建本机出口并自动生成凭据");
        let endpoint_id: Uuid = result["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .expect("返回应含 exit_endpoint_id");
        let config = read_endpoint_outbound_config(&store, endpoint_id).await;
        let username = config.get("username").and_then(|v| v.as_str()).unwrap_or("");
        let password = config.get("password").and_then(|v| v.as_str()).unwrap_or("");
        assert!(!username.is_empty(), "socks 自动生成 username 应非空: {config}");
        assert!(!password.is_empty(), "socks 自动生成 password 应非空: {config}");
    }

    /// A:http 已给凭据时保留原值,不覆盖。
    #[tokio::test]
    async fn test_local_exit_http_keeps_existing_credentials() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping http credential keep test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "http-keep", None, None).await;
        let result = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line(
                        "http",
                        json!({ "username": "keep-user", "password": "keep-pass" }),
                        None,
                    )],
                },
            )
            .await
            .expect("http 带凭据应建出本机出口");
        let endpoint_id: Uuid = result["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .unwrap();
        let config = read_endpoint_outbound_config(&store, endpoint_id).await;
        assert_eq!(config["username"], json!("keep-user"), "已有 username 应保留");
        assert_eq!(config["password"], json!("keep-pass"), "已有 password 应保留");
    }

    /// B:本机出口选 direct 域名 → Trojan 放行;选 cf 域名 → Trojan 被拦(CF 只放 ws/grpc/xhttp)。
    #[tokio::test]
    async fn test_local_exit_guard_keys_off_selected_node_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit selected-domain guard test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 节点同时有 direct 与 cf 域名(回填生成两条 node_domains)。
        let node = create_md_guard_node(
            &store,
            "le-sel",
            Some("direct-sel.example.test"),
            Some("cf-sel.example.test"),
        )
        .await;
        let domains = store.list_node_domains(node).await.unwrap();
        let direct_id = domains.iter().find(|d| d.kind == "direct").unwrap().id;
        let cf_id = domains.iter().find(|d| d.kind == "cf").unwrap().id;

        let trojan_config = json!({
            "password": "md-trojan-pass",
            "security": "tls",
            "server_name": "direct-sel.example.test",
            "certificate_file": "/etc/letsencrypt/live/direct-sel.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/direct-sel.example.test/privkey.pem",
        });

        // 选 direct 域名 → Trojan 放行。
        let allowed = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("trojan", trojan_config.clone(), Some(direct_id))],
                },
            )
            .await;
        assert!(allowed.is_ok(), "选 direct 域名应放行 Trojan 本机出口: {allowed:?}");

        // 选 cf 域名 → Trojan 被拦(CF 只放 ws/grpc/xhttp+tls)。
        let blocked = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("trojan", trojan_config, Some(cf_id))],
                },
            )
            .await;
        assert!(blocked.is_err(), "选 cf 域名应拦下 Trojan 本机出口(CF 只放 ws/grpc/xhttp)");
    }

    /// B:不带 node_domain_id 时回退现有行为(节点有 direct 域名即放行要证书协议)。
    #[tokio::test]
    async fn test_local_exit_guard_falls_back_without_node_domain_id() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit fallback guard test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let trojan_config = json!({
            "password": "md-fb-pass",
            "security": "tls",
            "server_name": "fb.example.test",
            "certificate_file": "/etc/letsencrypt/live/fb.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/fb.example.test/privkey.pem",
        });

        // 有 direct 域名、不选域名 → 回退节点级,Trojan 放行。
        let with_domain = create_md_guard_node(&store, "fb-yes", Some("fb.example.test"), None).await;
        let allowed = store
            .create_admin_local_exit_lines(
                with_domain,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("trojan", trojan_config.clone(), None)],
                },
            )
            .await;
        assert!(allowed.is_ok(), "有 direct 域名不选时应回退放行 Trojan: {allowed:?}");

        // 无任何域名、不选域名 → 回退节点级,Trojan 被拦。
        let no_domain = create_md_guard_node(&store, "fb-no", None, None).await;
        let blocked = store
            .create_admin_local_exit_lines(
                no_domain,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("trojan", trojan_config, None)],
                },
            )
            .await;
        assert!(blocked.is_err(), "无域名不选时应回退拦下 Trojan");
    }

    /// C:入口选 cf 域名 → VLESS+ws+tls 放行;选 direct 域名 → Trojan 放行;选 cf → Trojan+tcp 被拦。
    #[tokio::test]
    async fn test_access_entry_guard_keys_off_selected_node_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping entry selected-domain guard test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(
            &store,
            "entry-sel",
            Some("direct-entry.example.test"),
            Some("cf-entry.example.test"),
        )
        .await;
        let domains = store.list_node_domains(node).await.unwrap();
        let direct_id = domains.iter().find(|d| d.kind == "direct").unwrap().id;
        let cf_id = domains.iter().find(|d| d.kind == "cf").unwrap().id;

        // 选 cf 域名 + VLESS/ws/tls → 放行(不用开 cdn_enabled,选中域名 kind=cf 即按 CF 护栏)。
        // 端口用 2053(CF 支持的 HTTPS 端口),避免被 CF 入口端口护栏拦下。
        let cf_vless = store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-entry-cf-vless",
                2053,
                "vless",
                "ws",
                "tls",
                Some(cf_id),
            ))
            .await;
        assert!(cf_vless.is_ok(), "选 cf 域名的 VLESS/ws/tls 入口应放行: {cf_vless:?}");

        // 选 cf 域名 + Trojan/tcp → 被拦(CF 只放 ws/grpc/xhttp)。
        let cf_trojan = store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-entry-cf-trojan",
                28_802,
                "trojan",
                "tcp",
                "tls",
                Some(cf_id),
            ))
            .await;
        assert!(cf_trojan.is_err(), "选 cf 域名的 Trojan/tcp 入口应被拦下");

        // 选 direct 域名 + Trojan/tcp/tls → 放行(direct 域名可签证书)。
        let direct_trojan = store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-entry-direct-trojan",
                28_803,
                "trojan",
                "tcp",
                "tls",
                Some(direct_id),
            ))
            .await;
        assert!(direct_trojan.is_ok(), "选 direct 域名的 Trojan 入口应放行: {direct_trojan:?}");
    }

    /// 缺口3:橙云(CF)入口端口必须 ∈ CF 支持的 HTTPS 端口集合;非 CF 入口任意端口不受限。
    #[tokio::test]
    async fn test_cf_entry_listen_port_must_be_cf_supported() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping cf entry port guard test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(
            &store,
            "cf-port",
            Some("direct-port.example.test"),
            Some("cf-port.example.test"),
        )
        .await;
        let domains = store.list_node_domains(node).await.unwrap();
        let direct_id = domains.iter().find(|d| d.kind == "direct").unwrap().id;
        let cf_id = domains.iter().find(|d| d.kind == "cf").unwrap().id;

        // 选 cf 域名 + 非 CF 支持端口(22443)→ 被拦,错误文案点明 CF 支持端口集合。
        let bad = store
            .create_admin_access_entry(md_entry_input(
                node,
                "cf-bad-port",
                22_443,
                "vless",
                "ws",
                "tls",
                Some(cf_id),
            ))
            .await
            .expect_err("CF 入口非 CF 支持端口应被拦下");
        let message = bad.to_string();
        assert!(
            message.contains("橙云") && message.contains("8443"),
            "应报 CF 入口端口必须是 CF 支持的 HTTPS 端口,实际: {message}"
        );

        // 选 cf 域名 + CF 支持端口(8443)→ 放行。
        let good = store
            .create_admin_access_entry(md_entry_input(
                node,
                "cf-good-port",
                8443,
                "vless",
                "ws",
                "tls",
                Some(cf_id),
            ))
            .await;
        assert!(good.is_ok(), "CF 入口用 8443 应放行: {good:?}");

        // 选 direct 域名(非 CF 入口)+ 任意端口(22443)→ 放行,不受 CF 端口限制(向后兼容)。
        let direct_any = store
            .create_admin_access_entry(md_entry_input(
                node,
                "direct-any-port",
                22_443,
                "trojan",
                "tcp",
                "tls",
                Some(direct_id),
            ))
            .await;
        assert!(
            direct_any.is_ok(),
            "direct 入口任意端口应放行(不受 CF 端口限制): {direct_any:?}"
        );
    }

    // 构造一条入口输入,支持指定选中域名 id(None=不选,回退节点单域名)。
    fn md_entry_input(
        node_id: Uuid,
        name: &str,
        port: u16,
        protocol: &str,
        transport: &str,
        security: &str,
        node_domain_id: Option<Uuid>,
    ) -> AdminAccessEntryInput {
        AdminAccessEntryInput {
            access_node_id: node_id,
            name: name.to_string(),
            listen_host: String::new(),
            listen_port: port,
            protocol: protocol.to_string(),
            transport: transport.to_string(),
            security: security.to_string(),
            server_name: String::new(),
            ws_path: if transport == "ws" { "/md".to_string() } else { String::new() },
            ws_host: String::new(),
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            enabled: true,
            sort_weight: 0,
            node_domain_id,
            vless_quantum_encryption: false,
        }
    }
