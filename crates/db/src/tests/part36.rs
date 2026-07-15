/// 数据库测试分片 36。
// 本文件覆盖分组专用订阅规则与套餐默认出口分组。
// 测试只使用本地 DATABASE_URL 指向的测试库，不访问外部网络。
// 订阅 YAML 断言只检查规则顺序和分组目标。
// 分组专用规则应该优先于订阅通用规则输出。
// 套餐默认出口分组应该成为最终 MATCH 的目标。
// 规则字段默认空数组，旧数据不需要迁移填值。
// 默认出口字段可空，旧套餐会回退到第一个可用分组。
// 本文件不保存密钥、服务器地址或外部凭据。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_pg_group_dedicated_rules_and_plan_default_group_drive_subscription_yaml_when_database_url_is_set(
) {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL group dedicated rules test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let plan_id = uuid("00000000-0000-0000-0000-000000000101");
    let line_group_id = uuid("00000000-0000-0000-0000-000000000601");
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
                dedicated_rules: Some(vec!["DOMAIN-SUFFIX,openai.com".to_string()]),
                rule_set_bindings: None,
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
            "default_rules": ["DOMAIN-SUFFIX,cn,DIRECT"]
        }))
        .await
        .unwrap();

    let admin_plans = store.admin_plans_json().await.unwrap();
    let plan = admin_plans["plans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plan| plan["id"] == plan_id.to_string())
        .unwrap();
    assert_eq!(plan["default_line_group_id"], line_group_id.to_string());

    let routing = store.access_routing_json().await.unwrap();
    let group = routing["line_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["id"] == line_group_id.to_string())
        .unwrap();
    assert_eq!(group["dedicated_rules"][0], "DOMAIN-SUFFIX,openai.com");

    let yaml = store.generate_subscription_yaml("demo-token").await.unwrap();
    assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,AI分组"));
    assert!(yaml.contains("DOMAIN-SUFFIX,cn,DIRECT"));
    assert!(yaml.contains("MATCH,AI分组"));
    assert!(yaml.find("DOMAIN-SUFFIX,openai.com,AI分组") < yaml.find("DOMAIN-SUFFIX,cn,DIRECT"));
}
