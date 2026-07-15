// 数据库测试片段五十九:多域名 Phase 4 本机出口就地编辑/删除/列出 store。
// E1:update_local_exit_line 改端口/启用生效,并置节点 config_dirty。
// E2:delete_local_exit_line 删除生效 + 置节点 config_dirty;删不存在的端点报错。
// E3:list_node_local_exit_lines 列该节点 self_hosted 出口,含 node_domain_id。
// 真实 PostgreSQL 集成,缺 DATABASE_URL 时跳过且只输出脱敏原因。
// 只使用 example.test 占位与示例端口/RFC5737 IP,不写真实服务器/凭据。
// 断言以 store 返回 / 真实库读回为准,符合跨层对齐红线(写侧为准)。
// 复用 part57 的 create_md_guard_node / md_local_exit_line 等 helper,由父级 tests 模块 include! 引入。
// 从 part58 拆出 E 组,守住单文件 550 行硬上限。
// 本头部满足前十行中文注释约束。

    /// E:本机出口就地更新(改端口/启用)生效,并置节点 config_dirty。
    #[tokio::test]
    async fn test_update_local_exit_line_changes_fields_and_marks_dirty() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit update test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "le-upd", None, None).await;
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("socks", json!({}), None)],
                },
            )
            .await
            .unwrap();
        let endpoint_id: Uuid = created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap();
        // 先消化创建带来的 dirty,便于断言更新会重新置 dirty。
        clear_node_dirty(&store, node).await;

        store
            .update_local_exit_line(
                endpoint_id,
                AdminLocalExitLineUpdate {
                    port: Some(41_002),
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .expect("本机出口就地更新应成功");

        let (port, enabled): (i32, bool) =
            sqlx::query_as("SELECT port, enabled FROM exit_endpoints WHERE id = $1")
                .bind(endpoint_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(port, 41_002, "端口应被更新");
        assert!(!enabled, "enabled 应被更新为 false");
        assert!(node_is_dirty(&store, node).await, "更新本机出口应置节点 config_dirty");
    }

    /// E:本机出口删除生效,并置节点 config_dirty;删不存在的端点报错。
    #[tokio::test]
    async fn test_delete_local_exit_line_removes_and_marks_dirty() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit delete test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "le-del", None, None).await;
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("http", json!({}), None)],
                },
            )
            .await
            .unwrap();
        let endpoint_id: Uuid = created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap();
        clear_node_dirty(&store, node).await;

        store
            .delete_local_exit_line(endpoint_id)
            .await
            .expect("本机出口删除应成功");

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM exit_endpoints WHERE id = $1)")
                .bind(endpoint_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(!exists, "本机出口删除后应不存在");
        assert!(node_is_dirty(&store, node).await, "删除本机出口应置节点 config_dirty");

        // 删除不存在的 endpoint:返回 Err(非 self_hosted / 不存在)。
        let missing = store.delete_local_exit_line(Uuid::new_v4()).await;
        assert!(missing.is_err(), "删除不存在的本机出口应报错");
    }

    /// E:列出节点 self_hosted 出口线路,供 API/前端;含 node_domain_id。
    #[tokio::test]
    async fn test_list_node_local_exit_lines_lists_self_hosted() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping list local exit lines test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "le-list", Some("d-list.example.test"), None).await;
        let direct_id = store
            .list_node_domains(node)
            .await
            .unwrap()
            .iter()
            .find(|d| d.kind == "direct")
            .unwrap()
            .id;
        let trojan_config = json!({
            "password": "md-list-pass",
            "security": "tls",
            "server_name": "d-list.example.test",
            "certificate_file": "/etc/letsencrypt/live/d-list.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/d-list.example.test/privkey.pem",
        });
        store
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
            .unwrap();

        let value = store.list_node_local_exit_lines(node).await.unwrap();
        let items = value.as_array().expect("本机出口清单应为数组");
        assert_eq!(items.len(), 2, "应列出该节点两条 self_hosted 出口");
        let trojan = items
            .iter()
            .find(|i| i["outbound_type"] == "trojan")
            .expect("应含 trojan 出口");
        assert_eq!(
            trojan["node_domain_id"].as_str(),
            Some(direct_id.to_string().as_str()),
            "trojan 出口应回显 node_domain_id: {trojan}"
        );
        // 每条出口都必须含 node_domain_id 键(读侧契约「写侧有读侧无」红线)。
        for item in items {
            assert!(
                item.as_object().unwrap().contains_key("node_domain_id"),
                "每条出口都必须含 node_domain_id 键: {item}"
            );
        }
    }

    /// E:本机出口就地改 outbound_config 真持久化(socks 改 username/password)。
    /// 改协议配置走 fill+护栏后落库,库里 outbound_config 必须是新值(写侧为准)。
    #[tokio::test]
    async fn test_update_local_exit_line_persists_outbound_config() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit outbound_config update test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        let node = create_md_guard_node(&store, "le-cfg", None, None).await;
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("socks", json!({}), None)],
                },
            )
            .await
            .unwrap();
        let endpoint_id: Uuid = created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap();
        // 创建时自动生成的凭据,记下来确认改完确实变了。
        let before = read_endpoint_outbound_config(&store, endpoint_id).await;
        let before_user = before["username"].as_str().unwrap_or_default().to_string();
        clear_node_dirty(&store, node).await;

        // PUT 改 username/password:显式给新凭据,fill 不应覆盖管理员显式值。
        store
            .update_local_exit_line(
                endpoint_id,
                AdminLocalExitLineUpdate {
                    outbound_config: Some(json!({
                        "username": "le-cfg-user",
                        "password": "le-cfg-pass"
                    })),
                    ..Default::default()
                },
            )
            .await
            .expect("本机出口就地改协议配置应成功");

        let after = read_endpoint_outbound_config(&store, endpoint_id).await;
        assert_eq!(
            after["username"].as_str(),
            Some("le-cfg-user"),
            "outbound_config.username 应被持久化为新值: {after}"
        );
        assert_eq!(
            after["password"].as_str(),
            Some("le-cfg-pass"),
            "outbound_config.password 应被持久化为新值: {after}"
        );
        assert_ne!(
            after["username"].as_str().unwrap_or_default(),
            before_user,
            "新凭据应与创建时自动生成的不同"
        );
        assert!(
            node_is_dirty(&store, node).await,
            "改本机出口配置应置节点 config_dirty"
        );
    }

    /// E:就地改配置仍受证书域名护栏约束(编辑不绕护栏)。
    /// 无证书节点的本机出口,把配置改成 Trojan-TLS 形态 → 被护栏拒(纯 IP 节点不能用要证书协议)。
    #[tokio::test]
    async fn test_update_local_exit_line_config_edit_still_guards_cert_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set; skipping local exit cert-domain guard edit test");
            return;
        };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();

        // 无证书域名节点(create_md_guard_node 传 None=纯 IP 直连)。
        let node = create_md_guard_node(&store, "le-guard", None, None).await;
        let created = store
            .create_admin_local_exit_lines(
                node,
                AdminLocalExitLinesInput {
                    lines: vec![md_local_exit_line("trojan", trojan_tls_config(), None)],
                },
            )
            .await;
        // 纯 IP 节点连建 trojan 本机出口都该被护栏拒;若能建出来则用 socks 兜底再改。
        let endpoint_id = match created {
            Ok(value) => value["created_lines"][0]["exit_endpoint_id"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .unwrap(),
            Err(_) => {
                let socks = store
                    .create_admin_local_exit_lines(
                        node,
                        AdminLocalExitLinesInput {
                            lines: vec![md_local_exit_line("socks", json!({}), None)],
                        },
                    )
                    .await
                    .unwrap();
                socks["created_lines"][0]["exit_endpoint_id"]
                    .as_str()
                    .and_then(|v| v.parse().ok())
                    .unwrap()
            }
        };

        // 直接把库里端点类型改成 trojan,模拟「已有 trojan 本机出口」场景下再 PUT 改 TLS 配置。
        sqlx::query("UPDATE exit_endpoints SET outbound_type = 'trojan'::endpoint_type WHERE id = $1")
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();

        // PUT 把 outbound_config 改成要证书的 Trojan-TLS 形态 → 无证书节点应被护栏拒。
        let result = store
            .update_local_exit_line(
                endpoint_id,
                AdminLocalExitLineUpdate {
                    outbound_config: Some(trojan_tls_config()),
                    ..Default::default()
                },
            )
            .await;
        assert!(
            result.is_err(),
            "无证书节点的本机出口改成 Trojan-TLS 形态应被证书域名护栏拒,编辑不绕护栏"
        );
    }

    /// 构造一份要证书的 Trojan-TLS outbound_config(含证书/私钥路径,触发护栏与 fill 校验)。
    fn trojan_tls_config() -> serde_json::Value {
        json!({
            "password": "le-guard-pass",
            "security": "tls",
            "server_name": "le-guard.example.test",
            "certificate_file": "/etc/letsencrypt/live/le-guard.example.test/fullchain.pem",
            "key_file": "/etc/letsencrypt/live/le-guard.example.test/privkey.pem",
        })
    }

    // ---- 本片段专用 helper ----

    /// 清掉节点 config_dirty,便于断言后续写操作会重新置 dirty。
    async fn clear_node_dirty(store: &PgStore, node_id: Uuid) {
        sqlx::query("UPDATE access_nodes SET config_dirty = FALSE WHERE id = $1")
            .bind(node_id)
            .execute(store.pool())
            .await
            .unwrap();
    }

    /// 读节点 config_dirty 标志。
    async fn node_is_dirty(store: &PgStore, node_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>("SELECT config_dirty FROM access_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
    }
