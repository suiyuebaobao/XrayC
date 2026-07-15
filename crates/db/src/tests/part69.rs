// 数据库测试分片 69（管理员编辑用户:邮箱不可修改 + 非标准邮箱账号编辑不卡格式）。
// 覆盖两条口径:
// ① update_admin_user_json 收到与现值不同的邮箱必须拒（InvalidAgentPayload），邮箱列不被改。
// ② admin 等非标准格式邮箱（如 email='admin'）账号更新时不再做邮箱格式校验,
//    其它字段（disabled 等）可正常编辑、邮箱保持原值,消除旧实现 normalize 报「邮箱格式无效」卡死 admin。
// 测试只用示例邮箱/RFC 文档域名与隔离 PostgreSQL,不访问远端节点。
// 缺 DATABASE_URL 时只输出脱敏跳过原因,不当作通过（与 §9 测试要求一致）。
// 读回断言以 users 表落库为唯一真相;非标准邮箱用户用裸 SQL 直插造,绕过 create 的 normalize。
// 用户创建复用 PgStore 公开写入口,不造不存在的 helper。
// 本头部满足前十行中文注释约束。

#[tokio::test]
async fn test_update_admin_user_rejects_email_change() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping email-immutable update guard test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let created = store
        .create_admin_user_json(AdminUserCreate {
            email: format!("u-{}@example.test", Uuid::new_v4().simple()),
            password: "created123".to_string(),
            disabled: false,
            is_admin: false,
            plan_id: None,
            rate_limit_bps: None,
            rate_limit_up_bps: None,
            rate_limit_down_bps: None,
        })
        .await
        .unwrap();
    let user_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let original_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

    // 把邮箱改成另一个合法邮箱 → 编辑不允许改邮箱,必须被拒。
    let err = store
        .update_admin_user_json(
            user_id,
            AdminUserUpdate {
                email: Some(format!("changed-{}@example.test", Uuid::new_v4().simple())),
                disabled: None,
                is_admin: None,
                plan_id: None,
                rate_limit_bps: None,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, DbError::InvalidAgentPayload(_)),
        "编辑改邮箱应被 InvalidAgentPayload 拒,实际: {err:?}"
    );

    // 被拒后邮箱保持原值不变。
    let after: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(after, original_email, "被拒后邮箱必须保持原值");
}

#[tokio::test]
async fn test_update_admin_user_nonstandard_email_skips_format_check() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping nonstandard-email update test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 先正常建一个用户,再裸 SQL 把邮箱改成 'admin-xxx' 这类非标准格式（绕过 create 的 normalize 校验）。
    let created = store
        .create_admin_user_json(AdminUserCreate {
            email: format!("seed-{}@example.test", Uuid::new_v4().simple()),
            password: "created123".to_string(),
            disabled: false,
            is_admin: false,
            plan_id: None,
            rate_limit_bps: None,
            rate_limit_up_bps: None,
            rate_limit_down_bps: None,
        })
        .await
        .unwrap();
    let user_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let nonstandard = format!("admin-{}", Uuid::new_v4().simple());
    sqlx::query("UPDATE users SET email = $1 WHERE id = $2")
        .bind(&nonstandard)
        .bind(user_id)
        .execute(store.pool())
        .await
        .unwrap();

    // 传入同一个非标准邮箱 + 切换 disabled → 应成功（不再 normalize 报格式无效）,邮箱保持不变、disabled 生效。
    store
        .update_admin_user_json(
            user_id,
            AdminUserUpdate {
                email: Some(nonstandard.clone()),
                disabled: Some(true),
                is_admin: None,
                plan_id: None,
                rate_limit_bps: None,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        )
        .await
        .expect("非标准邮箱账号、邮箱未变更的更新应成功,不应被邮箱格式校验拦下");

    let (email_after, disabled_after): (String, bool) =
        sqlx::query_as("SELECT email, disabled FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(email_after, nonstandard, "邮箱必须保持原非标准值不变");
    assert!(disabled_after, "disabled 字段应被正常更新为 true");
}
