//! 管理端「本机出口就地编辑」与「节点多域名对账」HTTP 接口测试。
//! 覆盖:节点带 domains[] 创建→node_domains 落库;入口带 node_domain_id→落库;
//! 本机出口 GET 列出、PUT 改端口、DELETE 删除一条 self_hosted 出口。
//! 测试走真实 PG 迁移 + demo 管理员登录,缺 DATABASE_URL 时跳过避免本地误报。
//! 所有域名/主机使用 example.test 占位,不输出 token/真实地址/凭据。
//! 本机出口用 socks 空配置(免证书、自动生成凭据),避免依赖证书域名。
//! 断言只检查状态码、脱敏响应与直接读库的落库结果。
//! 不在测试日志中输出任何敏感信息。
//! 文件前十行中文注释满足仓库规则。
//! 与 db crate 的写侧对齐:node_domain_id/端口以库内真实值核实。

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request};
use serde_json::{json, Value};
use tower::ServiceExt;

// 登录 demo 管理员,返回 access_token。
async fn admin_login_token(app: Router) -> String {
    let login = request_json(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        json!({"account": "admin", "password": xrayc_db::DEMO_ADMIN_PASSWORD}),
    )
    .await;
    assert_eq!(login.0, StatusCode::OK, "{}", login.1);
    login.1["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

// 用安装态鉴权码创建一个确定 ID 的中转节点,便于直接读库断言。
async fn create_node_with_known_id(app: Router, token: &str, body_extra: Value) -> Uuid {
    let installed_node_id = Uuid::new_v4();
    let installed_token = format!("le-token-{}", Uuid::new_v4().simple());
    let auth_code = format!("xrayc-agent-v1:{installed_node_id}:{installed_token}");
    let mut body = json!({
        "name": format!("le-node-{}", Uuid::new_v4().simple()),
        "public_host": "le-node.example.test",
        "public_port": 443,
        "agent_token": auth_code,
        "remark": ""
    });
    // 合并额外字段(如 domains[] / cert_domain)。
    if let (Some(base), Some(extra)) = (body.as_object_mut(), body_extra.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    let created = request_json(
        app,
        Method::POST,
        "/api/admin/access-nodes",
        Some(token),
        body,
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    installed_node_id
}

#[tokio::test]
async fn test_pg_create_node_with_domains_lands_node_domains() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping node domains create test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    // domains[] 含一个 direct 主域名(也是 cert_domain)+ 一个额外 direct 域名 + 一个 cf 域名。
    let node_id = create_node_with_known_id(
        app.clone(),
        &token,
        json!({
            "cert_domain": "primary.example.test",
            "acme_email": "ops@example.test",
            "domains": [
                {"domain": "primary.example.test", "kind": "direct", "is_primary": true},
                {"domain": "extra.example.test", "kind": "direct"},
                {"domain": "cdn.example.test", "kind": "cf"}
            ]
        }),
    )
    .await;

    // 直接读库:三条域名都应落库(主域名由 cert_domain 同步,extra/cf 由对账新增,不重复)。
    let domains = store.list_node_domains(node_id).await.unwrap();
    let names: Vec<&str> = domains.iter().map(|d| d.domain.as_str()).collect();
    assert!(
        names.contains(&"primary.example.test"),
        "domains: {names:?}"
    );
    assert!(names.contains(&"extra.example.test"), "domains: {names:?}");
    assert!(names.contains(&"cdn.example.test"), "domains: {names:?}");
    assert_eq!(domains.len(), 3, "应恰好三条域名,无重复: {names:?}");
    // cf 域名 kind 正确。
    let cf = domains
        .iter()
        .find(|d| d.domain == "cdn.example.test")
        .unwrap();
    assert_eq!(cf.kind, "cf");
}

#[tokio::test]
async fn test_pg_create_entry_with_node_domain_id_lands() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping entry node_domain_id test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    // 建一个带 direct 主域名的节点,拿到该域名 id 供入口选择。
    let node_id = create_node_with_known_id(
        app.clone(),
        &token,
        json!({
            "cert_domain": "entry-direct.example.test",
            "acme_email": "ops@example.test"
        }),
    )
    .await;
    let domains = store.list_node_domains(node_id).await.unwrap();
    let direct = domains
        .iter()
        .find(|d| d.kind == "direct")
        .expect("应有 direct 主域名");

    // 建一个 Trojan 入口并选中该 direct 域名(要证书协议需直连域名)。
    let created = request_json(
        app.clone(),
        Method::POST,
        "/api/admin/access-entries",
        Some(&token),
        json!({
            "access_node_id": node_id,
            "name": format!("entry-{}", Uuid::new_v4().simple()),
            "listen_host": "entry-direct.example.test",
            "listen_port": 443,
            "protocol": "trojan",
            "security": "tls",
            "server_name": "entry-direct.example.test",
            "node_domain_id": direct.id
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    let entry_id = created.1["data"]["id"].as_str().unwrap();
    let entry_uuid: Uuid = entry_id.parse().unwrap();

    // 直接读库:access_entries.node_domain_id 应等于选中域名。
    let stored: Option<Uuid> =
        sqlx::query_scalar("SELECT node_domain_id FROM access_entries WHERE id = $1")
            .bind(entry_uuid)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(stored, Some(direct.id), "入口应落库选中的 node_domain_id");
}

#[tokio::test]
async fn test_pg_local_exit_line_list_update_delete() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping local exit line CRUD test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    let node_id = create_node_with_known_id(app.clone(), &token, json!({})).await;

    // 建一条 socks 本机出口(免证书、自动生成凭据)。
    let created = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines"),
        Some(&token),
        json!({
            "lines": [{
                "outbound_type": "socks",
                "network_mode": "tcp",
                "host": "le.example.test",
                "port": 38001
            }]
        }),
    )
    .await;
    assert_eq!(created.0, StatusCode::OK, "{}", created.1);

    // GET 列出:应有一条,端口 38001。
    let listed = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(listed.0, StatusCode::OK, "{}", listed.1);
    let lines = listed.1["data"]["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1, "应恰好一条本机出口: {}", listed.1);
    let endpoint_id = lines[0]["exit_endpoint_id"].as_str().unwrap().to_string();
    assert_eq!(lines[0]["port"].as_i64(), Some(38001));

    // PUT 改端口为 39002。
    let updated = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines/{endpoint_id}"),
        Some(&token),
        json!({"port": 39002}),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);
    // 直接读库核实端口生效。
    let endpoint_uuid: Uuid = endpoint_id.parse().unwrap();
    let port: i32 = sqlx::query_scalar("SELECT port FROM exit_endpoints WHERE id = $1")
        .bind(endpoint_uuid)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(port, 39002, "PUT 应把端口改成 39002");

    // DELETE 删除该出口线路。
    let deleted = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines/{endpoint_id}"),
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(deleted.0, StatusCode::OK, "{}", deleted.1);
    // 读库核实端点已删。
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exit_endpoints WHERE id = $1")
        .bind(endpoint_uuid)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(remaining, 0, "DELETE 应删掉该本机出口端点");
}

#[tokio::test]
async fn test_pg_local_exit_line_put_persists_outbound_config() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping local exit PUT outbound_config test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    let node_id = create_node_with_known_id(app.clone(), &token, json!({})).await;

    // 建一条 socks 本机出口(自动生成凭据)。
    let created = request_json(
        app.clone(),
        Method::POST,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines"),
        Some(&token),
        json!({"lines": [{"outbound_type": "socks", "network_mode": "tcp",
            "host": "le.example.test", "port": 38101}]}),
    )
    .await;
    assert_eq!(created.0, StatusCode::OK, "{}", created.1);
    let listed = request_json(
        app.clone(),
        Method::GET,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines"),
        Some(&token),
        Value::Null,
    )
    .await;
    let endpoint_id = listed.1["data"]["lines"][0]["exit_endpoint_id"]
        .as_str()
        .unwrap()
        .to_string();

    // PUT 改 username/password:store 应重跑护栏后持久化新凭据。
    let updated = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}/local-exit-lines/{endpoint_id}"),
        Some(&token),
        json!({"outbound_config": {"username": "put-user", "password": "put-pass"}}),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);

    // 直接读库核实 outbound_config 已更新为新凭据(写侧为准)。
    let endpoint_uuid: Uuid = endpoint_id.parse().unwrap();
    let cfg: Value = sqlx::query_scalar("SELECT outbound_config FROM exit_endpoints WHERE id = $1")
        .bind(endpoint_uuid)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(cfg["username"].as_str(), Some("put-user"), "{cfg}");
    assert_eq!(cfg["password"].as_str(), Some("put-pass"), "{cfg}");
}

