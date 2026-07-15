/// 数据库测试分片 47。
// 本文件覆盖中转节点多模式 CF 字段的迁移与读写回归。
// 测试只使用示例域名、RFC 文档地址和测试 UUID。
// 节点新增 cert_domain/acme_email/cf_enabled/cf_domain/cf_cert_mode 列。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因，不当作通过。
// 父级 tests 模块提供 PgStore、输入类型、json 和 pg_test_guard。
// 新增节点 CF 字段读写回归优先放在这里，避免旧分片超长。
// 不在这里写真实服务器、token、订阅或代理凭据。
// 读回断言以写入侧落库为唯一真相，符合跨层对齐红线。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_access_nodes_table_has_multimode_cf_columns_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node multimode CF columns migration test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 断言 5 个新列存在
    let columns = sqlx::query_scalar::<_, String>(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'access_nodes'
          AND column_name IN ('cert_domain', 'acme_email', 'cf_enabled', 'cf_domain', 'cf_cert_mode')
        ORDER BY column_name
        "#,
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(
        columns,
        vec![
            "acme_email".to_string(),
            "cert_domain".to_string(),
            "cf_cert_mode".to_string(),
            "cf_domain".to_string(),
            "cf_enabled".to_string(),
        ]
    );

    // 断言 cf_enabled 默认 false、cf_cert_mode 默认 'reuse_direct'
    let cf_enabled_default = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT column_default
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'access_nodes'
          AND column_name = 'cf_enabled'
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .unwrap_or_default();
    assert!(
        cf_enabled_default.starts_with("false"),
        "cf_enabled default should be false, got {cf_enabled_default}"
    );

    let cf_cert_mode_default = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT column_default
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'access_nodes'
          AND column_name = 'cf_cert_mode'
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .unwrap_or_default();
    assert!(
        cf_cert_mode_default.contains("reuse_direct"),
        "cf_cert_mode default should be reuse_direct, got {cf_cert_mode_default}"
    );
}

#[tokio::test]
async fn test_create_access_node_persists_cert_and_cf_fields_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node cert/CF persist test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "cf-multimode-node".to_string(),
            public_host: "cf-multimode.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-cf-multimode-token".to_string(),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("admin@example.test".to_string()),
            cf_enabled: true,
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(fields.cert_domain.as_deref(), Some("direct.example.test"));
    assert_eq!(fields.acme_email.as_deref(), Some("admin@example.test"));
    assert!(fields.cf_enabled);
    assert_eq!(fields.cf_domain.as_deref(), Some("cdn.example.test"));
    // 有 cf_domain 时写入侧派生 cf_cert_mode=reuse_direct(token-less 免 token,复用直连灰云证书);
    // acme 不再当 dns01 判据,要 dns01 由运维在 cf 域名上显式设。
    assert_eq!(fields.cf_cert_mode, "reuse_direct");
}

#[tokio::test]
async fn test_create_access_node_without_cf_fields_defaults_disabled_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node CF default test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 负向：不传 CF 字段时创建成功，cf_enabled=false、cf_domain 为空
    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "direct-only-node".to_string(),
            public_host: "direct-only.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-direct-only-token".to_string(),
            cert_domain: None,
            acme_email: None,
            cf_enabled: false,
            cf_domain: None,
            ip_direct_address: None,
        })
        .await
        .unwrap();

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(fields.cert_domain, None);
    assert_eq!(fields.acme_email, None);
    assert!(!fields.cf_enabled);
    assert_eq!(fields.cf_domain, None);
    assert_eq!(fields.cf_cert_mode, "reuse_direct");
}

#[tokio::test]
async fn test_update_access_node_persists_cert_and_cf_fields_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node CF update test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "cf-update-node".to_string(),
            public_host: "cf-update.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-cf-update-token".to_string(),
            cert_domain: None,
            acme_email: None,
            cf_enabled: false,
            cf_domain: None,
            ip_direct_address: None,
        })
        .await
        .unwrap();

    store
        .update_admin_access_node(
            node_id,
            AdminAccessNodeUpdate {
                cert_domain: Some("updated.example.test".to_string()),
                acme_email: Some("ops@example.test".to_string()),
                cf_enabled: Some(true),
                cf_domain: Some("updated-cdn.example.test".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let fields = store.access_node_cf_fields(node_id).await.unwrap().unwrap();
    assert_eq!(fields.cert_domain.as_deref(), Some("updated.example.test"));
    assert_eq!(fields.acme_email.as_deref(), Some("ops@example.test"));
    assert!(fields.cf_enabled);
    assert_eq!(fields.cf_domain.as_deref(), Some("updated-cdn.example.test"));
}

#[tokio::test]
async fn test_access_routing_json_node_includes_cf_fields_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL node CF read-model JSON test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: "cf-readmodel-node".to_string(),
            public_host: "cf-readmodel.example.test".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: "test-cf-readmodel-token".to_string(),
            cert_domain: Some("direct.example.test".to_string()),
            acme_email: Some("admin@example.test".to_string()),
            cf_enabled: true,
            cf_domain: Some("cdn.example.test".to_string()),
            ip_direct_address: None,
        })
        .await
        .unwrap();

    // 控制面节点摘要 JSON 必须回带 CF 字段,供前端编辑回显与入口橙云识别联动。
    let routing = store.access_routing_json().await.unwrap();
    let nodes = routing["access_nodes"]
        .as_array()
        .expect("access_nodes array present");
    let node = nodes
        .iter()
        .find(|node| node["id"].as_str() == Some(node_id.to_string().as_str()))
        .expect("created node present in routing json");
    assert_eq!(node["cf_enabled"].as_bool(), Some(true));
    assert_eq!(node["cert_domain"].as_str(), Some("direct.example.test"));
    assert_eq!(node["cf_domain"].as_str(), Some("cdn.example.test"));
    // 有 cf_domain 时写入侧派生 cf_cert_mode=reuse_direct(token-less 免 token)。
    assert_eq!(node["cf_cert_mode"].as_str(), Some("reuse_direct"));
}
