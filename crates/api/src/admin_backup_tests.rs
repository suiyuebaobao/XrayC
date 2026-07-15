//! 数据库备份管理后台接口 HTTP 层测试(spec §11 / §12)。
//! 覆盖:GET config 脱敏 + 鉴权(admin 可读、普通用户 403、匿名 401);
//! PUT config 写时保留(留空密码不覆盖旧值)并返回脱敏;run-now 合法 mode 写入
//! run_now 标记、非法 mode 返回 400;test-and-provision 非 IPv4 host 被拒(不真连
//! SSH)且响应脱敏不回显密码;state 返回 {full,offsite,wal,email} 契约结构。
//! 测试走真实 PG 迁移 + demo 管理员登录,缺 DATABASE_URL 时跳过避免本地误报。
//! 断言只看状态码、脱敏响应与结构,绝不打印/断言任何明文密码或真实主机。
//! 所有主机/邮箱用 example.test 占位;真实 provision 属 Task15,不在此测。
//! 文件前十行中文注释满足仓库规则,单文件控制在 550 行硬上限内。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

// 登录返回 access_token(账号/密码由调用方给,便于测 admin 与普通用户两种身份)。
async fn login_token(app: Router, account: &str, password: &str) -> Option<String> {
    let login = request_json(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": account, "password": password}),
    )
    .await;
    if login.0 != StatusCode::OK {
        return None;
    }
    login.1["data"]["access_token"]
        .as_str()
        .map(|value| value.to_string())
}

#[tokio::test]
async fn test_pg_admin_backup_config_get_redacts_and_requires_admin() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping backup config GET auth/redaction test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    // 预置一个密码,GET 后应脱敏成 ssh_password_set:true(而非明文)。
    store
        .update_backup_config_json(json!({"offsite": {"ssh_password": "pw-secret-xyz"}}))
        .await
        .unwrap();

    let email = format!("backup-cfg-{}@example.test", Uuid::new_v4().simple());
    store
        .register_user(&email, "user-password-123")
        .await
        .unwrap();
    let app = app(AppState::with_pg(store));

    // 匿名(无 Bearer)→ 401。
    let anon = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/backup/config",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(anon.0, StatusCode::UNAUTHORIZED, "{}", anon.1);

    // 普通用户 → 403。
    let user_token = login_token(app.clone(), &email, "user-password-123")
        .await
        .unwrap();
    let forbidden = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/backup/config",
        Some(&user_token),
        Value::Null,
    )
    .await;
    assert_eq!(forbidden.0, StatusCode::FORBIDDEN, "{}", forbidden.1);

    // 管理员 → 200 且脱敏:出 <field>_set、无明文密码/口令。
    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();
    let ok = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/backup/config",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(ok.0, StatusCode::OK, "{}", ok.1);
    let data = &ok.1["data"];
    assert_eq!(data["offsite"]["ssh_password_set"], json!(true));
    assert!(data["offsite"].get("ssh_password").is_none());
    assert!(data["email"].get("attach_passphrase").is_none());
    assert!(data["email"]["attach_passphrase_set"].is_boolean());
    assert!(
        !ok.1.to_string().contains("pw-secret-xyz"),
        "响应不得回显明文密码"
    );
}

#[tokio::test]
async fn test_pg_admin_backup_config_put_preserves_secrets() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping backup config PUT preserve test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();

    // 首写:带真实密码 → 返回脱敏 ssh_password_set:true、无明文。
    let first = request_json(
        app.clone(),
        Method::PUT,
        "/api/admin/backup/config",
        Some(&admin_token),
        json!({"offsite": {"enabled": true, "ssh_host": "192.0.2.20", "ssh_password": "put-pw-1"}}),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first.1["data"]["offsite"]["ssh_password_set"], json!(true));
    assert!(first.1["data"]["offsite"].get("ssh_password").is_none());
    assert!(
        !first.1.to_string().contains("put-pw-1"),
        "响应不得回显明文密码"
    );

    // 二写:留空密码 → 写时保留旧值(仍 _set:true),其它字段可改。
    let second = request_json(
        app.clone(),
        Method::PUT,
        "/api/admin/backup/config",
        Some(&admin_token),
        json!({"offsite": {"ssh_password": "", "ssh_port": 2022}}),
    )
    .await;
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(second.1["data"]["offsite"]["ssh_password_set"], json!(true));
    assert_eq!(second.1["data"]["offsite"]["ssh_port"], json!(2022));

    // 直接读库核实:留空未覆盖旧明文(以写入侧真实值为准)。
    let raw = store.backup_config_json().await.unwrap();
    assert_eq!(raw["offsite"]["ssh_password"], json!("put-pw-1"));
}

