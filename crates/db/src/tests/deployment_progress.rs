// 部署状态回归：远端成功不等于平台登记完成；取消后不能由迟到报告复活任务。

#[tokio::test]
async fn test_pg_deployment_progress_waits_for_registration_and_preserves_result() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (task, token) = store.create_deployment_task(CreateDeploymentTaskInput {
        kind: "agent_install".to_string(), target_type: "access_agent".to_string(), target_id: None,
        title: "安装状态回归".to_string(), summary: String::new(),
        safe_metadata: json!({"mode":"one_click"}),
        steps: json!([{"key":"agent_ready","status":"pending"},{"key":"node_registered","status":"pending"}]),
        created_by_user_id: None,
    }).await.unwrap();
    let id = Uuid::parse_str(task["id"].as_str().unwrap()).unwrap();
    let remote = store
        .record_deployment_task_report(DeploymentTaskReportInput {
            task_id: id,
            report_token: token.clone(),
            status: Some("succeeded".to_string()),
            step: Some("agent_ready".to_string()),
            message: Some("安装脚本完成".to_string()),
            progress_percent: Some(100),
            result: Some(json!({"installation":"complete"})),
        })
        .await
        .unwrap();
    assert_eq!(remote["status"], "running");
    assert_eq!(remote["progress_percent"], 95);
    assert_eq!(remote["steps"][1]["status"], "pending");
    assert!(store.touch_active_deployment_task(id).await.unwrap());
    assert!(store.deployment_task_allows_registration(id).await.unwrap());
    let registered = store
        .record_internal_deployment_task_report(DeploymentTaskReportInput {
            task_id: id,
            report_token: token,
            status: Some("succeeded".to_string()),
            step: Some("node_registered".to_string()),
            message: Some("节点登记完成".to_string()),
            progress_percent: Some(100),
            result: Some(json!({"registration":"complete"})),
        })
        .await
        .unwrap();
    assert_eq!(registered["status"], "succeeded");
    assert_eq!(registered["result"]["installation"], "complete");
    assert_eq!(registered["result"]["registration"], "complete");
}

#[tokio::test]
async fn test_pg_deployment_cancellation_cannot_be_overwritten_by_late_success() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let (task, token) = store
        .create_deployment_task(CreateDeploymentTaskInput {
            kind: "agent_install".to_string(),
            target_type: "access_agent".to_string(),
            target_id: None,
            title: "取消回归".to_string(),
            summary: String::new(),
            safe_metadata: json!({"mode":"one_click"}),
            steps: json!([]),
            created_by_user_id: None,
        })
        .await
        .unwrap();
    let id = Uuid::parse_str(task["id"].as_str().unwrap()).unwrap();
    store.cancel_deployment_task(id).await.unwrap();
    assert!(!store.touch_active_deployment_task(id).await.unwrap());
    assert!(!store.deployment_task_allows_registration(id).await.unwrap());
    let late = store
        .record_internal_deployment_task_report(DeploymentTaskReportInput {
            task_id: id,
            report_token: token,
            status: Some("succeeded".to_string()),
            step: Some("node_registered".to_string()),
            message: None,
            progress_percent: Some(100),
            result: None,
        })
        .await;
    assert!(late.is_err());
}
