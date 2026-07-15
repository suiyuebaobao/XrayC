/// 数据库测试分片 11。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    #[test]
    fn test_heartbeat_config_supports_vless_reality_endpoint() {
        let store = MemoryStore::seeded();
        let (data, node_id, vless_id) = store.write(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let line_id = *data.access_lines.keys().next().unwrap();
            let pool_id = data.access_lines.get(&line_id).unwrap().exit_pool_id;
            let user_id = *data.users.keys().next().unwrap();
            let vless_id = uuid("00000000-0000-0000-0000-000000009104");
            let vless_endpoint = ExitEndpoint {
                id: vless_id,
                resource_name: "third-party-vless".to_string(),
                ownership: "third_party".to_string(),
                owner_access_node_id: None,
                outbound_type: EndpointType::Vless,
                host: "vless.example.test".to_string(),
                port: 443,
                outbound_config: json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "reality",
                    "flow": "xtls-rprx-vision",
                    "server_name": "www.example.com",
                    "public_key": "reality-public-key",
                    "short_id": "abcd1234",
                    "fingerprint": "chrome"
                }),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            data.exit_pools.get_mut(&pool_id).unwrap().members = vec![vless_endpoint];
            data.user_exit_assignments = vec![UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: vless_id,
                assigned_at: Utc::now(),
                failover_reason: "initial".to_string(),
            }];
            (data.clone(), node_id, vless_id)
        });

        let heartbeat = heartbeat_json(&data, Some(node_id), None);
        let exit_tag = exit_endpoint_tag(vless_id);
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "vless"
        );
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["security"],
            "reality"
        );
        assert_eq!(
            heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_tag
        );

        let config: XrayAccessConfig =
            serde_json::from_value(heartbeat["config"].clone()).expect("config should deserialize");
        let rendered =
            xrayc_xray_config::compile_xray_config(&config).expect("config should compile");
        assert_eq!(
            rendered["outbounds"][0]["streamSettings"]["security"],
            "reality"
        );
        assert_eq!(
            rendered["outbounds"][0]["streamSettings"]["realitySettings"]["publicKey"],
            "reality-public-key"
        );
        assert_eq!(
            rendered["outbounds"][0]["streamSettings"]["realitySettings"]["shortId"],
            "abcd1234"
        );
    }

    #[test]
    fn test_heartbeat_config_supports_http_with_credentials() {
        let (heartbeat, rendered, http_id) = heartbeat_and_render_for_endpoint(test_exit_endpoint(
            "00000000-0000-0000-0000-000000009105",
            "third-party-http-auth",
            EndpointType::Http,
            "http-auth.example.test",
            8080,
            json!({
                "username": "http-user",
                "password": "http-secret"
            }),
        ));

        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "http"
        );
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["username"],
            "http-user"
        );
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["password"],
            "http-secret"
        );
        assert_eq!(
            heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(http_id)
        );
        assert_eq!(rendered["outbounds"][0]["protocol"], "http");
        assert_eq!(
            rendered["outbounds"][0]["settings"]["servers"][0]["users"][0],
            json!({"user": "http-user", "pass": "http-secret"})
        );
    }

    #[test]
    fn test_heartbeat_config_supports_trojan_shadowsocks_and_hysteria2_endpoints() {
        let (trojan_heartbeat, trojan_rendered, trojan_id) =
            heartbeat_and_render_for_endpoint(test_exit_endpoint(
                "00000000-0000-0000-0000-000000009106",
                "third-party-trojan",
                EndpointType::Trojan,
                "trojan.example.test",
                443,
                json!({
                    "password": "trojan-secret",
                    "server_name": "trojan-sni.example.test"
                }),
            ));
        assert_eq!(
            trojan_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "trojan"
        );
        assert_eq!(
            trojan_heartbeat["config"]["exit_endpoints"][0]["protocol"]["password"],
            "trojan-secret"
        );
        assert_eq!(
            trojan_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(trojan_id)
        );
        assert_eq!(trojan_rendered["outbounds"][0]["protocol"], "trojan");
        assert_eq!(
            trojan_rendered["outbounds"][0]["streamSettings"]["tlsSettings"]["serverName"],
            "trojan-sni.example.test"
        );

        let (ss_heartbeat, ss_rendered, ss_id) =
            heartbeat_and_render_for_endpoint(test_exit_endpoint(
                "00000000-0000-0000-0000-000000009107",
                "third-party-shadowsocks",
                EndpointType::Shadowsocks,
                "ss.example.test",
                8388,
                json!({
                    "method": "2022-blake3-aes-128-gcm",
                    "password": "ss-secret"
                }),
            ));
        assert_eq!(
            ss_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "shadowsocks"
        );
        assert_eq!(
            ss_heartbeat["config"]["exit_endpoints"][0]["protocol"]["method"],
            "2022-blake3-aes-128-gcm"
        );
        assert_eq!(
            ss_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(ss_id)
        );
        assert_eq!(ss_rendered["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(
            ss_rendered["outbounds"][0]["settings"]["password"],
            "ss-secret"
        );

        let (hy2_heartbeat, hy2_rendered, hy2_id) =
            heartbeat_and_render_for_endpoint(test_exit_endpoint(
                "00000000-0000-0000-0000-000000009108",
                "third-party-hysteria2",
                EndpointType::Hysteria,
                "hy2.example.test",
                443,
                json!({
                    "password": "hy2-secret",
                    "server_name": "hy2-sni.example.test"
                }),
            ));
        assert_eq!(
            hy2_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "hysteria2"
        );
        assert_eq!(
            hy2_heartbeat["config"]["exit_endpoints"][0]["protocol"]["password"],
            "hy2-secret"
        );
        assert_eq!(
            hy2_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(hy2_id)
        );
        assert_eq!(hy2_rendered["outbounds"][0]["protocol"], "hysteria");
        assert_eq!(
            hy2_rendered["outbounds"][0]["streamSettings"]["hysteriaSettings"]["version"],
            2
        );
        assert_eq!(
            hy2_rendered["outbounds"][0]["streamSettings"]["tlsSettings"]["serverName"],
            "hy2-sni.example.test"
        );

        let (hy2_auth_heartbeat, hy2_auth_rendered, hy2_auth_id) =
            heartbeat_and_render_for_endpoint(test_exit_endpoint(
                "00000000-0000-0000-0000-000000009109",
                "third-party-hysteria2-auth",
                EndpointType::Hysteria,
                "hy2-auth.example.test",
                443,
                json!({
                    "auth": "hy2-auth-secret",
                    "server_name": "hy2-auth-sni.example.test"
                }),
            ));
        assert_eq!(
            hy2_auth_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "hysteria2"
        );
        assert_eq!(
            hy2_auth_heartbeat["config"]["exit_endpoints"][0]["protocol"]["password"],
            "hy2-auth-secret"
        );
        assert_eq!(
            hy2_auth_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(hy2_auth_id)
        );
        assert_eq!(hy2_auth_rendered["outbounds"][0]["protocol"], "hysteria");
        assert_eq!(
            hy2_auth_rendered["outbounds"][0]["streamSettings"]["hysteriaSettings"]["version"],
            2
        );
    }

    #[tokio::test]
    async fn test_pg_http_vless_trojan_shadowsocks_hysteria2_heartbeat_and_subscription_redaction_when_database_url_is_set(
    ) {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL outbound protocol redaction test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let cases = vec![
            PgOutboundCase {
                name: "http-auth",
                resource_id: uuid("00000000-0000-0000-0000-000000009301"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009302"),
                outbound_type: "http",
                host: "http-auth-leak.example.test",
                port: 8080,
                config: json!({
                    "username": "http-leak-user",
                    "password": "http-leak-secret"
                }),
                heartbeat_type: "http",
                xray_protocol: "http",
                expected_protocol_fields: vec![
                    ("username", "http-leak-user"),
                    ("password", "http-leak-secret"),
                ],
                leaked_markers: vec![
                    "http-auth-leak.example.test",
                    "http-leak-user",
                    "http-leak-secret",
                ],
            },
            PgOutboundCase {
                name: "vless-reality",
                resource_id: uuid("00000000-0000-0000-0000-000000009311"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009312"),
                outbound_type: "vless",
                host: "vless-reality-leak.example.test",
                port: 443,
                config: json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "reality",
                    "server_name": "vless-reality-sni.example.test",
                    "public_key": "vless-reality-public-key",
                    "short_id": "abcd1234",
                    "fingerprint": "chrome"
                }),
                heartbeat_type: "vless",
                xray_protocol: "vless",
                expected_protocol_fields: vec![
                    ("uuid", "5dd7c58d-2ae0-478e-bba9-80354e17e107"),
                    ("security", "reality"),
                    ("server_name", "vless-reality-sni.example.test"),
                    ("public_key", "vless-reality-public-key"),
                ],
                leaked_markers: vec![
                    "vless-reality-leak.example.test",
                    "vless-reality-sni.example.test",
                    "vless-reality-public-key",
                ],
            },
            PgOutboundCase {
                name: "trojan",
                resource_id: uuid("00000000-0000-0000-0000-000000009303"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009304"),
                outbound_type: "trojan",
                host: "trojan-leak.example.test",
                port: 443,
                config: json!({
                    "password": "trojan-leak-secret",
                    "server_name": "trojan-leak-sni.example.test"
                }),
                heartbeat_type: "trojan",
                xray_protocol: "trojan",
                expected_protocol_fields: vec![
                    ("password", "trojan-leak-secret"),
                    ("server_name", "trojan-leak-sni.example.test"),
                ],
                leaked_markers: vec![
                    "trojan-leak.example.test",
                    "trojan-leak-secret",
                    "trojan-leak-sni.example.test",
                ],
            },
            PgOutboundCase {
                name: "shadowsocks",
                resource_id: uuid("00000000-0000-0000-0000-000000009305"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009306"),
                outbound_type: "shadowsocks",
                host: "ss-leak.example.test",
                port: 8388,
                config: json!({
                    "method": "2022-blake3-aes-128-gcm",
                    "password": "ss-leak-secret"
                }),
                heartbeat_type: "shadowsocks",
                xray_protocol: "shadowsocks",
                expected_protocol_fields: vec![
                    ("method", "2022-blake3-aes-128-gcm"),
                    ("password", "ss-leak-secret"),
                ],
                leaked_markers: vec![
                    "ss-leak.example.test",
                    "2022-blake3-aes-128-gcm",
                    "ss-leak-secret",
                ],
            },
            PgOutboundCase {
                name: "hysteria2",
                resource_id: uuid("00000000-0000-0000-0000-000000009307"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009308"),
                outbound_type: "hysteria",
                host: "hy2-leak.example.test",
                port: 443,
                config: json!({
                    "password": "hy2-leak-secret",
                    "server_name": "hy2-leak-sni.example.test"
                }),
                heartbeat_type: "hysteria2",
                xray_protocol: "hysteria",
                expected_protocol_fields: vec![
                    ("password", "hy2-leak-secret"),
                    ("server_name", "hy2-leak-sni.example.test"),
                ],
                leaked_markers: vec![
                    "hy2-leak.example.test",
                    "hy2-leak-secret",
                    "hy2-leak-sni.example.test",
                ],
            },
            PgOutboundCase {
                name: "hysteria2-auth",
                resource_id: uuid("00000000-0000-0000-0000-000000009309"),
                endpoint_id: uuid("00000000-0000-0000-0000-000000009310"),
                outbound_type: "hysteria",
                host: "hy2-auth-leak.example.test",
                port: 443,
                config: json!({
                    "auth": "hy2-auth-leak-secret",
                    "server_name": "hy2-auth-leak-sni.example.test"
                }),
                heartbeat_type: "hysteria2",
                xray_protocol: "hysteria",
                expected_protocol_fields: vec![
                    ("password", "hy2-auth-leak-secret"),
                    ("server_name", "hy2-auth-leak-sni.example.test"),
                ],
                leaked_markers: vec![
                    "hy2-auth-leak.example.test",
                    "hy2-auth-leak-secret",
                    "hy2-auth-leak-sni.example.test",
                ],
            },
        ];
        let endpoint_ids = cases
            .iter()
            .map(|case| case.endpoint_id)
            .collect::<Vec<_>>();
        let resource_ids = cases
            .iter()
            .map(|case| case.resource_id)
            .collect::<Vec<_>>();

        for case in cases {
            assert_pg_third_party_endpoint_heartbeat_and_subscription(&store, case).await;
        }
        cleanup_pg_third_party_endpoint_cases(&store, &endpoint_ids, &resource_ids).await;
    }