#[tokio::test]
async fn test_pg_update_node_reconciles_domains_removes_extra() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping node domains reconcile test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    // 初建带两个 direct 域名。
    let node_id = create_node_with_known_id(
        app.clone(),
        &token,
        json!({
            "cert_domain": "keep.example.test",
            "domains": [
                {"domain": "keep.example.test", "kind": "direct", "is_primary": true},
                {"domain": "drop.example.test", "kind": "direct"}
            ]
        }),
    )
    .await;
    assert_eq!(store.list_node_domains(node_id).await.unwrap().len(), 2);

    // PUT 只保留 keep.example.test:对账应删除 drop.example.test(未被引用)。
    let updated = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}"),
        Some(&token),
        json!({
            "domains": [
                {"domain": "keep.example.test", "kind": "direct", "is_primary": true}
            ]
        }),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);
    let domains = store.list_node_domains(node_id).await.unwrap();
    let names: Vec<&str> = domains.iter().map(|d| d.domain.as_str()).collect();
    assert_eq!(
        names,
        vec!["keep.example.test"],
        "对账后应只剩 keep: {names:?}"
    );
}

#[tokio::test]
async fn test_pg_update_node_empty_domains_clears_last_domain() {
    // 回归:删光域名(列表变空)必须真删掉。修复前 domains[] 用 Vec 区分不了「没传」与「传了空[]」,
    // 空清单被误判为「不改域名表」→ 节点最后一个域名永远删不掉(用户报「删不掉」);修复后空清单=删光。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping clear-last-domain test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;

    // 初建仅一个 direct 域名(节点的唯一域名)。
    let node_id = create_node_with_known_id(
        app.clone(),
        &token,
        json!({
            "cert_domain": "only.example.test",
            "domains": [
                {"domain": "only.example.test", "kind": "direct", "is_primary": true}
            ]
        }),
    )
    .await;
    assert_eq!(store.list_node_domains(node_id).await.unwrap().len(), 1);

    // 前端「删光域名」的真实载荷:回传节点单字段(cert_domain 清空)+ 空 domains[]。
    let updated = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}"),
        Some(&token),
        json!({
            "name": "renamed-node",
            "cert_domain": "",
            "cf_domain": "",
            "domains": []
        }),
    )
    .await;
    assert_eq!(updated.0, StatusCode::OK, "{}", updated.1);
    assert!(
        store.list_node_domains(node_id).await.unwrap().is_empty(),
        "删光后节点应无任何域名(空 domains[] 即清空)"
    );
}