#[tokio::test]
async fn test_pg_admin_backup_run_now_writes_marker_and_rejects_bad_mode() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping backup run-now marker test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();

    // 非法 mode → 400。
    let bad = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/backup/run-now?mode=bogus",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST, "{}", bad.1);

    // 缺 mode → 400。
    let missing = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/backup/run-now",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(missing.0, StatusCode::BAD_REQUEST, "{}", missing.1);

    // 合法 mode → 200,run_now 标记落 database_backup_state(直接读库核实)。
    let ok = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/backup/run-now?mode=full",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(ok.0, StatusCode::OK, "{}", ok.1);
    assert_eq!(ok.1["data"]["ok"], json!(true));
    let raw = store.database_backup_state_json().await.unwrap();
    assert_eq!(raw["run_now"]["mode"], json!("full"));
    assert!(
        raw["run_now"]["requested_at"].as_str().is_some(),
        "run_now 须带 requested_at 时间戳"
    );
}

#[tokio::test]
async fn test_pg_admin_backup_test_and_provision_rejects_non_ipv4_host() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping backup provision non-ipv4 test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();

    // 域名 host + 带密码:crate 先校验 IPv4 直接拒(不真连 SSH),ok:false。
    let result = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/backup/test-and-provision",
        Some(&admin_token),
        json!({"ssh_host": "backup.example.test", "ssh_password": "provision-pw-secret"}),
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    assert_eq!(result.1["data"]["ok"], json!(false));
    assert!(
        result.1["data"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("IPv4"),
        "非 IPv4 host 应给出 IPv4 提示: {}",
        result.1
    );
    // 响应绝不回显请求密码;失败不置 pubkey_installed。
    assert!(!result.1.to_string().contains("provision-pw-secret"));
    let raw = store.backup_config_json().await.unwrap();
    assert_eq!(raw["offsite"]["pubkey_installed"], json!(false));
}

#[tokio::test]
async fn test_pg_admin_backup_state_returns_contract_shape() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping backup state shape test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let admin_token = login_token(app.clone(), "admin", xrayc_db::DEMO_ADMIN_PASSWORD)
        .await
        .unwrap();

    let ok = request_json(
        app.clone(),
        Method::GET,
        "/api/admin/backup/state",
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(ok.0, StatusCode::OK, "{}", ok.1);
    let data = &ok.1["data"];
    // 三方式键齐全,每方式带默认骨架字段(异地已并入 full/wal,无独立 offsite 档)。
    for mode in ["full", "wal", "email"] {
        assert!(data[mode].is_object(), "缺方式 {mode}: {}", ok.1);
        assert!(data[mode]
            .as_object()
            .unwrap()
            .contains_key("last_success_at"));
    }
    // 异地结果并入 full/wal:骨架带 offsite_synced 占位;不再有独立 offsite 顶层键。
    assert!(data["full"]
        .as_object()
        .unwrap()
        .contains_key("offsite_synced"));
    assert!(
        data.get("offsite").is_none(),
        "不应有独立 offsite 顶层键: {}",
        ok.1
    );
    // run_now 键存在(初始为 null)。
    assert!(data.as_object().unwrap().contains_key("run_now"));
}

// 复用现有 axum-test oneshot 范式:构造带可选 Bearer 的 JSON 请求,返回 (状态码, JSON)。
async fn request_json(
    app: Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .oneshot(
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (status, value)
}
