// 数据库测试分片 54。
// 本文件覆盖部署任务删除/取消与卡死自愈(Phase 7)。
// delete:删任意任务记录,删后查列表不再出现该任务。
// cancel:仅非终态任务(waiting_for_server/running)标记 failed,原因"管理员取消";
// 已 succeeded 的终态任务取消不改其状态,保证终态不可回退。
// mark_stale:created_at/updated_at 超阈值的非终态任务标 failed,原因"超时未回连";
// 近期任务与 succeeded/failed 终态任务都不动,避免误杀正常进行中的安装。
// 测试只用测试 token、示例元数据,不写真实服务器资产。
// 缺少 DATABASE_URL 时只输出脱敏跳过原因,不当作通过。
// 本头部满足前十行中文注释约束。

/// 构造一条最小部署任务输入,只填必需字段,target_id 用 None。
fn stale_test_task_input(title: &str) -> CreateDeploymentTaskInput {
    CreateDeploymentTaskInput {
        kind: "agent_install".to_string(),
        target_type: "access_agent".to_string(),
        target_id: None,
        title: title.to_string(),
        summary: "测试部署任务".to_string(),
        safe_metadata: serde_json::json!({"mode": "test"}),
        steps: serde_json::json!([
            {"key": "one_click_requested", "title": "创建安装任务", "detail": "", "status": "current"}
        ]),
        created_by_user_id: None,
    }
}

/// 从任务 JSON 里取出 id(Uuid 字符串)。
fn task_id_from_json(task: &Value) -> Uuid {
    Uuid::parse_str(task["id"].as_str().unwrap()).unwrap()
}

/// 在部署任务列表里按 id 找到对应项的 status,找不到返回 None。
async fn task_status_in_list(store: &PgStore, task_id: Uuid) -> Option<String> {
    let list = store.deployment_tasks_json().await.unwrap();
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| task_id_from_json(item) == task_id)
        .map(|item| item["status"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn test_delete_deployment_task() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping deployment task delete test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    let (task, _token) = store
        .create_deployment_task(stale_test_task_input("待删除任务"))
        .await
        .unwrap();
    let task_id = task_id_from_json(&task);
    assert!(task_status_in_list(&store, task_id).await.is_some());

    // 删除后任务记录应彻底消失,列表里不再出现。
    store.delete_deployment_task(task_id).await.unwrap();
    assert!(
        task_status_in_list(&store, task_id).await.is_none(),
        "删除后任务不应再出现在列表"
    );
}

#[tokio::test]
async fn test_cancel_unfinished_task_marks_failed() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping deployment task cancel test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // waiting_for_server 非终态任务:取消应标记 failed。
    let (waiting, _token) = store
        .create_deployment_task(stale_test_task_input("待取消任务"))
        .await
        .unwrap();
    let waiting_id = task_id_from_json(&waiting);
    assert_eq!(waiting["status"], "waiting_for_server");

    let canceled = store.cancel_deployment_task(waiting_id).await.unwrap();
    assert_eq!(canceled["status"], "failed", "未完成任务取消后应为 failed");
    assert!(
        canceled["error_summary"]
            .as_str()
            .unwrap()
            .contains("管理员取消"),
        "取消原因应记为管理员取消"
    );
    assert_eq!(task_status_in_list(&store, waiting_id).await.as_deref(), Some("failed"));

    // 已 succeeded 的终态任务:取消不应改变其状态。
    let (succeeded, _token2) = store
        .create_deployment_task(stale_test_task_input("已完成任务"))
        .await
        .unwrap();
    let succeeded_id = task_id_from_json(&succeeded);
    sqlx::query("UPDATE deployment_tasks SET status = 'succeeded', completed_at = now() WHERE id = $1")
        .bind(succeeded_id)
        .execute(store.pool())
        .await
        .unwrap();

    let result = store.cancel_deployment_task(succeeded_id).await.unwrap();
    assert_eq!(result["status"], "succeeded", "终态任务取消不应回退状态");
    assert_eq!(
        task_status_in_list(&store, succeeded_id).await.as_deref(),
        Some("succeeded")
    );
}

#[tokio::test]
async fn test_mark_stale_waiting_task_auto_failed() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping stale deployment task auto-fail test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();

    // 久远的 waiting_for_server:created_at/updated_at 倒回两小时前,应被扫成 failed。
    let (stale, _t1) = store
        .create_deployment_task(stale_test_task_input("卡死任务"))
        .await
        .unwrap();
    let stale_id = task_id_from_json(&stale);
    sqlx::query(
        "UPDATE deployment_tasks SET created_at = now() - interval '2 hours', \
         updated_at = now() - interval '2 hours' WHERE id = $1",
    )
    .bind(stale_id)
    .execute(store.pool())
    .await
    .unwrap();

    // 近期 waiting_for_server:不应被动到。
    let (fresh, _t2) = store
        .create_deployment_task(stale_test_task_input("新建任务"))
        .await
        .unwrap();
    let fresh_id = task_id_from_json(&fresh);

    // 久远但已 succeeded:终态不应被动到。
    let (done, _t3) = store
        .create_deployment_task(stale_test_task_input("早已完成任务"))
        .await
        .unwrap();
    let done_id = task_id_from_json(&done);
    sqlx::query(
        "UPDATE deployment_tasks SET status = 'succeeded', completed_at = now(), \
         created_at = now() - interval '2 hours', updated_at = now() - interval '2 hours' \
         WHERE id = $1",
    )
    .bind(done_id)
    .execute(store.pool())
    .await
    .unwrap();

    // 阈值 900 秒(15 分钟):只有久远非终态任务被标 failed。
    let affected = store.mark_stale_deployment_tasks_failed(900).await.unwrap();
    assert!(affected >= 1, "至少应有一条卡死任务被标记 failed");

    assert_eq!(
        task_status_in_list(&store, stale_id).await.as_deref(),
        Some("failed"),
        "久远的 waiting_for_server 应被标 failed"
    );
    assert_eq!(
        task_status_in_list(&store, fresh_id).await.as_deref(),
        Some("waiting_for_server"),
        "近期任务不应被标 failed"
    );
    assert_eq!(
        task_status_in_list(&store, done_id).await.as_deref(),
        Some("succeeded"),
        "已完成的终态任务不应被改写"
    );
}
