// 数据库测试分片 15。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 后续移动测试时请保持 item 边界完整。
// 文件头部使用中文注释满足仓库拆分约束。
// SQL 与断言内容来自原 lib.rs 内联测试。
// 本头部满足前十行中文注释约束。

    async fn assert_pg_third_party_endpoint_heartbeat_and_subscription(
        store: &PgStore,
        case: PgOutboundCase,
    ) {
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let line_group_id = uuid("00000000-0000-0000-0000-000000000601");

        sqlx::query(
            "UPDATE access_nodes SET status = 'unknown', config_dirty = TRUE WHERE id = $1",
        )
        .bind(node_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE access_lines SET enabled = TRUE WHERE id = $1")
            .bind(line_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE exit_pools SET enabled = TRUE WHERE id = $1")
            .bind(pool_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'offline', allow_new_assignments = FALSE
            WHERE exit_pool_id = $1 AND exit_endpoint_id <> $2
            "#,
        )
        .bind(pool_id)
        .bind(case.endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_resources (
                id, name, region_code, ownership, enabled
            )
            VALUES ($1, $2, 'US', 'third_party', TRUE)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                region_code = EXCLUDED.region_code,
                ownership = EXCLUDED.ownership,
                enabled = TRUE
            "#,
        )
        .bind(case.resource_id)
        .bind(format!("third-party-{}", case.name))
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (
                id, exit_resource_id, outbound_type, host, port,
                outbound_config, enabled
            )
            VALUES ($1, $2, $3::endpoint_type, $4, $5, $6, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                exit_resource_id = EXCLUDED.exit_resource_id,
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(case.endpoint_id)
        .bind(case.resource_id)
        .bind(case.outbound_type)
        .bind(case.host)
        .bind(i32::from(case.port))
        .bind(case.config.clone())
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
                weight = EXCLUDED.weight,
                status = EXCLUDED.status,
                allow_new_assignments = EXCLUDED.allow_new_assignments
            "#,
        )
        .bind(pool_id)
        .bind(case.endpoint_id)
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
            .bind(case.endpoint_id)
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
        .bind(case.endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();

        let yaml = store
            .generate_subscription_yaml("demo-token")
            .await
            .unwrap();
        assert!(
            yaml.contains("access.example.test"),
            "{} subscription should expose only access line",
            case.name
        );
        let subscription_json = store
            .user_subscription_json_for_user(user_id)
            .await
            .unwrap()
            .to_string();
        for marker in &case.leaked_markers {
            assert!(
                !yaml.contains(marker),
                "{} leaked `{}` into subscription yaml",
                case.name,
                marker
            );
            assert!(
                !subscription_json.contains(marker),
                "{} leaked `{}` into user subscription json",
                case.name,
                marker
            );
        }

        let assigned = assigned_exit_assignment(store, user_id, line_id, pool_id).await;
        assert_eq!(assigned.exit_endpoint_id, case.endpoint_id, "{}", case.name);

        let heartbeat = store.heartbeat_json(Some(node_id), None).await.unwrap();
        let exit_tag = exit_endpoint_tag(case.endpoint_id);
        let endpoint = heartbeat["config"]["exit_endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|endpoint| endpoint["tag"] == exit_tag)
            .unwrap_or_else(|| panic!("{} endpoint should be present", case.name));
        assert_eq!(endpoint["protocol"]["type"], case.heartbeat_type);
        assert_eq!(endpoint["protocol"]["address"], case.host);
        assert_eq!(endpoint["protocol"]["port"], i32::from(case.port));
        for (field, expected) in &case.expected_protocol_fields {
            assert_eq!(
                endpoint["protocol"][*field],
                json!(expected),
                "{} heartbeat field {}",
                case.name,
                field
            );
        }
        assert!(heartbeat["config"]["routing_rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["outbound_tag"] == exit_tag));

        let config: XrayAccessConfig =
            serde_json::from_value(heartbeat["config"].clone()).expect("config should deserialize");
        let rendered =
            xrayc_xray_config::compile_xray_config(&config).expect("config should compile");
        let outbound = rendered["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == exit_tag)
            .unwrap_or_else(|| panic!("{} rendered outbound should be present", case.name));
        assert_eq!(outbound["protocol"], case.xray_protocol);
    }

    async fn cleanup_pg_third_party_endpoint_cases(
        store: &PgStore,
        endpoint_ids: &[Uuid],
        resource_ids: &[Uuid],
    ) {
        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let direct_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");

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
            .bind(direct_endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
        for endpoint_id in endpoint_ids {
            sqlx::query(
                "DELETE FROM exit_pool_members WHERE exit_pool_id = $1 AND exit_endpoint_id = $2",
            )
            .bind(pool_id)
            .bind(endpoint_id)
            .execute(store.pool())
            .await
            .unwrap();
            sqlx::query("DELETE FROM exit_endpoints WHERE id = $1")
                .bind(endpoint_id)
                .execute(store.pool())
                .await
                .unwrap();
        }
        for resource_id in resource_ids {
            sqlx::query("DELETE FROM exit_resources WHERE id = $1")
                .bind(resource_id)
                .execute(store.pool())
                .await
                .unwrap();
        }
        sqlx::query(
            r#"
            UPDATE exit_pool_members
            SET status = 'healthy', allow_new_assignments = TRUE
            WHERE exit_pool_id = $1 AND exit_endpoint_id = $2
            "#,
        )
        .bind(pool_id)
        .bind(direct_endpoint_id)
        .execute(store.pool())
        .await
        .unwrap();
    }

    async fn assigned_line_ids(store: &PgStore, user_id: Uuid, group_id: Uuid) -> Vec<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT access_line_id
            FROM user_access_line_assignments
            WHERE user_id = $1 AND line_group_id = $2
            ORDER BY assigned_at, access_line_id
            "#,
        )
        .bind(user_id)
        .bind(group_id)
        .fetch_all(store.pool())
        .await
        .unwrap()
    }

    #[derive(Debug, sqlx::FromRow)]
    struct AuditLogTestRow {
        action: String,
        resource_type: String,
        resource_id: Option<Uuid>,
        client_ip: String,
        request_summary: Value,
    }

    #[derive(Debug, sqlx::FromRow)]
    struct TestExitAssignmentRow {
        exit_endpoint_id: Uuid,
        failover_reason: String,
        assigned_at: chrono::DateTime<chrono::Utc>,
    }

    async fn assigned_exit_assignment(
        store: &PgStore,
        user_id: Uuid,
        access_line_id: Uuid,
        exit_pool_id: Uuid,
    ) -> TestExitAssignmentRow {
        sqlx::query_as::<_, TestExitAssignmentRow>(
            r#"
            SELECT exit_endpoint_id, failover_reason, assigned_at
            FROM user_exit_assignments
            WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
            "#,
        )
        .bind(user_id)
        .bind(access_line_id)
        .bind(exit_pool_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
    }
