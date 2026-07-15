//! 数据库自动备份编排入口(目录模块 `backups` 的根)。
//! 本文件负责一轮备份的总编排:读 `site_settings.backup_config`、取 advisory 锁防多 Worker
//! 并发、由纯决策层 `plan` 判定四模式(全量 / 异地 / WAL / 邮件)各自 Run/Skip,再薄接线到
//! 各模式的执行(`run` 子模块),最后把运行态写回 `database_backup_state` 并清除已消费的 run_now。
//! 具体职责已按文件拆分:plan(纯决策)、schedule(判 due)、dump(pg_dump/清理)、
//! offsite(异地 rsync)、wal(pg_basebackup/归档清理)、email(附件加密+发送)、secrets(脱敏)、
//! run(各模式执行、历史与状态回写的薄接线)、run_offsite(各模式落盘后异地并入处理)。
//! 红线:异地/WAL/邮件任一失败都只脱敏记状态 + 继续,绝不中断其它模式、绝不 panic、绝不拖垮全量;
//! 所有落状态/日志的错误摘要先过 `secrets` 脱敏。对外暴露 `DatabaseBackupRunner`/`run_all_if_due`。
//! 本头部满足前十行中文说明约束。

mod dump;
mod email;
mod offsite;
mod plan;
mod run;
mod run_now;
mod run_offsite;
mod schedule;
mod secrets;
mod wal;

use std::path::PathBuf;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use xrayc_db::PgStore;

use plan::RunNowTarget;

const DEFAULT_DUMP_TIMEOUT_SECONDS: u64 = 1_800;
const DEFAULT_RETRY_SECONDS: u64 = 3_600;
/// WAL 归档卷默认路径(postgres archive_command 写、worker 只读+清理),compose 注入同名 env。
const DEFAULT_WAL_ARCHIVE_DIR: &str = "/wal-archive";
/// base 备份仓库卷默认路径(worker 写),compose 注入同名 env。
const DEFAULT_WAL_REPO_DIR: &str = "/wal-repo";
/// 中心机备份私钥目录默认路径(免密同步用),compose 挂持久卷、注入同名 env。
const DEFAULT_BACKUP_SSH_KEY_DIR: &str = "/var/lib/xrayc/backup_ssh";

#[derive(Debug, Clone)]
pub(crate) struct DatabaseBackupRunner {
    database_url: String,
    backup_dir: PathBuf,
    pg_dump_bin: String,
    dump_timeout: StdDuration,
    retry_delay: StdDuration,
    /// WAL base 备份仓库卷(pg_basebackup 落地 + 保留清理)。
    wal_repo_dir: PathBuf,
    /// WAL 归档卷(pg_archivecleanup 协调清理的对象)。
    wal_archive_dir: PathBuf,
    /// 中心机备份私钥目录(异地 / WAL 免密 rsync 用)。
    key_dir: PathBuf,
}

/// 单个模式在一轮里的执行结果(供 main.rs 日志观测,不含敏感信息)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ModeStatus {
    /// 本轮该模式未触发(未到期 / 未开启 / 无 run_now)。
    #[default]
    Idle,
    /// 触发了但主动跳过(如邮件无收件人 / 无可发 dump / SMTP 配置不完整)。
    Skipped,
    /// 成功执行。
    Ran,
    /// 执行失败(已脱敏落状态,不中断其它模式)。
    Failed,
    /// 未取到 advisory 锁,整轮跳过。
    Locked,
}

/// 一轮备份编排的结果快照(三种方式各一状态 + 全量文件名),仅供日志。
/// 异地不再是独立方式:异地结果并入 full/wal 的 state,故本报告不再有独立 offsite 档。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BackupRoundReport {
    pub(crate) full: ModeStatus,
    pub(crate) full_file: Option<String>,
    pub(crate) wal: ModeStatus,
    pub(crate) email: ModeStatus,
}

