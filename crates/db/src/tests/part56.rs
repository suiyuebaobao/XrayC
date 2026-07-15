// 数据库测试片段五十六:node_domains 多域名表 + 旧单域名迁移回填。
// 规则:迁移把 access_nodes.cert_domain→direct 行、cf_domain→cf 行,均 is_primary=true。
// 派生 cf_enabled:存在 kind='cf' 行即启用 CF。真实 PG,缺 DATABASE_URL 跳过。
// 只用 example.test 占位。断言以 store 返回为准。父 tests 模块 include! 引入。
// 单文件低于 550 行。本头满足前十行中文注释。
// 覆盖:回填两行、重复 domain 报错、删被引用域名报错、cf 域名 cf_cert_mode 派生。
// 还覆盖 update_access_node 改 cert_domain 同步 node_domains 主行。
// 所有测试串行(--test-threads=1),共享 xrayc_test 库由 pg_test_guard 串行化。
// 不在此保存任何真实服务器地址或凭据。
// 本头部满足前十行中文注释约束。
    #[tokio::test]
    async fn test_node_domains_backfill_from_single_domain_columns() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store.create_admin_access_node(AdminAccessNodeInput {
            name: "md-backfill".into(),
            public_host: "md.example.test".into(),
            public_port: 443,
            agent_token: "md-backfill-token".into(),
            cert_domain: Some("direct.example.test".into()),
            cf_domain: Some("cf.example.test".into()),
            ..Default::default()
        }).await.unwrap();
        let domains = store.list_node_domains(node_id).await.unwrap();
        assert!(domains.iter().any(|d| d.domain == "direct.example.test" && d.kind == "direct" && d.is_primary));
        assert!(domains.iter().any(|d| d.domain == "cf.example.test" && d.kind == "cf" && d.is_primary));
    }

    /// 同节点重复 domain 必须报错(业务原因,而非裸 unique 约束错误)。
    #[tokio::test]
    async fn test_add_node_domain_rejects_duplicate_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-dup".into(),
                public_host: "md-dup.example.test".into(),
                public_port: 443,
                agent_token: "md-dup-token".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "alt.example.test".into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let err = store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "alt.example.test".into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(err, Err(DbError::InvalidInput(_))), "重复 domain 应报 InvalidInput,实际: {err:?}");
    }

    /// 删除仍被 access_entries 引用的域名必须报错(先改引用再删)。
    #[tokio::test]
    async fn test_delete_node_domain_rejects_when_referenced_by_entry() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-ref".into(),
                public_host: "md-ref.example.test".into(),
                public_port: 443,
                agent_token: "md-ref-token".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let domain_id = store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "ref.example.test".into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        // 真实建一条入口并把它的 node_domain_id 指向该域名,制造引用关系。
        sqlx::query(
            "INSERT INTO access_entries (access_node_id, listen_port, node_domain_id) VALUES ($1, 28443, $2)",
        )
        .bind(node_id)
        .bind(domain_id)
        .execute(store.pool())
        .await
        .unwrap();
        let err = store.delete_node_domain(domain_id).await;
        assert!(matches!(err, Err(DbError::InvalidInput(_))), "被引用域名删除应报 InvalidInput,实际: {err:?}");
        // 改走其他域名(置空引用)后即可删除。
        sqlx::query("UPDATE access_entries SET node_domain_id = NULL WHERE node_domain_id = $1")
            .bind(domain_id)
            .execute(store.pool())
            .await
            .unwrap();
        store.delete_node_domain(domain_id).await.unwrap();
        let remaining = store.list_node_domains(node_id).await.unwrap();
        assert!(remaining.iter().all(|d| d.id != domain_id), "解除引用后域名应已删除");
    }

    /// CF 域名默认 cf_cert_mode 派生为 reuse_direct(token-less 免 token,acme 非 DNS-01 判据);direct 域名 cf_cert_mode 为空。
    #[tokio::test]
    async fn test_add_cf_node_domain_derives_reuse_direct_cert_mode() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-cfmode".into(),
                public_host: "md-cfmode.example.test".into(),
                public_port: 443,
                agent_token: "md-cfmode-token".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "cfmode.example.test".into(),
                    kind: "cf".into(),
                    acme_email: Some("ops@example.test".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "directmode.example.test".into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let domains = store.list_node_domains(node_id).await.unwrap();
        let cf = domains.iter().find(|d| d.domain == "cfmode.example.test").unwrap();
        assert_eq!(cf.kind, "cf");
        assert_eq!(
            cf.cf_cert_mode.as_deref(),
            Some("reuse_direct"),
            "CF + acme 默认 reuse_direct(免 token);acme 非 DNS-01 凭据信号,要 dns01 须显式设"
        );
        let direct = domains.iter().find(|d| d.domain == "directmode.example.test").unwrap();
        assert_eq!(direct.cf_cert_mode, None, "direct 域名不应有 cf_cert_mode");
        // node_has_domain_kind:存在 cf 行即视为启用 CF;不存在的 kind 返回 false。
        assert!(store.node_has_domain_kind(node_id, "cf").await.unwrap());
        assert!(store.node_has_domain_kind(node_id, "direct").await.unwrap());
    }

    /// set_primary 保证同 kind 下唯一 is_primary:第二个域名设主后,第一个不再是主。
    #[tokio::test]
    async fn test_set_primary_node_domain_unique_per_kind() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-primary".into(),
                public_host: "md-primary.example.test".into(),
                public_port: 443,
                agent_token: "md-primary-token".into(),
                cert_domain: Some("p1.example.test".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let second = store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "p2.example.test".into(),
                    kind: "direct".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        store.set_primary_node_domain(node_id, second, "direct").await.unwrap();
        let domains = store.list_node_domains(node_id).await.unwrap();
        let primaries: Vec<&str> = domains
            .iter()
            .filter(|d| d.kind == "direct" && d.is_primary)
            .map(|d| d.domain.as_str())
            .collect();
        assert_eq!(primaries, vec!["p2.example.test"], "direct 下应只有 p2 为主域名");
    }

    /// update_access_node 改 cert_domain 应同步 node_domains 的 direct 主行。
    #[tokio::test]
    async fn test_update_access_node_syncs_primary_node_domain() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-upd".into(),
                public_host: "md-upd.example.test".into(),
                public_port: 443,
                agent_token: "md-upd-token".into(),
                cert_domain: Some("old.example.test".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .update_admin_access_node(
                node_id,
                AdminAccessNodeUpdate {
                    cert_domain: Some("new.example.test".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let domains = store.list_node_domains(node_id).await.unwrap();
        // 新 cert_domain 成为 direct 主行;旧域名不再是 direct 主域名。
        assert!(
            domains.iter().any(|d| d.domain == "new.example.test" && d.kind == "direct" && d.is_primary),
            "更新后 new.example.test 应为 direct 主域名,实际: {domains:?}"
        );
        assert!(
            !domains.iter().any(|d| d.domain == "old.example.test" && d.is_primary),
            "旧域名不应仍是主域名"
        );
    }

    /// cf_enabled 读取改为按 node_domains 派生:节点 cf_enabled 列为 false,
    /// 但通过 add_node_domain 加了 kind='cf' 行后,读模型应报 cf_enabled=true。
    #[tokio::test]
    async fn test_cf_enabled_read_derives_from_node_domains() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        // 建节点不带 cf_domain,cf_enabled 列派生为 false。
        let node_id = store
            .create_admin_access_node(AdminAccessNodeInput {
                name: "md-cfderive".into(),
                public_host: "md-cfderive.example.test".into(),
                public_port: 443,
                agent_token: "md-cfderive-token".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let before = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
        assert!(!before.cf_enabled, "未加 cf 域名前 cf_enabled 应为 false");
        // 仅通过 node_domains 加一个 cf 域名(不动 access_nodes.cf_domain 列)。
        store
            .add_node_domain(
                node_id,
                AddNodeDomainInput {
                    domain: "derive.example.test".into(),
                    kind: "cf".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let after = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
        assert!(after.cf_enabled, "加了 kind=cf 行后 cf_enabled 应派生为 true");
    }
