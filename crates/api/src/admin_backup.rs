//! 数据库备份管理后台接口模块(spec §11)。
//! 提供五个管理员 JWT 接口:读写 backup_config(脱敏/写时保留)、
//! test-and-provision(装公钥,SSH 逻辑收敛到 xrayc-backup crate)、
//! run-now(往 database_backup_state 写触发标记给 worker 编排)、读 state。
//! handler 只处理请求/响应与脱敏,配置读写与合并保留全由 db 层 store 方法完成。
//! 敏感项(SSH 密码/邮件口令)绝不回显:输出脱敏成 <field>_set,审计不记明文。
//! provision 的连接信息只在请求内一次性使用,响应 message 已由 crate 抹掉 host/密码。
//! 本模块不直接 SSH、不写日志敏感值、不触碰 worker/前端。
//! 单文件控制在 550 行硬上限内,复杂逻辑写明「为什么」。
//! 本头部满足前十行中文注释约束。

use super::*;
use serde::Deserialize;
use serde_json::Value;

/// run-now 允许的备份方式枚举:三种备份方式(全量/WAL/邮件)。
/// 异地不再是独立方式(并入 full/wal 的 destination),故不接受 "offsite"。
const RUN_NOW_MODES: [&str; 3] = ["full", "wal", "email"];

/// 中心机备份私钥目录:优先取 compose 注入的 env,缺省回退与 compose 默认一致的路径。
/// api 只读该目录用于一次性装公钥;worker 侧读写同一持久卷。
fn backup_ssh_key_dir() -> PathBuf {
    std::env::var("XRAYC_BACKUP_SSH_KEY_DIR")
        .unwrap_or_else(|_| "/var/lib/xrayc/backup_ssh".to_string())
        .into()
}

/// GET /api/admin/backup/config —— 返回脱敏配置(SSH 密码/邮件口令出 <field>_set,不回显明文)。
pub(crate) async fn admin_backup_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.public_backup_config_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

/// PUT /api/admin/backup/config —— 写入配置(密码/口令留空即写时保留旧值),再返回脱敏版。
pub(crate) async fn update_admin_backup_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_backup_config_json(body).await {
        Ok(_) => {
            // 审计只记设置键与脱敏标记,绝不记 SSH 密码/邮件口令明文。
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "backup_config.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "backup_config", "secrets_redacted": true}),
            )
            .await;
            // 重读脱敏版返回,不把明文凭据回吐。
            match pg.public_backup_config_json().await {
                Ok(data) => {
                    Json(serde_json::json!({"success": true, "data": data})).into_response()
                }
                Err(err) => internal_error(err).into_response(),
            }
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

/// test-and-provision 请求体:字段全可选,缺失时回退已存配置(host/port/user)或密码兜底。
#[derive(Debug, Deserialize)]
pub(crate) struct TestProvisionRequest {
    pub(crate) ssh_host: Option<String>,
    pub(crate) ssh_port: Option<u16>,
    pub(crate) ssh_user: Option<String>,
    pub(crate) ssh_password: Option<String>,
    // remote_dir 走 PUT config 保存,本端点接收但不参与 provision(serde 默认忽略未声明字段)。
}

/// POST /api/admin/backup/test-and-provision —— 测 SSH 连通 + 装公钥(§7)。
/// 逻辑:取连接信息(请求体优先,缺失回退已存配置;密码缺失用库内明文兜底)→ 调 crate
/// provision_offsite 在请求内一次性 SSH 装公钥 → 成功仅把 pubkey_installed/指纹写回(最小
/// patch,深合并 + 写时保留,绝不覆盖其它字段/密码)→ 返回脱敏 {ok,fingerprint,message}。
pub(crate) async fn admin_backup_test_and_provision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TestProvisionRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    // 读明文配置(含 offsite 密码)作字段回退与密码兜底。
    let config = match pg.backup_config_json().await {
        Ok(config) => config,
        Err(err) => return internal_error(err).into_response(),
    };
    let offsite = config.get("offsite");
    let host = body
        .ssh_host
        .filter(|value| !value.is_empty())
        .or_else(|| offsite_str(offsite, "ssh_host"))
        .unwrap_or_default();
    let port = body
        .ssh_port
        .or_else(|| {
            offsite
                .and_then(|value| value.get("ssh_port"))
                .and_then(Value::as_u64)
                .map(|value| value as u16)
        })
        .unwrap_or(22);
    let user = body
        .ssh_user
        .filter(|value| !value.is_empty())
        .or_else(|| offsite_str(offsite, "ssh_user"))
        .unwrap_or_else(|| "root".to_string());
    // 请求体没带密码则用已存配置里的明文兜底(§7:密钥失效可回退重装公钥)。
    let password = body
        .ssh_password
        .filter(|value| !value.is_empty())
        .or_else(|| offsite_str(offsite, "ssh_password"))
        .unwrap_or_default();

    let key_dir = backup_ssh_key_dir();
    let result =
        match xrayc_backup::provision_offsite(&host, port, &user, &password, &key_dir).await {
            Ok(result) => result,
            // 仅二进制缺失等无法启动的异常走 Err;其 context 固定、不含密码/host。
            Err(err) => return internal_error(err).into_response(),
        };

    if result.ok {
        // 成功:最小 patch 写回 pubkey_installed + 指纹。深合并保留其它字段,写时保留不动密码。
        let patch = serde_json::json!({
            "offsite": {
                "pubkey_installed": true,
                "pubkey_fingerprint": result.fingerprint.clone().unwrap_or_default(),
            }
        });
        if let Err(err) = pg.update_backup_config_json(patch).await {
            return internal_error(err).into_response();
        }
        record_admin_audit(
            pg,
            &claims,
            &headers,
            "backup.provision",
            "site_settings",
            None,
            serde_json::json!({
                "setting_key": "backup_config",
                "pubkey_installed": true,
                "secrets_redacted": true
            }),
        )
        .await;
    }

    // 响应脱敏:message 已由 crate 抹掉 host/密码,fingerprint 仅公钥指纹(可展示)。
    Json(serde_json::json!({
        "success": true,
        "data": {
            "ok": result.ok,
            "fingerprint": result.fingerprint,
            "message": result.message,
        }
    }))
    .into_response()
}

