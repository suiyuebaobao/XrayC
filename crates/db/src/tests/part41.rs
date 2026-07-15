/// 数据库测试分片 41。
// 本文件覆盖订阅规则库和线路分组绑定。
// 规则库用于替代旧的分组 dedicated_rules 直写模式。
// 测试只使用本地 DATABASE_URL 指向的测试库，不访问外部网络。
// 规则库被多个分组引用后，修改规则库应同步影响订阅生成。
// 删除已被分组绑定的规则库必须被拒绝，避免分组引用悬空。
// 旧 dedicated_rules 迁移应自动生成规则库并绑定回原分组。
// 订阅 YAML 断言只检查规则目标和分组数量，不依赖真实代理。
// 新增测试优先放在本文件，避免继续扩大旧分片。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_subscription_rule_set_crud_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL subscription rule set CRUD test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let rule_set_id = store
        .create_subscription_rule_set(AdminSubscriptionRuleSetInput {
            name: "AI 规则".to_string(),
            description: "OpenAI and Claude".to_string(),
            enabled: Some(true),
            rules: vec![
                "DOMAIN-SUFFIX,openai.com,PROXY".to_string(),
                "  ".to_string(),
                "DOMAIN-SUFFIX,baidu.com,DIRECT".to_string(),
            ],
        })
        .await
        .expect("rule set should be created");

    let rule_sets = store
        .list_subscription_rule_sets()
        .await
        .expect("rule sets should load");
    let created = rule_sets
        .iter()
        .find(|rule_set| rule_set.id == rule_set_id)
        .expect("created rule set should exist");
    assert_eq!(created.name, "AI 规则");
    assert_eq!(created.description, "OpenAI and Claude");
    assert!(created.enabled);
    assert_eq!(
        created.rules,
        vec![
            "DOMAIN-SUFFIX,openai.com,PROXY",
            "DOMAIN-SUFFIX,baidu.com,DIRECT"
        ]
    );
    assert_eq!(created.binding_count, 0);

    store
        .update_subscription_rule_set(
            rule_set_id,
            AdminSubscriptionRuleSetInput {
                name: "AI 新规则".to_string(),
                description: "updated".to_string(),
                enabled: Some(false),
                rules: vec!["DOMAIN-SUFFIX,anthropic.com,PROXY".to_string()],
            },
        )
        .await
        .expect("rule set should update");

    let updated = store
        .list_subscription_rule_sets()
        .await
        .expect("rule sets should reload")
        .into_iter()
        .find(|rule_set| rule_set.id == rule_set_id)
        .expect("updated rule set should exist");
    assert_eq!(updated.name, "AI 新规则");
    assert_eq!(updated.description, "updated");
    assert!(!updated.enabled);
    assert_eq!(
        updated.rules,
        vec!["DOMAIN-SUFFIX,anthropic.com,PROXY"]
    );

    let deleted = store
        .delete_subscription_rule_set(rule_set_id)
        .await
        .expect("unbound rule set should delete");
    assert_eq!(deleted, 1);
}

#[tokio::test]
async fn test_pg_subscription_rule_set_binding_drives_subscription_yaml_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL subscription rule set binding test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
    let plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let rule_set_id = store
        .create_subscription_rule_set(AdminSubscriptionRuleSetInput {
            name: "绑定 AI 规则".to_string(),
            description: "用于订阅 YAML 目标重写".to_string(),
            enabled: Some(true),
            rules: vec!["DOMAIN-SUFFIX,openai.com,PROXY".to_string()],
        })
        .await
        .unwrap();

    store
        .update_admin_line_group(
            line_group_id,
            AdminLineGroupInput {
                name: "AI分组".to_string(),
                country_code: "GLOBAL".to_string(),
                icon: String::new(),
                group_level: None,
                parent_group_id: None,
                sort_weight: Some(100),
                billing_multiplier: None,
                enabled: Some(true),
                dedicated_rules: Some(vec!["DOMAIN-SUFFIX,legacy.example".to_string()]),
                rule_set_bindings: Some(vec![LineGroupRuleSetBindingInput {
                    rule_set_id,
                    position: 100,
                    enabled: true,
                }]),
            },
        )
        .await
        .unwrap();
    store
        .replace_admin_plan_line_groups_with_default(
            plan_id,
            vec![AdminPlanLineGroupInput {
                line_group_id,
                billing_multiplier: Some(1.0),
            }],
            Some(line_group_id),
        )
        .await
        .unwrap();
    store
        .update_subscription_settings_json(json!({
            "default_rules": ["MATCH,PROXY"]
        }))
        .await
        .unwrap();

    let routing = store.access_routing_json().await.unwrap();
    let group = routing["line_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["id"] == line_group_id.to_string())
        .unwrap();
    assert_eq!(group["rule_set_bindings"][0]["rule_set_id"], rule_set_id.to_string());

    let delete_result = store.delete_subscription_rule_set(rule_set_id).await;
    assert!(matches!(delete_result, Err(DbError::InvalidInput(_))));

    let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,AI分组"));
    assert!(!yaml.contains("DOMAIN-SUFFIX,legacy.example,AI分组"));
}