impl BackupRoundReport {
    /// 未取到锁:三种方式一律标 Locked。
    fn locked() -> Self {
        Self {
            full: ModeStatus::Locked,
            full_file: None,
            wal: ModeStatus::Locked,
            email: ModeStatus::Locked,
        }
    }
}

impl DatabaseBackupRunner {
    pub(crate) fn from_env(database_url: String) -> Self {
        let backup_dir = std::env::var("DATABASE_BACKUP_DIR")
            .or_else(|_| std::env::var("XRAYC_DATABASE_BACKUP_DIR"))
            .unwrap_or_else(|_| "/backups".to_string());
        let pg_dump_bin = std::env::var("PG_DUMP_BIN").unwrap_or_else(|_| "pg_dump".to_string());
        let dump_timeout = dump::env_duration_seconds(
            "DATABASE_BACKUP_TIMEOUT_SECONDS",
            DEFAULT_DUMP_TIMEOUT_SECONDS,
            60,
            86_400,
        );
        let retry_delay = dump::env_duration_seconds(
            "DATABASE_BACKUP_RETRY_SECONDS",
            DEFAULT_RETRY_SECONDS,
            60,
            86_400,
        );
        let wal_repo_dir = std::env::var("XRAYC_WAL_REPO_DIR")
            .unwrap_or_else(|_| DEFAULT_WAL_REPO_DIR.to_string());
        let wal_archive_dir = std::env::var("XRAYC_WAL_ARCHIVE_DIR")
            .unwrap_or_else(|_| DEFAULT_WAL_ARCHIVE_DIR.to_string());
        let key_dir = std::env::var("XRAYC_BACKUP_SSH_KEY_DIR")
            .unwrap_or_else(|_| DEFAULT_BACKUP_SSH_KEY_DIR.to_string());
        Self {
            database_url,
            backup_dir: PathBuf::from(backup_dir),
            pg_dump_bin,
            dump_timeout,
            retry_delay,
            wal_repo_dir: PathBuf::from(wal_repo_dir),
            wal_archive_dir: PathBuf::from(wal_archive_dir),
            key_dir: PathBuf::from(key_dir),
        }
    }

