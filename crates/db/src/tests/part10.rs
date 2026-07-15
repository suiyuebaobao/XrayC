/// 数据库测试分片 10。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    #[tokio::test]
    async fn test_pg_heartbeat_returns_vless_reality_endpoint_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping PostgreSQL vless config test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
        let resource_id = uuid("00000000-0000-0000-0000-000000009211");
        let endpoint_id = uuid("00000000-0000-0000-0000-000000009212");

        sqlx::query(
            r#"
            INSERT INTO exit_resources (id, name, region_code, enabled)
            VALUES ($1, 'third-party-vless', 'US', TRUE)
            ON CONFLICT (id) DO UPDATE SET enabled = TRUE
            "#,
        )
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, outbound_type, host, port,
                outbound_config, enabled
            )
            VALUES (
                $1, $2, 'vless'::endpoint_type, 'vless.example.test', 443,
                '{
                    "uuid":"5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security":"reality",
                    "flow":"xtls-rprx-vision",
                    "server_name":"www.example.com",
                    "public_key":"reality-public-key",
                    "short_id":"abcd1234",
                    "fingerprint":"chrome"
                }'::jsonb,
                TRUE
            )
            ON CONFLICT (id) DO UPDATE SET
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(endpoint_id)
        .bind(resource_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status,
                allow_new_assignments
            )
            VALUES ($1, $2, 100, 'healthy', TRUE)
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                status = EXCLUDED.status,
                allow_new_assignments = EXCLUDED.allow_new_assignments
            "#,
        )
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = CASE WHEN exit_endpoint_id = $2 THEN 'healthy' ELSE 'offline' END,
                allow_new_assignments = CASE WHEN exit_endpoint_id = $2 THEN TRUE ELSE FALSE END
            WHERE exit_pool_id = $1
            "#,
        )
        .bind(pool_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            DELETE FROM user_exit_assignments
            WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
            "#,
        )
        .bind(user_id)
        .bind(line_id)
        .bind(pool_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE access_lines SET exit_endpoint_id = $2 WHERE id = $1")
            .bind(line_id)
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(line_group_id)
        .bind(endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(yaml.contains("access.example.test"));
        assert!(!yaml.contains("vless.example.test"));
        assert!(!yaml.contains("5dd7c58d-2ae0-478e-bba9-80354e17e107"));
        assert!(!yaml.contains("reality-public-key"));
        let assigned = assigned_exit_assignment(&store, user_id, line_id, pool_id).await;
        assert_eq!(assigned.exit_endpoint_id, endpoint_id);

        let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        let exit_tag = exit_endpoint_tag(endpoint_id);
        let vless_endpoint = heartbeat["config"]["exit_endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|endpoint| endpoint["tag"] == exit_tag)
            .expect("vless endpoint should be present");
        assert_eq!(vless_endpoint["protocol"]["type"], "vless");
        assert_eq!(vless_endpoint["protocol"]["security"], "reality");
        assert_eq!(
            vless_endpoint["protocol"]["public_key"],
            "reality-public-key"
        );
        assert_eq!(vless_endpoint["protocol"]["short_id"], "abcd1234");
        let has_vless_route = heartbeat["config"]["routing_rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["outbound_tag"] == exit_tag);
        assert!(has_vless_route);
    }

    #[test]
    fn test_heartbeat_config_supports_no_auth_socks_and_http_endpoints() {
        let store = MemoryStore::seeded();
        let (mut data, node_id, socks_id, http_id) = store.write(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let line_id = *data.access_lines.keys().next().unwrap();
            let pool_id = data.access_lines.get(&line_id).unwrap().exit_pool_id;
            let user_id = *data.users.keys().next().unwrap();
            let socks_id = uuid("00000000-0000-0000-0000-000000009101");
            let http_id = uuid("00000000-0000-0000-0000-000000009102");
            let socks_endpoint = ExitEndpoint {
                id: socks_id,
                resource_name: "third-party-socks".to_string(),
                ownership: "third_party".to_string(),
                owner_access_node_id: None,
                outbound_type: EndpointType::Socks,
                host: "socks.example.test".to_string(),
                port: 1080,
                outbound_config: json!({}),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            let http_endpoint = ExitEndpoint {
                id: http_id,
                resource_name: "third-party-http".to_string(),
                ownership: "third_party".to_string(),
                owner_access_node_id: None,
                outbound_type: EndpointType::Http,
                host: "http.example.test".to_string(),
                port: 8080,
                outbound_config: json!({}),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            data.exit_pools.get_mut(&pool_id).unwrap().members =
                vec![socks_endpoint, http_endpoint];
            data.user_exit_assignments = vec![UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: socks_id,
                assigned_at: Utc::now(),
                failover_reason: "initial".to_string(),
            }];
            (data.clone(), node_id, socks_id, http_id)
        });

        let socks_heartbeat = heartbeat_json(&data, Some(node_id), None);
        assert_eq!(socks_heartbeat["config_status"]["required"], true);
        assert_eq!(
            socks_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "socks"
        );
        assert_eq!(
            socks_heartbeat["config"]["exit_endpoints"][0]["protocol"]["address"],
            "socks.example.test"
        );
        assert_eq!(
            socks_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(socks_id)
        );

        data.user_exit_assignments[0].exit_endpoint_id = http_id;
        let http_heartbeat = heartbeat_json(&data, Some(node_id), None);
        assert_eq!(
            http_heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "http"
        );
        assert_eq!(
            http_heartbeat["config"]["exit_endpoints"][0]["protocol"]["address"],
            "http.example.test"
        );
        assert_eq!(
            http_heartbeat["config"]["routing_rules"][0]["outbound_tag"],
            exit_endpoint_tag(http_id)
        );
    }

    #[test]
    fn test_heartbeat_config_supports_socks_with_credentials() {
        let store = MemoryStore::seeded();
        let (data, node_id) = store.write(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let line_id = *data.access_lines.keys().next().unwrap();
            let pool_id = data.access_lines.get(&line_id).unwrap().exit_pool_id;
            let user_id = *data.users.keys().next().unwrap();
            let socks_id = uuid("00000000-0000-0000-0000-000000009103");
            let credentialed_socks = ExitEndpoint {
                id: socks_id,
                resource_name: "credentialed-socks".to_string(),
                ownership: "third_party".to_string(),
                owner_access_node_id: None,
                outbound_type: EndpointType::Socks,
                host: "socks-auth.example.test".to_string(),
                port: 1080,
                outbound_config: json!({
                    "username": "user",
                    "password": "secret"
                }),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            data.exit_pools.get_mut(&pool_id).unwrap().members = vec![credentialed_socks];
            data.user_exit_assignments = vec![UserExitAssignment {
                user_id,
                access_line_id: line_id,
                exit_pool_id: pool_id,
                exit_endpoint_id: socks_id,
                assigned_at: Utc::now(),
                failover_reason: "initial".to_string(),
            }];
            (data.clone(), node_id)
        });

        let heartbeat = heartbeat_json(&data, Some(node_id), None);
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["type"],
            "socks"
        );
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["username"],
            "user"
        );
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["protocol"]["password"],
            "secret"
        );
    }

    #[test]
    fn test_heartbeat_config_does_not_render_self_hosted_line_without_access_lines() {
        let store = MemoryStore::seeded();
        let (data, node_id) = store.write(|data| {
            let node_id = *data.access_nodes.keys().next().unwrap();
            let pool_id = *data.exit_pools.keys().next().unwrap();
            let socks_id = uuid("00000000-0000-0000-0000-000000009105");
            let local_socks = ExitEndpoint {
                id: socks_id,
                resource_name: "local-socks-service".to_string(),
                ownership: "self_hosted".to_string(),
                owner_access_node_id: Some(node_id),
                outbound_type: EndpointType::Socks,
                host: "access.example.test".to_string(),
                port: 38081,
                outbound_config: json!({
                    "username": "local-user",
                    "password": "local-pass"
                }),
                stream_config: json!({}),
                weight: 100,
                priority: 100,
                allow_new_assignments: true,
                healthy: true,
                status: "healthy".to_string(),
            };
            data.access_lines.clear();
            data.user_exit_assignments.clear();
            data.exit_pools.get_mut(&pool_id).unwrap().members = vec![local_socks];
            (data.clone(), node_id)
        });

        let heartbeat = heartbeat_json(&data, Some(node_id), None);

        assert_eq!(heartbeat["config_status"]["required"], true);
        assert!(heartbeat["config"].is_null());
    }

    #[test]
    fn test_endpoint_config_empty_requires_empty_object() {
        assert!(endpoint_config_is_empty(&json!({})));
        assert!(!endpoint_config_is_empty(&json!({"username": "agent"})));
        assert!(!endpoint_config_is_empty(&json!(null)));
        assert!(!endpoint_config_is_empty(&json!([])));
        assert!(!endpoint_config_is_empty(&json!("")));
    }

    #[test]
    fn test_exit_endpoint_write_validation_rejects_invalid_protocol_payloads() {
        for (outbound_type, config) in [
            ("http", json!({"username": "user"})),
            (
                "vless",
                json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "reality"
                }),
            ),
            (
                "vless",
                json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "grpc"
                }),
            ),
            ("trojan", json!({})),
            (
                "trojan",
                json!({"password": "trojan-secret", "security": "grpc"}),
            ),
            ("shadowsocks", json!({"method": "2022-blake3-aes-128-gcm"})),
            ("hysteria", json!({"server_name": "hy2.example.test"})),
            ("direct", json!({"password": "must-not-exist"})),
        ] {
            assert!(
                validate_exit_endpoint_protocol_config(outbound_type, &config).is_err(),
                "{outbound_type} should reject invalid config"
            );
        }
        assert!(validate_exit_endpoint_protocol_config("http", &json!("bad")).is_err());
    }

    #[test]
    fn test_exit_endpoint_write_validation_accepts_valid_protocol_payloads() {
        for (outbound_type, config) in [
            ("socks", json!({})),
            ("socks", json!({"username": "user", "password": "pass"})),
            ("http", json!({})),
            ("http", json!({"username": "user", "password": "pass"})),
            (
                "vless",
                json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "reality",
                    "server_name": "www.example.test",
                    "public_key": "reality-public-key"
                }),
            ),
            (
                "vless",
                json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "tls",
                    "server_name": "www.example.test"
                }),
            ),
            (
                "vless",
                json!({
                    "uuid": "5dd7c58d-2ae0-478e-bba9-80354e17e107",
                    "security": "none"
                }),
            ),
            (
                "trojan",
                json!({
                    "password": "trojan-secret",
                    "security": "tls",
                    "server_name": "trojan.example.test"
                }),
            ),
            (
                "shadowsocks",
                json!({"method": "2022-blake3-aes-128-gcm", "password": "MDEyMzQ1Njc4OWFiY2RlZg=="}),
            ),
            (
                "hysteria",
                json!({"auth": "hy2-auth", "allow_insecure": true}),
            ),
            ("direct", json!({})),
        ] {
            validate_exit_endpoint_protocol_config(outbound_type, &config)
                .unwrap_or_else(|err| panic!("{outbound_type} should accept valid config: {err}"));
        }
    }

    #[test]
    fn test_exit_endpoint_write_validation_rejects_cleartext_trojan() {
        assert!(validate_exit_endpoint_protocol_config(
            "trojan",
            &json!({"password": "trojan-secret", "security": "none"})
        )
        .is_err());
        assert!(validate_exit_endpoint_protocol_config(
            "trojan",
            &json!({"password": "trojan-secret", "security": "tls"})
        )
        .is_err());
    }

    #[test]
    fn test_xhttp_mode_validation_defaults_to_auto() {
        assert_eq!(validate_xhttp_mode("").unwrap(), "auto");
        assert_eq!(validate_xhttp_mode("stream-one").unwrap(), "stream-one");
        assert_eq!(validate_xhttp_mode("auto").unwrap(), "auto");
        assert!(validate_xhttp_mode("invalid").is_err());
        assert!(validate_access_protocol_transport("vless", "xhttp").is_ok());
        assert!(validate_access_protocol_transport("vless", "tcp").is_ok());
    }