#[tokio::test]
async fn test_pg_update_node_cf_cert_mode_defaults_reuse_direct() {
    // 回归:update 路径 cf_cert_mode 一律默认 reuse_direct(token-less 免 token),acme 不再触发 dns01。
    // 修复前裸 SQL 只看 cf_domain 有没有就写 dns01;且 acme 本就是直连 HTTP-01 也要用的邮箱、不是 DNS-01
    // 凭据信号(DNS-01 需 CF API token),据 acme 误判 dns01 会锚到签不出的 cf_domain 路径、被 BUG-D 跳过。
    // 修复后:有无 acme 都落 reuse_direct;要 dns01 由运维在 cf 域名上显式设。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping cf_cert_mode derive test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    let app = app(AppState::with_pg(store.clone()));
    let token = admin_login_token(app.clone()).await;
    let node_id = create_node_with_known_id(
        app.clone(),
        &token,
        json!({ "ip_direct_address": "192.0.2.7" }),
    )
    .await;

    // token-less:填 CF 域名、不填 acme → reuse_direct(复用直连灰云证书,免 token)。
    let r1 = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}"),
        Some(&token),
        json!({
            "cf_domain": "cf.example.test",
            "domains": [{"domain": "cf.example.test", "kind": "cf", "is_primary": true}]
        }),
    )
    .await;
    assert_eq!(r1.0, StatusCode::OK, "{}", r1.1);
    let mode1: String = sqlx::query_scalar("SELECT cf_cert_mode FROM access_nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        mode1, "reuse_direct",
        "token-less CF 编辑后应 reuse_direct,不得写成 dns01"
    );

    // 即便带 acme(直连证书 HTTP-01 要用),编辑后 cf_cert_mode 仍应 reuse_direct,不被误判 dns01。
    let r2 = request_json(
        app.clone(),
        Method::PUT,
        &format!("/api/admin/access-nodes/{node_id}"),
        Some(&token),
        json!({
            "cf_domain": "cf.example.test",
            "acme_email": "ops@example.test",
            "domains": [{"domain": "cf.example.test", "kind": "cf", "is_primary": true}]
        }),
    )
    .await;
    assert_eq!(r2.0, StatusCode::OK, "{}", r2.1);
    let mode2: String = sqlx::query_scalar("SELECT cf_cert_mode FROM access_nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        mode2, "reuse_direct",
        "有 acme 也不应误判 dns01:acme 非 DNS-01 凭据信号,update 后仍应 reuse_direct"
    );
}

// 本模块单独实现请求工具,避免跨测试模块耦合。
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
