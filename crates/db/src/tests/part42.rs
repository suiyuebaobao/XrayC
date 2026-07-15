/// 数据库测试分片 42。
// 本文件覆盖入口管理和绑定节点迁移的基础兼容性。
// 迁移后旧 access_lines 必须能映射到新的绑定节点表。
// 分组的新成员表必须能从旧出口成员关系回填。
// 测试只使用隔离 PostgreSQL，不访问远端节点。
// DATABASE_URL 缺失时只输出脱敏跳过原因。
// 断言不依赖真实服务器地址或私有凭据。
// 后续入口管理 CRUD 测试优先追加到本分片。
// 本文件通过父模块 include 聚合，不定义独立模块。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_entry_management_migration_preserves_existing_access_lines_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL entry management migration test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let missing_binding_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM access_lines al
        WHERE NOT EXISTS (
            SELECT 1
            FROM access_entry_exit_bindings b
            WHERE b.id = al.id
        )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(missing_binding_count, 0);

    let missing_entry_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM access_entry_exit_bindings b
        WHERE NOT EXISTS (
            SELECT 1
            FROM access_entries e
            WHERE e.id = b.access_entry_id
        )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(missing_entry_count, 0);

    let migrated_group_binding_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM line_group_binding_nodes
        WHERE line_group_id = $1
          AND entry_exit_binding_id = $2
        "#,
    )
    .bind(uuid("00000000-0000-0000-0000-000000000601"))
    .bind(uuid("00000000-0000-0000-0000-000000000501"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(migrated_group_binding_count, 1);
}

#[tokio::test]
async fn test_pg_entry_management_creates_binding_node_for_group_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL entry binding node create test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("entry-binding-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.10".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("entry-binding-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "entry-binding-resource".to_string(),
                    endpoint_name: "entry-binding-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.10".to_string(),
                    port: 38_181,
                    outbound_config: json!({"username": "entry-user", "password": "entry-pass"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "cdn-vless-ws-entry".to_string(),
            listen_host: String::new(),
            // CF 入口端口必须 ∈ CF 支持的 HTTPS 端口集合(2053 在内),否则被端口护栏拦下。
            listen_port: 2053,
            protocol: "vless".to_string(),
            transport: "ws".to_string(),
            security: "tls".to_string(),
            server_name: "cdn-entry.example.test".to_string(),
            ws_path: "/vless-ws".to_string(),
            ws_host: "cdn-entry.example.test".to_string(),
            cdn_enabled: true,
            cdn_provider: "cloudflare".to_string(),
            cdn_hostname: "cdn-entry.example.test".to_string(),
            cdn_server: "cdn-entry.example.test".to_string(),
            enabled: true,
            sort_weight: 100,
                    node_domain_id: None,
            vless_quantum_encryption: false,
        })
        .await
        .unwrap();
    let binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id,
                name: "cdn-vless-ws-binding".to_string(),
                enabled: true,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();
    let group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("entry-binding-group-{}", Uuid::new_v4().simple()),
            country_code: "US".to_string(),
            icon: String::new(),
            group_level: None,
            parent_group_id: None,
            sort_weight: Some(100),
            billing_multiplier: None,
            enabled: Some(true),
            dedicated_rules: None,
            rule_set_bindings: None,
        })
        .await
        .unwrap();

    let count = store
        .replace_admin_line_group_binding_nodes(group_id, vec![binding_id])
        .await
        .unwrap();
    assert_eq!(count, 1);

    let runtime_line = sqlx::query_as::<_, (Uuid, String, String, String, String, String)>(
        r#"
        SELECT exit_endpoint_id, listen_host, protocol, transport, xhttp_path, xhttp_host
        FROM access_lines
        WHERE id = $1
        "#,
    )
    .bind(binding_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(runtime_line.0, exit_endpoint_id);
    // CDN 入口：listen_host 物化为 cdn_hostname（订阅据此公布 CDN 域名），不再是节点 public_host。
    assert_eq!(runtime_line.1, "cdn-entry.example.test");
    assert_eq!(runtime_line.2, "vless");
    assert_eq!(runtime_line.3, "ws");
    assert_eq!(runtime_line.4, "/vless-ws");
    assert_eq!(runtime_line.5, "cdn-entry.example.test");

    let group_binding_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM line_group_binding_nodes
        WHERE line_group_id = $1
          AND entry_exit_binding_id = $2
        "#,
    )
    .bind(group_id)
    .bind(binding_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(group_binding_count, 1);

    let routing = store.access_routing_json().await.unwrap();
    let group_json = routing["line_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(&group_id.to_string()))
        .unwrap();
    assert_eq!(
        group_json["binding_node_ids"].as_array().unwrap()[0],
        binding_id.to_string()
    );

    store
        .replace_admin_line_group_lines(group_id, Vec::new())
        .await
        .unwrap();
    let group_binding_count_after_legacy_update = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM line_group_binding_nodes
        WHERE line_group_id = $1
          AND entry_exit_binding_id = $2
        "#,
    )
    .bind(group_id)
    .bind(binding_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(group_binding_count_after_legacy_update, 1);
}

#[tokio::test]
async fn test_pg_access_entry_reality_generates_public_fields_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL entry Reality field test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("entry-reality-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.12".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("entry-reality-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "entry-reality-resource".to_string(),
                    endpoint_name: "entry-reality-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.12".to_string(),
                    port: 38_182,
                    outbound_config: json!({"username": "entry-user", "password": "entry-pass"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "vless-reality-entry".to_string(),
            listen_host: String::new(),
            listen_port: 44_382,
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
                    node_domain_id: None,
            vless_quantum_encryption: false,
        })
        .await
        .unwrap();
    let binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id,
                name: String::new(),
                enabled: true,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();

    let fields = sqlx::query_as::<_, (String, String, serde_json::Value)>(
        "SELECT public_key, short_id, inbound_config FROM access_lines WHERE id = $1",
    )
    .bind(binding_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let opened_inbound = fields.2;
    assert!(!fields.0.is_empty());
    assert!(!fields.1.is_empty());
    assert_eq!(opened_inbound["security"], "reality");
    assert!(!opened_inbound["private_key"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .is_empty());

    let entry_inbound = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT inbound_config FROM access_entries WHERE id = $1",
    )
    .bind(entry_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE access_lines SET inbound_config = $2 WHERE id = $1")
        .bind(binding_id)
        .bind(entry_inbound)
        .execute(store.pool())
        .await
        .unwrap();
    // 字段加密已移除，入口配置明文存取；把入口态配置直接拷进线路态后，
    // repair 仍负责按入口态重新同步线路态入站配置。
    let repaired = store
        .repair_access_entry_binding_line_inbound_configs()
        .await
        .unwrap();
    assert!(repaired >= 1);
    let repaired_inbound =
        sqlx::query_scalar::<_, serde_json::Value>("SELECT inbound_config FROM access_lines WHERE id = $1")
            .bind(binding_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let opened_repaired = repaired_inbound;
    assert_eq!(opened_repaired["security"], "reality");
    assert!(!opened_repaired["private_key"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .is_empty());
}

/// 回归：删除入口出口绑定后，分组的 `line_group_binding_nodes` 不得残留悬挂 id，
/// 且即便库里残留一条指向已删绑定的悬挂行，读模型(管理端 + 订阅同源)也必须把它过滤掉。
/// 钉死 bug：悬挂 id 会让分组唯一线路全不可见 → 订阅整单 422「没有可用线路」。
#[tokio::test]
async fn test_pg_line_group_binding_nodes_drop_dangling_after_binding_delete_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL dangling binding node filter test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let node_id = store
        .create_admin_access_node(AdminAccessNodeInput {
            name: format!("dangling-node-{}", Uuid::new_v4().simple()),
            public_host: "203.0.113.30".to_string(),
            public_port: 443,
            remark: String::new(),
            agent_token: format!("dangling-token-{}", Uuid::new_v4().simple()),
            ..Default::default()
        })
        .await
        .unwrap();
    let created = store
        .create_admin_local_exit_lines(
            node_id,
            AdminLocalExitLinesInput {
                lines: vec![AdminLocalExitLineInput {
                    resource_name: "dangling-resource".to_string(),
                    endpoint_name: "dangling-endpoint".to_string(),
                    region_code: "US".to_string(),
                    outbound_type: "socks".to_string(),
                    network_mode: "tcp".to_string(),
                    host: "198.51.100.30".to_string(),
                    port: 38_190,
                    outbound_config: json!({"username": "u", "password": "p"}),
                    stream_config: json!({}),
                    probe_config: json!({}),
                    enabled: true,
                    node_domain_id: None,
                }],
            },
        )
        .await
        .unwrap();
    let exit_endpoint_id = uuid::Uuid::parse_str(
        created["created_lines"][0]["exit_endpoint_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let entry_id = store
        .create_admin_access_entry(AdminAccessEntryInput {
            access_node_id: node_id,
            name: "dangling-vless-entry".to_string(),
            listen_host: String::new(),
            listen_port: 443,
            protocol: "vless".to_string(),
            transport: "tcp".to_string(),
            security: "reality".to_string(),
            server_name: "www.cloudflare.com".to_string(),
            ws_path: String::new(),
            ws_host: String::new(),
            cdn_enabled: false,
            cdn_provider: String::new(),
            cdn_hostname: String::new(),
            cdn_server: String::new(),
            enabled: true,
            sort_weight: 100,
            node_domain_id: None,
            vless_quantum_encryption: false,
        })
        .await
        .unwrap();
    let binding_id = store
        .create_admin_access_entry_exit_binding(
            entry_id,
            AdminAccessEntryExitBindingInput {
                exit_endpoint_id,
                name: "dangling-binding".to_string(),
                enabled: true,
                sort_weight: 100,
                remark: String::new(),
            },
        )
        .await
        .unwrap();
    let group_id = store
        .create_admin_line_group(AdminLineGroupInput {
            name: format!("dangling-group-{}", Uuid::new_v4().simple()),
            country_code: "US".to_string(),
            icon: String::new(),
            group_level: None,
            parent_group_id: None,
            sort_weight: Some(100),
            billing_multiplier: None,
            enabled: Some(true),
            dedicated_rules: None,
            rule_set_bindings: None,
        })
        .await
        .unwrap();
    store
        .replace_admin_line_group_binding_nodes(group_id, vec![binding_id])
        .await
        .unwrap();

    // 删前：分组读模型应见到这条绑定节点(对应运行态 access_line 存在)。
    let routing = store.access_routing_json().await.unwrap();
    assert_eq!(
        binding_node_ids_of_group(&routing, group_id),
        vec![binding_id]
    );

    // 制造真实悬挂态：绑定与 line_group_binding_nodes 行仍在(FK 约束不允许直接插无主行)，
    // 但其运行态 access_line 被清掉——这正是出口池剪枝/出口删除等路径会留下的中间态。
    // `access_lines.id == access_entry_exit_bindings.id`，订阅靠 access_lines 才能渲染线路；
    // 缺了 access_line，这个 binding id 在订阅侧无对应线路 → 分组唯一线路不可见 → 整单 422。
    let deleted = sqlx::query("DELETE FROM access_lines WHERE id = $1")
        .bind(binding_id)
        .execute(store.pool())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(deleted, 1, "运行态 access_line 已删，binding 与绑定行仍残留库里");

    // 管理端读模型：悬挂 id 被过滤，分组绑定节点为空(前端反选不会卡在失效项上)。
    let routing_dangling = store.access_routing_json().await.unwrap();
    assert!(
        binding_node_ids_of_group(&routing_dangling, group_id).is_empty(),
        "缺运行态 access_line 的悬挂 id 必须被管理端读模型过滤"
    );
    // 订阅/计费同源读模型：分组 binding_node_ids 不含悬挂 id，避免整单 422。
    let data = store.load_store_data().await.unwrap();
    let group = data.line_groups.get(&group_id).unwrap();
    assert!(
        group.binding_node_ids.is_empty(),
        "订阅读模型必须过滤悬挂 id，否则分组无可用线路触发 422"
    );
}

/// 从 access-routing 读模型 JSON 里取指定分组的 binding_node_ids(测试辅助)。
fn binding_node_ids_of_group(routing: &serde_json::Value, group_id: Uuid) -> Vec<Uuid> {
    routing["line_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["id"].as_str() == Some(&group_id.to_string()))
        .and_then(|group| group["binding_node_ids"].as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect()
        })
        .unwrap_or_default()
}