    /// 一轮备份总编排:按 plan 决策依次跑 全量 → 异地 → WAL → 邮件,全程持同一 advisory 锁。
    /// 先在锁外用「full_succeeded=false」预判本轮是否有任何模式需要触发或有 run_now 待清:
    /// 全无则直接返回(免锁、免 DB 抖动)。取到锁后进入 `run_locked_round` 做最终决策与执行。
    pub(crate) async fn run_all_if_due(
        &self,
        store: &PgStore,
    ) -> anyhow::Result<BackupRoundReport> {
        let config = store.backup_config_json().await?;
        let state = store.database_backup_state_json().await?;
        let now = Utc::now();

        // 取锁前预判本轮是否有任一方式到期(full/wal/email 各自独立调度,互不依赖)或有 run_now 待处理;
        // 都没有就不取锁直接返回,避免空转。
        let pre = plan::plan_backup(&config, &state, now, false);
        if !pre.any() && pre.run_now == RunNowTarget::None {
            return Ok(BackupRoundReport::default());
        }

        let Some(lock_guard) = store.try_database_backup_lock().await? else {
            return Ok(BackupRoundReport::locked());
        };
        let result = self.run_locked_round(store, &config, now).await;
        // 锁释放失败要如实上抛(与旧 run_if_due 一致),但不吞掉执行结果。
        let unlock_result = lock_guard.release().await;
        match (result, unlock_result) {
            (Ok(report), Ok(())) => Ok(report),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(error), _) => Err(error),
        }
    }

    /// 持锁内的一轮执行:锁内重读 state 做最终决策(防锁外预判与取锁之间的竞态),
    /// 分两相运行——相位一全量(不依赖 full_succeeded)、相位二异地/WAL/邮件(全量成功与否已知)。
    /// 各模式执行方法自行捕获错误、脱敏落状态、绝不上抛(错误隔离);仅底层 DB/锁故障才上抛。
    async fn run_locked_round(
        &self,
        store: &PgStore,
        config: &Value,
        now: DateTime<Utc>,
    ) -> anyhow::Result<BackupRoundReport> {
        let mut state = store.database_backup_state_json().await?;
        if !state.is_object() {
            state = json!({});
        }
        let secrets = secrets::backup_secrets(&self.database_url);
        let run_now = plan::parse_run_now(&state);
        // 记下本轮消费的 run_now 标识(requested_at),供轮末「只清这一条」的条件清除比对。
        let consumed_run_now_at = run_now::run_now_requested_at(&state);
        let mut report = BackupRoundReport::default();

        // ---- 相位一:全量 ----
        let full_should_run = plan::plan_backup(config, &state, now, false).full;
        if full_should_run {
            match self.run_full_backup(store, config, &mut state).await {
                Ok(file_name) => {
                    report.full = ModeStatus::Ran;
                    report.full_file = Some(file_name);
                }
                Err(error) => {
                    // 全量失败:state.full 已在 run_full_backup 内落失败态;此处只脱敏记日志,
                    // 绝不中断 WAL/邮件——它们各自独立调度、与全量成功与否无关(全独立)。
                    report.full = ModeStatus::Failed;
                    let summary = secrets::sanitize_text(error.to_string(), &secrets);
                    tracing::warn!(summary = %summary, "full backup failed");
                }
            }
        }

        // ---- 相位二:WAL / 邮件(各自独立调度,逐一错误隔离)----
        // WAL/邮件都按各自 schedule 独立判到期,与全量成功与否无关;相位一先跑全量只为让邮件能取到
        // 本轮最新 dump(取不到就发上一份已存在的)。异地不再是独立方式(并入 full/wal 的 destination)。
        let plan = plan::plan_backup(config, &state, now, false);
        if plan.wal {
            report.wal = self.run_wal(store, config, &mut state, &secrets).await;
        }
        if plan.email {
            report.email = self.run_email(store, config, &mut state, &secrets).await;
        }

        // 清除本轮消费的 run_now:重读 DB 比对 requested_at,一致才清;轮内被 API 换了新标记
        // (不同)则不清、留给下一轮,避免误清丢触发(问题B 红线)。
        if run_now != RunNowTarget::None {
            let db_state = store
                .database_backup_state_json()
                .await
                .unwrap_or_else(|_| json!({}));
            let current_at = run_now::run_now_requested_at(&db_state);
            let consumed_at = consumed_run_now_at.as_deref();
            if run_now::should_clear_run_now(consumed_at, current_at.as_deref()) {
                // 用重读的 db_state(含最新模式态)删 run_now 写回,不走 persist_state(会再注入)。
                let mut cleared = db_state;
                clear_run_now(&mut cleared);
                if let Err(error) = store.update_database_backup_state_json(cleared).await {
                    tracing::warn!(?error, "clear run_now state failed");
                }
            }
        }
        Ok(report)
    }

    /// 整份运行态写回。并发保护:写回前以 DB 现存 run_now 为准注入,避免整份覆盖抹掉
    /// 「轮进行中 API 新设的 run_now」(点了立即运行却没反应的根因);DB 读失败退化为不注入。
    async fn persist_state(&self, store: &PgStore, state: &Value) -> anyhow::Result<()> {
        let db_state = store
            .database_backup_state_json()
            .await
            .unwrap_or_else(|_| json!({}));
        let to_write = run_now::preserve_run_now(state.clone(), &db_state);
        store
            .update_database_backup_state_json(to_write)
            .await
            .map_err(anyhow::Error::from)
    }
}

/// 把某模式的运行态子对象整体写入 state 顶层(各模式键独立,不深合并)。
/// 若本次写入没显式带 status,按时间戳自动补(last_failed_at→failed、last_success_at→success、
/// last_started_at→running、都无→idle),让前端「状态」列对所有模式(不只全量)都正确显示中文状态。
fn set_mode_state(state: &mut Value, mode: &str, mut value: Value) {
    if let Some(object) = value.as_object_mut() {
        if !object.contains_key("status") {
            let present = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
            };
            let status = if present("last_failed_at") {
                "failed"
            } else if present("last_success_at") {
                "success"
            } else if present("last_started_at") {
                "running"
            } else {
                "idle"
            };
            object.insert("status".to_string(), json!(status));
        }
    }
    if let Some(object) = state.as_object_mut() {
        object.insert(mode.to_string(), value);
    }
}

