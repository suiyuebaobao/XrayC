// 数据库测试片段五十八:多域名 Phase 5b 读模型落点 + Phase 3 证书锚定 + 心跳 node_domains。
// A:节点控制面读模型每个节点必含 domains[](空则 []),由 list_node_domains 映射成 NodeDomainView。
// B:入口/出口读模型 SELECT 出 node_domain_id(未选 null),并解析出 node_domain(domain/kind)。
// C:入口证书路径锚定选中 node_domain(选 cf→按 cf_cert_mode 锚 cf,选 direct→锚 direct,未选→回退节点主域名)。
// D:心跳响应带本节点 node_domains 清单(domain+kind+cf_cert_mode+acme_email),供 agent 遍历签证书。
// 本机出口就地编辑/删除/列出(Phase 4 的 E 组)拆到 part59,守住单文件 550 行硬上限。
// 真实 PostgreSQL 集成,缺 DATABASE_URL 时跳过且只输出脱敏原因;只用 example.test 占位/RFC5737 IP。
// 断言以 store 返回 / 真实库读回为准,符合跨层对齐红线(写侧为准)。
// 父级 tests 模块通过 include! 引入本文件,复用 part57 的 create_md_guard_node 等 helper。
// 本头部满足前十行中文注释约束。

    /// A:节点控制面读模型每个节点必含 domains 数组(空则 []),有域名时映射出 domain/kind/is_primary/cert_status。
    #[tokio::test]
    async fn test_access_routing_json_node_carries_domains_array() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping node domains read-model test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 节点 A:有 direct + cf 域名(回填两条 node_domains)。
        let node_with = create_md_guard_node(
            &store,
            "rm-with",
            Some("direct-rm.example.test"),
            Some("cf-rm.example.test"),
        )
        .await;
        // 节点 B:无任何域名(domains 必须是空数组,不能缺键)。
        let node_without = create_md_guard_node(&store, "rm-without", None, None).await;

        let value = store.access_routing_json().await.unwrap();
        let nodes = value["access_nodes"].as_array().expect("access_nodes 应为数组");

        let with = nodes
            .iter()
            .find(|n| n["id"].as_str() == Some(&node_with.to_string()))
            .expect("应找到有域名的节点");
        let domains = with["domains"].as_array().expect("有域名节点必含 domains 数组");
        assert!(
            domains
                .iter()
                .any(|d| d["domain"] == "direct-rm.example.test" && d["kind"] == "direct"),
            "domains 应含 direct 域名视图: {with}"
        );
        assert!(
            domains
                .iter()
                .any(|d| d["domain"] == "cf-rm.example.test" && d["kind"] == "cf"),
            "domains 应含 cf 域名视图: {with}"
        );
        // 视图字段齐全:每项必含 id/domain/kind/is_primary/cert_status。
        let first = &domains[0];
        for key in ["id", "domain", "kind", "is_primary", "cert_status"] {
            assert!(first.as_object().unwrap().contains_key(key), "domains 项缺键 {key}: {first}");
        }

        let without = nodes
            .iter()
            .find(|n| n["id"].as_str() == Some(&node_without.to_string()))
            .expect("应找到无域名的节点");
        // 「写侧有读侧无」红线:domains 键必含,空则空数组,不得缺失或 null。
        assert!(
            without.as_object().unwrap().contains_key("domains"),
            "无域名节点也必须含 domains 键: {without}"
        );
        assert_eq!(without["domains"], json!([]), "无域名节点 domains 应为空数组");
    }

    /// B:入口读模型 SELECT 出 node_domain_id(未选 null),选了则解析出 node_domain(domain/kind)。
    #[tokio::test]
    async fn test_admin_access_entries_json_carries_node_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping entry node_domain read-model test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(
            &store,
            "entry-rm",
            Some("direct-er.example.test"),
            None,
        )
        .await;
        let domains = store.list_node_domains(node).await.unwrap();
        let direct_id = domains.iter().find(|d| d.kind == "direct").unwrap().id;

        // 入口选 direct 域名。
        store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-entry-rm-direct",
                29_101,
                "trojan",
                "tcp",
                "tls",
                Some(direct_id),
            ))
            .await
            .expect("选 direct 域名的 Trojan 入口应建出");
        // 入口不选域名(免证书 Reality:IP 直连)。
        store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-entry-rm-none",
                29_102,
                "vless",
                "tcp",
                "reality",
                None,
            ))
            .await
            .expect("不选域名的 Reality 入口应建出");

        let value = store.list_admin_access_entries_json().await.unwrap();
        let entries = value.as_array().expect("入口读模型应为数组");

        let selected = entries
            .iter()
            .find(|e| e["name"] == "md-entry-rm-direct")
            .expect("应找到选域名入口");
        assert_eq!(
            selected["node_domain_id"].as_str(),
            Some(direct_id.to_string().as_str()),
            "选了域名的入口 node_domain_id 应回显: {selected}"
        );
        assert_eq!(
            selected["node_domain"]["domain"], "direct-er.example.test",
            "应解析出 node_domain.domain: {selected}"
        );
        assert_eq!(selected["node_domain"]["kind"], "direct", "应解析出 node_domain.kind: {selected}");

        let none = entries
            .iter()
            .find(|e| e["name"] == "md-entry-rm-none")
            .expect("应找到不选域名入口");
        // 「写侧有读侧无」红线:未选也必含 node_domain_id 键,值为 null。
        assert!(
            none.as_object().unwrap().contains_key("node_domain_id"),
            "未选域名入口也必须含 node_domain_id 键: {none}"
        );
        assert!(none["node_domain_id"].is_null(), "未选域名入口 node_domain_id 应为 null: {none}");
        assert!(none["node_domain"].is_null(), "未选域名入口 node_domain 应为 null: {none}");
    }

    /// B:出口读模型 SELECT 出 node_domain_id(未选 null),选了则解析出 node_domain(domain/kind)。
    #[tokio::test]
    async fn test_admin_exit_endpoints_json_carries_node_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping exit node_domain read-model test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "exit-rm", Some("direct-xr.example.test"), None).await;
        let domains = store.list_node_domains(node).await.unwrap();
        let direct_id = domains.iter().find(|d| d.kind == "direct").unwrap().id;

        let trojan_config = json!({
            "password": "md-rm-pass",
            "security": "tls",
            "server_name": "direct-xr.example.test",
            "certificate_file": "/etc/letsencrypt/live/direct-xr.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/direct-xr.example.test/privkey.pem",
        });
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![
                        md_local_exit_line("trojan", trojan_config, Some(direct_id)),
                        md_local_exit_line("socks", json!({}), None),
                    ],
                },
            )
            .await
            .expect("本机出口应建出");
        let trojan_ep: Uuid = created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap();

        let value = store.admin_exit_endpoints_json().await.unwrap();
        let items = value["exit_endpoints"].as_array().expect("出口读模型应为数组");

        let selected = items
            .iter()
            .find(|e| e["id"].as_str() == Some(&trojan_ep.to_string()))
            .expect("应找到选域名的出口");
        assert_eq!(
            selected["node_domain_id"].as_str(),
            Some(direct_id.to_string().as_str()),
            "出口 node_domain_id 应回显: {selected}"
        );
        assert_eq!(
            selected["node_domain"]["domain"], "direct-xr.example.test",
            "出口应解析出 node_domain.domain: {selected}"
        );
        assert_eq!(selected["node_domain"]["kind"], "direct", "出口应解析出 node_domain.kind");

        // 不选域名的 socks 出口:node_domain_id 键必含且为 null。
        for item in items {
            assert!(
                item.as_object().unwrap().contains_key("node_domain_id"),
                "每条出口都必须含 node_domain_id 键: {item}"
            );
        }
    }

    /// C:入口选 direct 域名时,证书路径锚定该 direct 域名(而非节点主域名 cert_domain)。
    #[tokio::test]
    async fn test_entry_cert_anchors_to_selected_direct_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping entry cert anchor direct test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 节点主 direct 域名为 primary-direct;再加一个备用 direct 域名 alt-direct。
        let node = create_md_guard_node(
            &store,
            "cert-direct",
            Some("primary-direct.example.test"),
            None,
        )
        .await;
        let alt_id = store
            .add_node_domain(
                node,
                AddNodeDomainInput {
                    domain: "alt-direct.example.test".to_string(),
                    kind: "direct".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // 入口选备用 alt-direct 域名 → 证书路径必须锚定 alt-direct,而非 primary-direct。
        let entry_id = store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-cert-alt",
                29_201,
                "trojan",
                "tcp",
                "tls",
                Some(alt_id),
            ))
            .await
            .expect("选备用 direct 域名的 Trojan 入口应建出");

        let config = read_entry_inbound_config(&store, entry_id).await;
        let cert = config["certificate_file"].as_str().unwrap_or_default();
        assert!(
            cert.contains("alt-direct.example.test"),
            "证书应锚定选中的备用 direct 域名,实际: {cert}"
        );
        assert!(
            !cert.contains("primary-direct.example.test"),
            "证书不应再锚定节点主域名: {cert}"
        );
        // server_name(订阅 SNI,落 access_entries.server_name 列)也应是选中域名。
        let sni = read_entry_server_name(&store, entry_id).await;
        assert_eq!(sni, "alt-direct.example.test", "SNI 应锚定选中的备用 direct 域名: {sni}");
    }

    /// C:入口选 cf 域名(dns01)时,证书路径锚定该 cf 域名(独立 LE 路径)。
    #[tokio::test]
    async fn test_entry_cert_anchors_to_selected_cf_domain_dns01() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping entry cert anchor cf test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "cert-cf", Some("direct-cc.example.test"), None).await;
        // 加一个显式 dns01 的 cf 域名(默认派生已是 reuse_direct;本测试要验 dns01 锚定到选中 cf 域名自己的 LE 路径)。
        let cf_id = store
            .add_node_domain(
                node,
                AddNodeDomainInput {
                    domain: "cf-pick.example.test".to_string(),
                    kind: "cf".to_string(),
                    acme_email: Some("ops@example.test".to_string()),
                    cf_cert_mode: Some("dns01".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // 入口选 cf 域名 + VLESS/ws/tls → 证书锚定该 cf 域名自己的 LE 路径。
        // 端口用 2083(CF 支持的 HTTPS 端口),避免被 CF 入口端口护栏拦下。
        let entry_id = store
            .create_admin_access_entry(md_entry_input(
                node,
                "md-cert-cf",
                2083,
                "vless",
                "ws",
                "tls",
                Some(cf_id),
            ))
            .await
            .expect("选 cf 域名的 VLESS/ws/tls 入口应建出");

        let config = read_entry_inbound_config(&store, entry_id).await;
        let cert = config["certificate_file"].as_str().unwrap_or_default();
        assert!(
            cert.contains("cf-pick.example.test"),
            "证书应锚定选中的 cf 域名(dns01 独立路径),实际: {cert}"
        );
        assert!(
            !cert.contains("direct-cc.example.test"),
            "证书不应锚定节点直连主域名: {cert}"
        );
    }

    /// D:心跳响应带本节点 node_domains 清单(domain+kind+cf_cert_mode+acme_email)。
    #[tokio::test]
    async fn test_heartbeat_carries_node_domains_for_agent() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping heartbeat node_domains test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(
            &store,
            "hb-dom",
            Some("direct-hb.example.test"),
            None,
        )
        .await;
        // 加一个显式 dns01 的 cf 域名,心跳应把 cf_cert_mode + acme_email 一并下发给 agent。
        store
            .add_node_domain(
                node,
                AddNodeDomainInput {
                    domain: "cf-hb.example.test".to_string(),
                    kind: "cf".to_string(),
                    acme_email: Some("hb@example.test".to_string()),
                    cf_cert_mode: Some("dns01".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let response = store.heartbeat_json(Some(node), None).await.unwrap();
        let domains = response["node_domains"]
            .as_array()
            .expect("心跳响应必含 node_domains 数组");
        let direct = domains
            .iter()
            .find(|d| d["domain"] == "direct-hb.example.test")
            .expect("心跳应含 direct 域名");
        assert_eq!(direct["kind"], "direct", "direct 域名 kind 应为 direct");

        let cf = domains
            .iter()
            .find(|d| d["domain"] == "cf-hb.example.test")
            .expect("心跳应含 cf 域名");
        assert_eq!(cf["kind"], "cf", "cf 域名 kind 应为 cf");
        assert_eq!(cf["cf_cert_mode"], "dns01", "cf 域名应下发 cf_cert_mode=dns01");
        assert_eq!(cf["acme_email"], "hb@example.test", "cf 域名应下发 acme_email");
    }

    // ---- 本片段专用 helper ----

    /// 读回入口 inbound_config,验证证书路径锚定。
    async fn read_entry_inbound_config(store: &PgStore, entry_id: Uuid) -> serde_json::Value {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT inbound_config FROM access_entries WHERE id = $1",
        )
        .bind(entry_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
    }

    /// 读回入口 server_name 列(订阅 SNI),验证锚定选中域名。
    async fn read_entry_server_name(store: &PgStore, entry_id: Uuid) -> String {
        sqlx::query_scalar::<_, String>("SELECT server_name FROM access_entries WHERE id = $1")
            .bind(entry_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
    }