/// 从 offsite 对象取字符串字段(空/缺失返回 None,便于 or_else 链式回退)。
fn offsite_str(offsite: Option<&Value>, field: &str) -> Option<String> {
    offsite
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// run-now 查询参数:mode 必填且须为枚举之一。
#[derive(Debug, Deserialize)]
pub(crate) struct RunNowQuery {
    pub(crate) mode: Option<String>,
}

/// POST /api/admin/backup/run-now?mode=full|wal|email —— 立即触发一次。
/// 校验 mode 枚举 → 往 database_backup_state 写 run_now 触发标记(不覆盖其它状态字段);
/// worker 编排会读它、执行、清除。返回 {ok,message}。异地不再单独触发(并入 full/wal)。
pub(crate) async fn admin_backup_run_now(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RunNowQuery>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mode = query.mode.unwrap_or_default();
    if !RUN_NOW_MODES.contains(&mode.as_str()) {
        return bad_request("mode 只能是 full/wal/email 之一").into_response();
    }
    // 读现有状态并保留其它字段:update_* 是整值覆盖,故必须先读回再插 run_now 再写回。
    let current = match pg.database_backup_state_json().await {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let mut state_object = current.as_object().cloned().unwrap_or_default();
    state_object.insert(
        "run_now".to_string(),
        serde_json::json!({
            "mode": mode,
            "requested_at": chrono::Utc::now().to_rfc3339(),
        }),
    );
    if let Err(err) = pg
        .update_database_backup_state_json(Value::Object(state_object))
        .await
    {
        return internal_error(err).into_response();
    }
    record_admin_audit(
        pg,
        &claims,
        &headers,
        "backup.run_now",
        "site_settings",
        None,
        serde_json::json!({"setting_key": "database_backup_state", "mode": mode}),
    )
    .await;
    Json(serde_json::json!({
        "success": true,
        "data": {
            "ok": true,
            "message": format!("已触发 {mode} 备份,worker 将尽快执行"),
        }
    }))
    .into_response()
}

/// GET /api/admin/backup/state —— 各方式最近状态。
/// 把原始 database_backup_state 整理成前端要的 {full,wal,email} 结构(异地结果并入 full/wal)。
pub(crate) async fn admin_backup_state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.database_backup_state_json().await {
        Ok(raw) => Json(serde_json::json!({"success": true, "data": shape_backup_state(raw)}))
            .into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

/// 把原始状态整理成 {full,wal,email} 三方式结构:每方式先给一份默认骨架,
/// 再用库里已有字段浅覆盖(worker 后续写入的字段得以保留);附带透传 run_now 触发标记。
/// 异地不再是独立方式:异地结果作为 full/wal 的字段(offsite_synced 等)由浅覆盖自然带出。
/// 状态本身只含时间戳/成败/脱敏错误摘要/文件名,不含凭据,故无需额外回显控制。
fn shape_backup_state(raw: Value) -> Value {
    let raw_object = raw.as_object().cloned().unwrap_or_default();
    // 顶层 history 段(worker 侧 append_mode_history 写在此,独立于各 mode 子对象、
    // 不被 set_mode_state 覆盖);缺失给空对象。把各方式历史并进对应方式输出,前端一处拿到。
    let history = raw_object
        .get("history")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut data = serde_json::Map::new();
    for mode in RUN_NOW_MODES {
        let mut skeleton = default_mode_state(mode);
        if let (Some(target), Some(existing)) = (
            skeleton.as_object_mut(),
            raw_object.get(mode).and_then(Value::as_object),
        ) {
            for (key, value) in existing {
                target.insert(key.clone(), value.clone());
            }
        }
        // 并入该方式历史数组(缺失给空数组,保证前端结构稳定)。
        if let Some(target) = skeleton.as_object_mut() {
            target.insert(
                "history".to_string(),
                history
                    .get(mode)
                    .cloned()
                    .unwrap_or_else(|| Value::Array(Vec::new())),
            );
        }
        data.insert(mode.to_string(), skeleton);
    }
    // 透传 run_now(供前端展示"已触发待执行");缺失给 null 保持结构稳定。
    data.insert(
        "run_now".to_string(),
        raw_object.get("run_now").cloned().unwrap_or(Value::Null),
    );
    Value::Object(data)
}

/// 单个方式的默认状态骨架:通用五字段全给 null,再按方式补各自字段。
/// full/wal 额外带异地结果字段(offsite_synced/offsite_last_sync_at/offsite_error),
/// 保证前端结构稳定(异地已并入 full/wal,不再有独立 offsite 档)。
fn default_mode_state(mode: &str) -> Value {
    let mut base = serde_json::json!({
        "last_started_at": Value::Null,
        "last_success_at": Value::Null,
        "last_failed_at": Value::Null,
        "error_summary": Value::Null,
        "last_file_name": Value::Null,
    });
    if let Some(object) = base.as_object_mut() {
        // full/wal 都可推异地(destination=both/offsite),故都带异地结果占位字段。
        if mode == "full" || mode == "wal" {
            object.insert("offsite_synced".to_string(), Value::Null);
            object.insert("offsite_last_sync_at".to_string(), Value::Null);
            object.insert("offsite_error".to_string(), Value::Null);
        }
        match mode {
            "wal" => {
                object.insert("wal_last_archive_ok".to_string(), Value::Null);
            }
            "email" => {
                object.insert("email_last_sent_at".to_string(), Value::Null);
                object.insert("split_parts".to_string(), Value::Null);
            }
            _ => {}
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::shape_backup_state;
    use serde_json::json;

    #[test]
    fn shape_backup_state_merges_top_level_history_into_each_mode() {
        // 顶层 history[mode] 应并入对应方式输出;缺历史的方式给空数组;结构稳定。
        let raw = json!({
            "full": {"last_success_at": "2026-07-01T00:00:00Z"},
            "history": {
                "full": [
                    {"at": "2026-07-01T00:00:00Z", "ok": true, "size_bytes": 123, "file": "x.dump",
                     "offsite_synced": true, "error": null}
                ],
                "wal": []
            }
        });
        let shaped = shape_backup_state(raw);
        // full 历史并入 full 输出(与运行态字段共存)。
        assert_eq!(
            shaped["full"]["last_success_at"],
            json!("2026-07-01T00:00:00Z")
        );
        let full_history = shaped["full"]["history"]
            .as_array()
            .expect("full 历史应为数组");
        assert_eq!(full_history.len(), 1);
        assert_eq!(full_history[0]["size_bytes"], json!(123));
        assert_eq!(full_history[0]["file"], json!("x.dump"));
        // wal 历史为空数组;email 无历史也给空数组(不缺键)。
        assert_eq!(shaped["wal"]["history"], json!([]));
        assert_eq!(shaped["email"]["history"], json!([]));
    }

    #[test]
    fn shape_backup_state_history_defaults_to_empty_when_absent() {
        // 完全没有 history 段:三方式都给空数组,前端结构稳定。
        let shaped = shape_backup_state(json!({}));
        for mode in ["full", "wal", "email"] {
            assert_eq!(
                shaped[mode]["history"],
                json!([]),
                "方式 {mode} 历史应为空数组"
            );
        }
    }
}