/// 把 `patch` 的键**合并**进某方式已存在的 state 子对象(不整体替换,保留 dump/prune 已写字段)。
/// 用于全量/WAL 落盘成功后把异地结果(offsite_synced/offsite_last_sync_at/offsite_error 等)并入
/// 同一方式的 state:先取现有子对象(缺失则空对象),逐键覆盖 patch,再写回顶层。
/// 与 set_mode_state 的区别:后者整块替换(会丢已写字段),本函数只增量合并、保留原有 status 等。
fn merge_mode_state(state: &mut Value, mode: &str, patch: Value) {
    let mut current = state
        .get(mode)
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let (Some(object), Some(patch_object)) = (current.as_object_mut(), patch.as_object()) {
        for (key, value) in patch_object {
            object.insert(key.clone(), value.clone());
        }
    }
    if let Some(object) = state.as_object_mut() {
        object.insert(mode.to_string(), current);
    }
}

/// 追加一条某方式的备份历史到 **独立顶层** `state["history"][mode]`(最新在前、只留最近 30 条)。
/// 为何独立存放而非挂在 `state[mode]` 子对象下:各方式运行态 `state[mode]` 由 `set_mode_state`
/// **整体替换**写回(见上方 set_mode_state),若历史挂在 `state[mode]` 里,每次写运行态都会把
/// 历史数组整块冲掉;故历史必须放到与各 mode 子对象平级的顶层 `history` 段——`set_mode_state`/
/// `merge_mode_state` 都只碰 `state[mode]`、绝不动 `state["history"]`,两者互不干扰。
/// 本函数只读写 `state["history"][mode]`,**绝不触碰 `state[mode]` 本身**:首次为空以空数组起步,
/// 头部插入本轮 entry(最新在前),再截断到 30 条(超出丢最老,保留最近 30 次)。
/// 纯 JSON 逻辑、不做 IO、state 非对象时安全无操作、绝不 panic。
pub(super) fn append_mode_history(state: &mut Value, mode: &str, entry: Value) {
    /// 每方式历史最多保留条数(超出丢最老)。
    const HISTORY_LIMIT: usize = 30;
    let Some(root) = state.as_object_mut() else {
        return;
    };
    // 顶层 history 段(缺失则建空对象),与各 mode 子对象平级、不受 set_mode_state 覆盖。
    let history = root.entry("history").or_insert_with(|| json!({}));
    let Some(history_object) = history.as_object_mut() else {
        return;
    };
    let list = history_object
        .entry(mode)
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(array) = list.as_array_mut() else {
        return;
    };
    // 最新在前:头部插入;超上限从尾部(最老)截断。
    array.insert(0, entry);
    if array.len() > HISTORY_LIMIT {
        array.truncate(HISTORY_LIMIT);
    }
}

/// 组装一条备份历史 entry(纯函数,便于单测 size 取值/字段形态)。
/// `file`/`offsite_synced`/`error` 用 Option:None → JSON null(对应「不适用/未推异地/无错误」)。
/// **不做脱敏**:error 须由调用方先过 `secrets` 脱敏后再传入,本函数只装配。
pub(super) fn history_entry(
    at: String,
    ok: bool,
    size_bytes: u64,
    file: Option<String>,
    offsite_synced: Option<bool>,
    error: Option<String>,
) -> Value {
    json!({
        "at": at,
        "ok": ok,
        "size_bytes": size_bytes,
        "file": file,
        "offsite_synced": offsite_synced,
        "error": error,
    })
}

