/// 数据库测试分片 01。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。
    use super::*;
    use std::sync::{Arc, OnceLock};
    use tokio::sync::{Barrier, Mutex, MutexGuard};
    use xrayc_core::MemoryStore;

    static PG_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    async fn pg_test_guard() -> MutexGuard<'static, ()> {
        assert_safe_pg_test_database();
        PG_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await
    }

    fn assert_safe_pg_test_database() {
        if std::env::var("XRAYC_ALLOW_DESTRUCTIVE_DB_TESTS").ok().as_deref() == Some("1") {
            return;
        }
        let database_url = std::env::var("DATABASE_URL").unwrap_or_default();
        let database_name = database_url
            .rsplit_once('/')
            .map(|(_, name)| name)
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        assert!(
            database_name.contains("test") || database_name.contains("ci"),
            "refusing destructive PostgreSQL tests against non-test database; set XRAYC_ALLOW_DESTRUCTIVE_DB_TESTS=1 only for disposable test databases"
        );
    }

    #[test]
    fn test_public_audit_summary_redacts_sensitive_fields() {
        let summary = public_audit_summary(json!({
            "name": "safe-change",
            "token": "secret-token",
            "ssh_password_redacted": true,
            "subscription_link": "https://example.test/sub/sub-secret",
            "outbound_config": {
                "proxy_url": "socks5://agent:secret-password@example.test:1080",
                "password": "secret-password"
            },
            "nested": {
                "port": 1080,
                "private_key": "redacted-test-private-key"
            }
        }));

        assert_eq!(summary["name"], "safe-change");
        assert_eq!(summary["nested"]["port"], 1080);
        assert_eq!(summary["sensitive_fields_redacted"], true);
        let text = summary.to_string();
        assert!(!text.contains("secret-token"));
        assert!(!text.contains("secret-password"));
        assert!(!text.contains("socks5://"));
        assert!(!text.contains("sub-secret"));
        assert!(!text.contains("outbound_config"));
        assert!(!text.to_ascii_lowercase().contains("password"));
        assert!(!text.to_ascii_lowercase().contains("token"));
    }

    #[test]
    fn test_trojan_inbound_config_defaults_to_letsencrypt_tls_files() {
        let config =
            normalize_access_inbound_config("trojan", json!({}), "trojan.example.test").unwrap();

        validate_access_inbound_config("trojan", &config, "trojan.example.test").unwrap();
        assert_eq!(config["security"], "tls");
        assert_eq!(
            config["certificate_file"],
            "/etc/letsencrypt/live/trojan.example.test/fullchain.pem"
        );
        assert_eq!(
            config["key_file"],
            "/etc/letsencrypt/live/trojan.example.test/privkey.pem"
        );
    }

    #[test]
    fn test_db_port_to_u16_rejects_invalid_ports() {
        assert_eq!(db_port_to_u16(0, "exit_endpoints.port", true).unwrap(), 0);
        assert_eq!(
            db_port_to_u16(1, "access_lines.listen_port", false).unwrap(),
            1
        );
        assert_eq!(
            db_port_to_u16(65_535, "access_lines.listen_port", false).unwrap(),
            65_535
        );
        assert!(db_port_to_u16(0, "access_lines.listen_port", false).is_err());
        assert!(db_port_to_u16(-1, "exit_endpoints.port", true).is_err());
        assert!(db_port_to_u16(65_536, "exit_endpoints.port", true).is_err());
    }

    #[test]
    fn test_access_protocol_validation_allows_shadowsocks_and_rejects_unsafe_inbounds() {
        assert_eq!(validate_access_protocol("ss").unwrap(), "shadowsocks");
        assert_eq!(
            validate_access_protocol("shadowsocks").unwrap(),
            "shadowsocks"
        );
        assert_eq!(validate_access_protocol("hy2").unwrap(), "hysteria");
        assert_eq!(
            validate_access_protocol("hysteria2").unwrap(),
            "hysteria"
        );
        for protocol in ["http", "socks", "vmess"] {
            let err = validate_access_protocol(protocol).expect_err("protocol should be rejected");
            assert!(err.to_string().contains("unsupported"));
        }
    }

    #[test]
    fn test_access_protocol_mapping_supports_shadowsocks_without_vless_fallback() {
        let store = MemoryStore::seeded();
        let mut line = store.read(|data| data.access_lines.values().next().unwrap().clone());
        line.protocol = "shadowsocks".to_string();
        line.transport = "tcp".to_string();
        line.udp_enabled = false;
        line.inbound_config = json!({
            "method": "aes-256-gcm",
            "password": "line-root-secret"
        });

        let protocol = access_protocol_for_line(&line).expect("shadowsocks should map");

        match protocol {
            XrayAccessProtocol::Shadowsocks {
                method,
                server_password,
                network,
            } => {
                assert_eq!(method, "aes-256-gcm");
                assert_eq!(server_password, "line-root-secret");
                assert_eq!(network, "tcp");
            }
            other => panic!("unexpected protocol: {other:?}"),
        }

        line.protocol = "vmess".to_string();
        assert!(access_protocol_for_line(&line).is_none());
    }

    #[test]
    fn test_client_ip_hash_requires_sha256_hex_format() {
        assert!(is_valid_client_ip_hash(
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_valid_client_ip_hash("198.51.100.10"));
        assert!(!is_valid_client_ip_hash("sha256:198.51.100.10"));
        assert!(!is_valid_client_ip_hash("sha256:test-client"));
    }

    #[test]
    fn test_memory_json_read_models_match_frontend_contract() {
        let store = MemoryStore::seeded();
        let (routing, pools, summary, subscription) = store.read(|data| {
            (
                access_routing_json(data),
                exit_pools_json(data),
                operations_summary_json(data),
                user_subscription_json(data),
            )
        });

        assert!(routing["access_lines"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()));
        let routing_text = routing.to_string();
        assert!(!routing_text.contains("198.51.100.10"));
        assert!(!routing_text.contains("outbound_config"));
        assert!(!routing_text.contains("demo-agent-token"));
        assert!(pools["exit_pools"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));
        assert_eq!(routing["access_lines"][0]["region_code"], "HK");
        assert_eq!(routing["access_lines"][0]["region_name"], "香港");
        assert_eq!(routing["access_lines"][0]["region_flag"], "🇭🇰");
        assert!(routing["access_lines"][0]["online_users"].is_null());
        assert!(routing["access_lines"][0]["latency_ms"].is_null());
        assert_eq!(routing["access_lines"][0]["metric_status"], "no_data");
        assert_eq!(routing["access_lines"][0]["probe_status"], "unknown");
        assert_eq!(routing["exit_pools"][0]["members"][0]["status"], "healthy");
        assert_eq!(summary["line_group_count"], 1);
        assert_eq!(summary["exit_pool_count"], 1);
        assert_eq!(subscription["access_lines"][0]["region_code"], "HK");
        assert_eq!(subscription["access_lines"][0]["region_name"], "香港");
        assert_eq!(subscription["access_lines"][0]["region_flag"], "🇭🇰");
        assert_eq!(summary["active_users"], 1);
        assert_eq!(subscription["token"], "demo-token");
        let plans = store.read(|data| {
            data.plans
                .values()
                .map(|plan| serde_json::to_value(plan).unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(plans[0]["price_cents"], 0);
        assert_eq!(plans[0]["duration_days"], 30);
    }

    #[test]
    fn test_heartbeat_rate_limit_uses_user_override_above_plan() {
        let store = MemoryStore::seeded();
        let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
        store.write(|data| {
            let plan = data.plans.values_mut().next().unwrap();
            plan.rate_limit_bps = 10_000_000;
            let user = data.users.values_mut().next().unwrap();
            let user_id = user.id;
            user.rate_limit_bps = Some(25_000_000);
            let line = data.access_lines.values().next().unwrap();
            let endpoint = data
                .exit_pools
                .get(&line.exit_pool_id)
                .unwrap()
                .members
                .first()
                .unwrap();
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line.id,
                exit_pool_id: line.exit_pool_id,
                exit_endpoint_id: endpoint.id,
                assigned_at: Utc::now(),
                failover_reason: "unit-test".to_string(),
            });
        });

        let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));

        assert_eq!(heartbeat["config_status"]["required"], true);
        assert_eq!(heartbeat["config"]["rate_limits"][0]["rate_limit_bps"], 25_000_000);
        assert_eq!(
            heartbeat["config"]["exit_endpoints"][0]["sockopt_mark"],
            heartbeat["config"]["rate_limits"][0]["mark"]
        );
        assert!(
            heartbeat["config"]["routing_rules"][0]["outbound_tag"]
                .as_str()
                .is_some_and(|tag| tag.contains("-user-"))
        );
        assert_eq!(
            heartbeat["config"]["rate_limits"][0]["xray_user_key"],
            "u-"
                .to_owned()
                + heartbeat["config"]["rate_limits"][0]["user_id"]
                    .as_str()
                    .unwrap()
                    .replace('-', "")
                    .as_str()
                + "@xrayc.local"
        );
    }

    #[test]
    fn test_subscription_read_model_requires_assignable_exit_member() {
        let store = MemoryStore::seeded();
        assert_eq!(
            store.read(|data| user_subscription_json(data)["access_lines"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default()),
            1
        );

        let (access_node_id, exit_pool_id, exit_endpoint_id) = store.read(|data| {
            let line = data.access_lines.values().next().unwrap();
            let endpoint = data
                .exit_pools
                .get(&line.exit_pool_id)
                .unwrap()
                .members
                .first()
                .unwrap();
            (line.access_node_id, line.exit_pool_id, endpoint.id)
        });
        store.write(|data| {
            data.exit_pools
                .get_mut(&exit_pool_id)
                .unwrap()
                .members
                .first_mut()
                .unwrap()
                .status = "offline".to_string();
        });
        assert_eq!(
            store.read(|data| user_subscription_json(data)["access_lines"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default()),
            0
        );

        store.write(|data| {
            data.exit_pools
                .get_mut(&exit_pool_id)
                .unwrap()
                .members
                .first_mut()
                .unwrap()
                .status = "healthy".to_string();
            data.access_exit_probe_states.insert(
                (access_node_id, exit_endpoint_id),
                AccessExitProbeState {
                    access_node_id,
                    exit_endpoint_id,
                    effective_status: "offline".to_string(),
                },
            );
        });
        assert_eq!(
            store.read(|data| user_subscription_json(data)["access_lines"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default()),
            1
        );
        assert_eq!(
            store.read(|data| {
                let user_id = data.tokens["demo-token"].user_id;
                user_subscription_json_for_user_with_probe_policy(data, user_id, true)[
                    "access_lines"
                ]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default()
            }),
            0
        );
    }

    #[test]
    fn test_subscription_read_model_exports_all_entries_bound_to_flat_group() {
        let store = MemoryStore::seeded();
        store.write(|data| {
            let plan_id = *data.plans.keys().next().unwrap();
            let group_id = data.plans[&plan_id].line_groups[0].line_group_id;
            let base_line_id = *data.access_lines.keys().next().unwrap();
            let mut second_line = data.access_lines[&base_line_id].clone();
            second_line.id = Uuid::new_v4();
            second_line.name = "共享出口池备用线路".to_string();
            second_line.listen_port += 1;
            data.access_lines.insert(second_line.id, second_line.clone());
            data.line_groups
                .get_mut(&group_id)
                .unwrap()
                .line_ids
                .push(second_line.id);
        });

        let names = store.read(|data| {
            user_subscription_json(data)["access_lines"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|line| line["name"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            names,
            vec![
                "共享出口池备用线路".to_string(),
                "香港 01".to_string()
            ]
        );
    }