/// 清除 run_now(本轮立即触发已消费)。
fn clear_run_now(state: &mut Value) {
    if let Some(object) = state.as_object_mut() {
        object.remove("run_now");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_run_now_removes_only_run_now_key() {
        // 清除 run_now 只删该键,其它方式状态原样保留(避免误伤)。
        let mut state = json!({
            "run_now": {"mode": "full", "requested_at": "2026-07-01T00:00:00Z"},
            "full": {"last_success_at": "2026-07-01T00:00:00Z"},
            "wal": {"last_success_at": "2026-07-01T00:00:00Z"}
        });
        clear_run_now(&mut state);
        assert!(state.get("run_now").is_none(), "run_now 应被清除");
        assert!(state.get("full").is_some(), "full 子状态不受影响");
        assert!(state.get("wal").is_some(), "wal 子状态不受影响");
    }

    #[test]
    fn set_mode_state_replaces_only_target_mode() {
        // 写某方式子状态只覆盖该方式键,其它顶层键保留(多方式共存红线)。
        let mut state = json!({
            "full": {"status": "success"},
            "run_now": {"mode": "wal"}
        });
        set_mode_state(&mut state, "wal", json!({"base_dir_name": "base-x"}));
        assert_eq!(state["wal"]["base_dir_name"], json!("base-x"));
        assert_eq!(state["full"]["status"], json!("success"), "full 键不受影响");
        assert!(state.get("run_now").is_some(), "run_now 键不受影响");
    }

    #[test]
    fn merge_mode_state_preserves_existing_fields() {
        // 异地结果并入 full:合并只增量覆盖 patch 键,dump 已写的 last_success_at/status 等保留。
        let mut state = json!({
            "full": {"status": "success", "last_success_at": "2026-07-01T00:00:00Z", "last_file_name": "x.dump"}
        });
        merge_mode_state(
            &mut state,
            "full",
            json!({"offsite_synced": true, "offsite_last_sync_at": "2026-07-01T00:05:00Z"}),
        );
        // 原有字段保留。
        assert_eq!(state["full"]["status"], json!("success"));
        assert_eq!(state["full"]["last_file_name"], json!("x.dump"));
        // 异地字段并入 full(不再有独立 offsite 顶层键)。
        assert_eq!(state["full"]["offsite_synced"], json!(true));
        assert_eq!(
            state["full"]["offsite_last_sync_at"],
            json!("2026-07-01T00:05:00Z")
        );
        assert!(state.get("offsite").is_none(), "不应有独立 offsite 顶层键");
    }

    #[test]
    fn merge_mode_state_creates_mode_when_absent() {
        // 目标方式子对象缺失时,合并会新建该子对象(不 panic)。
        let mut state = json!({});
        merge_mode_state(&mut state, "wal", json!({"offsite_synced": false}));
        assert_eq!(state["wal"]["offsite_synced"], json!(false));
    }

    #[test]
    fn set_mode_state_ignores_non_object_state() {
        // state 非对象时安全无操作(不 panic)。
        let mut state = json!("not-an-object");
        set_mode_state(&mut state, "full", json!({"status": "running"}));
        assert_eq!(state, json!("not-an-object"));
    }

    #[test]
    fn locked_report_marks_all_modes_locked() {
        let report = BackupRoundReport::locked();
        assert_eq!(report.full, ModeStatus::Locked);
        assert_eq!(report.wal, ModeStatus::Locked);
        assert_eq!(report.email, ModeStatus::Locked);
    }

    #[test]
    fn append_mode_history_first_entry_starts_from_empty() {
        // 首次追加:从无 history 段起步,建 history[mode] 数组并落入该 entry。
        let mut state = json!({});
        append_mode_history(&mut state, "wal", json!({"at": "t1"}));
        let list = state["history"]["wal"]
            .as_array()
            .expect("应建 wal 历史数组");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["at"], json!("t1"));
    }

    #[test]
    fn append_mode_history_prepends_newest_first() {
        // 最新在前:后追加的排在数组头部。
        let mut state = json!({});
        append_mode_history(&mut state, "full", json!({"at": "t1"}));
        append_mode_history(&mut state, "full", json!({"at": "t2"}));
        let list = state["history"]["full"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["at"], json!("t2"), "最新在前");
        assert_eq!(list[1]["at"], json!("t1"));
    }

    #[test]
    fn append_mode_history_truncates_to_30() {
        // 只保留最近 30 条:追加 35 条后长度封顶 30,最老 5 条被挤掉。
        let mut state = json!({});
        for index in 0..35 {
            append_mode_history(&mut state, "full", json!({ "at": format!("t{index}") }));
        }
        let list = state["history"]["full"].as_array().unwrap();
        assert_eq!(list.len(), 30, "只保留最近 30 条");
        assert_eq!(list[0]["at"], json!("t34"), "最新在前");
        assert_eq!(
            list[29]["at"],
            json!("t5"),
            "最老保留的是第 6 新(t0..t4 被挤掉)"
        );
    }

    #[test]
    fn append_mode_history_does_not_touch_mode_subobject() {
        // 独立存放红线:追加历史绝不动 state[mode];反向 set_mode_state 覆盖 mode 后历史仍在。
        let mut state = json!({ "full": {"status": "success", "last_file_name": "x.dump"} });
        append_mode_history(&mut state, "full", json!({"at": "t1", "ok": true}));
        assert_eq!(
            state["full"]["status"],
            json!("success"),
            "state.full 原样保留"
        );
        assert_eq!(state["full"]["last_file_name"], json!("x.dump"));
        assert_eq!(state["history"]["full"][0]["at"], json!("t1"));
        // set_mode_state 整体替换 full 后,独立顶层历史不受影响。
        set_mode_state(&mut state, "full", json!({"status": "running"}));
        assert_eq!(
            state["history"]["full"][0]["at"],
            json!("t1"),
            "覆盖 mode 不应冲掉历史"
        );
    }

    #[test]
    fn append_mode_history_modes_are_independent() {
        // 各方式历史互不干扰:写 full 不影响 wal,未写的 email 无历史。
        let mut state = json!({});
        append_mode_history(&mut state, "full", json!({"at": "f1"}));
        append_mode_history(&mut state, "wal", json!({"at": "w1"}));
        assert_eq!(state["history"]["full"].as_array().unwrap().len(), 1);
        assert_eq!(state["history"]["wal"].as_array().unwrap().len(), 1);
        assert_eq!(state["history"].get("email"), None);
    }

    #[test]
    fn history_entry_maps_options_to_null_and_keeps_values() {
        // entry 组装:成功条带 size/file/offsite_synced;失败条 file/offsite_synced 为 null、带 error。
        let ok_entry = history_entry(
            "2026-07-01T00:00:00Z".to_string(),
            true,
            12_345,
            Some("x.dump".to_string()),
            Some(true),
            None,
        );
        assert_eq!(ok_entry["at"], json!("2026-07-01T00:00:00Z"));
        assert_eq!(ok_entry["ok"], json!(true));
        assert_eq!(ok_entry["size_bytes"], json!(12_345));
        assert_eq!(ok_entry["file"], json!("x.dump"));
        assert_eq!(ok_entry["offsite_synced"], json!(true));
        assert_eq!(ok_entry["error"], Value::Null);

        let fail_entry = history_entry(
            "t".to_string(),
            false,
            0,
            None,
            None,
            Some("boom".to_string()),
        );
        assert_eq!(fail_entry["ok"], json!(false));
        assert_eq!(fail_entry["size_bytes"], json!(0));
        assert_eq!(fail_entry["file"], Value::Null);
        assert_eq!(fail_entry["offsite_synced"], Value::Null);
        assert_eq!(fail_entry["error"], json!("boom"));
    }

    #[test]
    fn default_report_is_all_idle() {
        let report = BackupRoundReport::default();
        assert_eq!(report.full, ModeStatus::Idle);
        assert_eq!(report.wal, ModeStatus::Idle);
        assert_eq!(report.email, ModeStatus::Idle);
        assert!(report.full_file.is_none());
    }
}
