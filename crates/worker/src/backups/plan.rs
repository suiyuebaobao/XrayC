//! 备份编排的纯决策层:输入 backup_config + 运行态 state + now + 本轮全量是否成功,
//! 输出三种备份方式(full/wal/email)各自的 Run/Skip 决策与 run_now 立即触发目标。
//! 异地(offsite)不再是独立备份模式:它只是「共享异地服务器配置」,是否推异地由 full/wal 各自的
//! `destination`(local/offsite/both)在执行层决定,故本决策层不再产出 offsite 分支。
//! 本模块只做判定、绝无 IO/DB/网络副作用,便于单测覆盖各类边界(enabled/schedule/依赖 full 成功/
//! run_now 强制/wal 未 configured 等)。执行与状态回写由 mod.rs/run.rs 薄接线。
//! 决策依据严格对齐 spec §5/§6 的组合语义:各方式独立开关 + 独立调度,email 可「跟随全量成功即
//! 触发」(full_succeeded)。run_now 绕过 schedule 强制某方式一次。
//! 每方式的调度到期读各自 state 子对象(state.full/wal 的 last_success_at),互不串扰。
//! 本头部满足前十行中文注释约束。

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::schedule::{
    full_backup_decision, schedule_from_json, schedule_is_due, FullBackupDecision,
};

/// 把 RFC3339 字面量解析成 UTC 时间(解析失败 → None)。与 schedule.rs 私有实现同口径。
fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// 立即触发目标模式:api 侧写入 `state.run_now = {"mode": ..., "requested_at": ...}`,
/// 编排层据此绕过 schedule 强制跑该模式一次,执行后清除 run_now。None=无立即触发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunNowTarget {
    None,
    Full,
    Wal,
    Email,
}

impl RunNowTarget {
    /// 由 mode 字符串映射;未知/缺失 → None(不误触发)。
    /// 注意:offsite 不再是独立触发模式(并入 full/wal 的 destination),故此处不接受 "offsite"。
    fn from_mode(mode: Option<&str>) -> Self {
        match mode {
            Some("full") => Self::Full,
            Some("wal") => Self::Wal,
            Some("email") => Self::Email,
            _ => Self::None,
        }
    }
}

/// 从运行态 state 解析 run_now 目标(读 `state.run_now.mode`)。
pub(super) fn parse_run_now(state: &Value) -> RunNowTarget {
    let mode = state
        .get("run_now")
        .and_then(|run_now| run_now.get("mode"))
        .and_then(Value::as_str);
    RunNowTarget::from_mode(mode)
}

/// 三种备份方式一轮的执行决策(纯函数产物)。true=本轮该跑,false=跳过。
/// 异地不在此列:是否推异地由 full/wal 各自 destination 在执行层决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BackupPlan {
    /// 本轮的立即触发目标(供编排执行后清除 run_now)。
    pub(super) run_now: RunNowTarget,
    pub(super) full: bool,
    pub(super) wal: bool,
    pub(super) email: bool,
}

impl BackupPlan {
    /// 是否有任一方式需要跑(用于编排层判断「是否值得取锁」)。
    pub(super) fn any(&self) -> bool {
        self.full || self.wal || self.email
    }
}

/// 纯决策:输入 config/state/now,输出三种方式 Run/Skip + run_now 目标。
///
/// 三种备份方式**全部独立**:full/wal/email 各读自己的 schedule + 各自 state 子对象判到期,
/// 互不依赖(用户 2026-07-01 定:邮件与全量是两种不同备份,彻底分开)。
/// `_full_succeeded` 参数为兼容编排层两相调用签名而保留,现已无任何方式依赖它(邮件已解耦全量)。
pub(super) fn plan_backup(
    config: &Value,
    state: &Value,
    now: DateTime<Utc>,
    _full_succeeded: bool,
) -> BackupPlan {
    let run_now = parse_run_now(state);
    BackupPlan {
        run_now,
        full: decide_full(config, state, now, run_now),
        wal: decide_wal(config, state, now, run_now),
        email: decide_email(config, state, now, run_now),
    }
}

/// 取某模式的 state 子对象(缺失 → 空对象),模式调度到期读各自子对象的 last_success_at。
fn mode_state(state: &Value, mode: &str) -> Value {
    state.get(mode).cloned().unwrap_or_else(|| json!({}))
}

/// 取某模式 state 子对象里的 last_success_at(用于 interval/cron 判到期)。
fn mode_last_success(state: &Value, mode: &str) -> Option<DateTime<Utc>> {
    state
        .get(mode)
        .and_then(|mode| mode.get("last_success_at"))
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_utc)
}

/// 全量决策:`full.enabled && (run_now==full || full_backup_decision==Due)`。
/// enabled 是硬门(停用即使 run_now==full 也不跑);run_now 绕过 schedule 与失败退避。
fn decide_full(config: &Value, state: &Value, now: DateTime<Utc>, run_now: RunNowTarget) -> bool {
    let full = config.get("full").cloned().unwrap_or_else(|| json!({}));
    let enabled = full.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    if !enabled {
        return false;
    }
    if run_now == RunNowTarget::Full {
        return true;
    }
    // 调度 + 失败退避判定读 full 自己的 state 子对象(last_success_at/next_attempt_at)。
    let full_state = mode_state(state, "full");
    matches!(
        full_backup_decision(&full, &full_state, now),
        FullBackupDecision::Due
    )
}

/// WAL 决策:`wal.enabled && wal.configured && (run_now==wal || full_backup_schedule 到期)`。
/// configured=false(archive/pg_basebackup 未就绪)即跳过,绝不误跑。
fn decide_wal(config: &Value, state: &Value, now: DateTime<Utc>, run_now: RunNowTarget) -> bool {
    let wal = config.get("wal").cloned().unwrap_or_else(|| json!({}));
    let enabled = wal.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let configured = wal
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled || !configured {
        return false;
    }
    if run_now == RunNowTarget::Wal {
        return true;
    }
    let schedule = schedule_from_json(wal.get("full_backup_schedule").unwrap_or(&Value::Null));
    schedule_is_due(&schedule, mode_last_success(state, "wal"), now)
}

/// 邮件决策(完全独立):`email.enabled && (run_now==email || email.schedule 到期)`。
/// 邮件是独立备份方式,按自己的 `schedule`(间隔/cron)到点取最新已存在 dump 发送,
/// **不跟随全量**(用户 2026-07-01 定:邮件与全量彻底分开);或被 run_now 强制发一次。
fn decide_email(config: &Value, state: &Value, now: DateTime<Utc>, run_now: RunNowTarget) -> bool {
    let email = config.get("email").cloned().unwrap_or_else(|| json!({}));
    let enabled = email
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return false;
    }
    if run_now == RunNowTarget::Email {
        return true;
    }
    let schedule = schedule_from_json(email.get("schedule").unwrap_or(&Value::Null));
    schedule_is_due(&schedule, mode_last_success(state, "email"), now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// 构造一份「三种方式全按 spec 默认」的 backup_config(便于逐方式改写测试)。
    /// offsite 段只留共享连接配置(无 enabled/schedule),不参与决策。
    fn base_config() -> Value {
        json!({
            "offsite": {
                "pubkey_installed": false,
                "ssh_host": "203.0.113.9",
                "retention_days": 30
            },
            "full": {
                "enabled": true,
                "exclude_traffic_logs": false,
                "schedule": {"kind": "interval", "interval_minutes": 1440, "cron": ""},
                "retention_days": 30,
                "destination": "local"
            },
            "wal": {
                "enabled": false,
                "configured": false,
                "full_backup_schedule": {"kind": "interval", "interval_minutes": 10080, "cron": ""},
                "retention_full": 4,
                "destination": "local"
            },
            "email": {
                "enabled": false,
                "recipients": ["a@x.test"],
                "oversize_mode": "notify",
                "notify_on_success": true,
                "notify_on_failure": true
            }
        })
    }

    // ---- run_now 解析 ----

    #[test]
    fn parse_run_now_maps_each_mode() {
        assert_eq!(
            parse_run_now(&json!({"run_now": {"mode": "full"}})),
            RunNowTarget::Full
        );
        // offsite 不再是独立触发模式:mode="offsite" 映射为 None(不误触发)。
        assert_eq!(
            parse_run_now(&json!({"run_now": {"mode": "offsite"}})),
            RunNowTarget::None
        );
        assert_eq!(
            parse_run_now(&json!({"run_now": {"mode": "wal"}})),
            RunNowTarget::Wal
        );
        assert_eq!(
            parse_run_now(&json!({"run_now": {"mode": "email"}})),
            RunNowTarget::Email
        );
    }

    #[test]
    fn parse_run_now_none_when_absent_or_unknown() {
        assert_eq!(parse_run_now(&json!({})), RunNowTarget::None);
        assert_eq!(
            parse_run_now(&json!({"run_now": {"mode": "bogus"}})),
            RunNowTarget::None
        );
        assert_eq!(parse_run_now(&json!({"run_now": {}})), RunNowTarget::None);
    }

    // ---- 全量 ----

    #[test]
    fn full_runs_when_enabled_and_no_success_state() {
        // 启用 + 从未成功 → 到期该跑。
        let plan = plan_backup(&base_config(), &json!({}), Utc::now(), false);
        assert!(plan.full);
    }

    #[test]
    fn full_skipped_when_disabled_even_with_run_now() {
        // enabled=false 是硬门:即便 run_now==full 也不跑。
        let mut config = base_config();
        config["full"]["enabled"] = json!(false);
        let state = json!({"run_now": {"mode": "full"}});
        let plan = plan_backup(&config, &state, Utc::now(), false);
        assert!(!plan.full);
    }

    #[test]
    fn full_forced_by_run_now_even_when_not_due() {
        // 12 分钟前刚成功(默认 1440 分钟间隔未到)但 run_now==full → 强制跑。
        let now = Utc::now();
        let state = json!({
            "run_now": {"mode": "full"},
            "full": {"last_success_at": (now - Duration::minutes(12)).to_rfc3339()}
        });
        let plan = plan_backup(&base_config(), &state, now, false);
        assert!(plan.full);
    }

    #[test]
    fn full_not_due_reads_full_substate_last_success() {
        // 到期判定读 state.full.last_success_at(而非顶层):12h 前成功 → 未到 1440 分钟。
        let now = Utc::now();
        let state = json!({"full": {"last_success_at": (now - Duration::hours(12)).to_rfc3339()}});
        assert!(!plan_backup(&base_config(), &state, now, false).full);
    }

    // ---- WAL ----

    #[test]
    fn wal_skipped_when_not_configured() {
        // enabled=true 但 configured=false → 跳过(即使 run_now==wal)。
        let mut config = base_config();
        config["wal"]["enabled"] = json!(true);
        let state = json!({"run_now": {"mode": "wal"}});
        assert!(!plan_backup(&config, &state, Utc::now(), false).wal);
    }

    #[test]
    fn wal_skipped_when_disabled() {
        let mut config = base_config();
        config["wal"]["configured"] = json!(true);
        // enabled 仍 false
        assert!(!plan_backup(&config, &json!({}), Utc::now(), false).wal);
    }

    #[test]
    fn wal_runs_when_enabled_configured_and_schedule_due() {
        // enabled+configured+从未成功 → 到期该跑。
        let mut config = base_config();
        config["wal"]["enabled"] = json!(true);
        config["wal"]["configured"] = json!(true);
        assert!(plan_backup(&config, &json!({}), Utc::now(), false).wal);
    }

    #[test]
    fn wal_forced_by_run_now_even_when_not_due() {
        let now = Utc::now();
        let mut config = base_config();
        config["wal"]["enabled"] = json!(true);
        config["wal"]["configured"] = json!(true);
        let state = json!({
            "run_now": {"mode": "wal"},
            "wal": {"last_success_at": now.to_rfc3339()}
        });
        assert!(plan_backup(&config, &state, now, false).wal);
    }

    // ---- 邮件 ----

    #[test]
    fn email_skipped_when_disabled() {
        // enabled=false:即便 run_now==email 也不发(硬门)。
        let state = json!({"run_now": {"mode": "email"}});
        assert!(!plan_backup(&base_config(), &state, Utc::now(), true).email);
    }

    #[test]
    fn email_runs_on_own_schedule_independent_of_full() {
        // 邮件按自己的 schedule 到期即发,与全量成功与否无关(全独立):
        // 无 last_success → 到期 → 发;full_succeeded=false/true 都应发(不依赖全量)。
        let mut config = base_config();
        config["email"]["enabled"] = json!(true);
        assert!(plan_backup(&config, &json!({}), Utc::now(), false).email);
        assert!(plan_backup(&config, &json!({}), Utc::now(), true).email);
    }

    #[test]
    fn email_not_due_when_recent_success_even_if_full_succeeded() {
        // 邮件刚发过(在 interval 内)→ 不到期 → 不发;哪怕全量本轮成功也不发(彻底解耦全量)。
        let mut config = base_config();
        config["email"]["enabled"] = json!(true);
        config["email"]["schedule"] =
            json!({"kind": "interval", "interval_minutes": 1440, "cron": ""});
        let now = Utc::now();
        let state =
            json!({"email": {"last_success_at": (now - Duration::minutes(60)).to_rfc3339()}});
        assert!(!plan_backup(&config, &state, now, true).email);
    }

    #[test]
    fn email_forced_by_run_now() {
        // run_now==email 绕过 schedule 强制发一次(取最新已存在 dump)。
        let mut config = base_config();
        config["email"]["enabled"] = json!(true);
        let state = json!({"run_now": {"mode": "email"}});
        assert!(plan_backup(&config, &state, Utc::now(), false).email);
    }

    // ---- 组合 / 隔离 ----

    #[test]
    fn run_now_target_surfaced_in_plan() {
        // plan 携带 run_now 目标,供编排层执行后清除。
        let state = json!({"run_now": {"mode": "wal"}});
        assert_eq!(
            plan_backup(&base_config(), &state, Utc::now(), false).run_now,
            RunNowTarget::Wal
        );
    }

    #[test]
    fn modes_decided_independently() {
        // 全开三种方式 + 全部到期:各方式互不影响,决策彼此独立(异地由 destination 决定,不在此列)。
        let now = Utc::now();
        let mut config = base_config();
        config["full"]["destination"] = json!("both");
        config["wal"]["enabled"] = json!(true);
        config["wal"]["configured"] = json!(true);
        config["email"]["enabled"] = json!(true);
        let plan = plan_backup(&config, &json!({}), now, true);
        assert!(plan.full, "全量到期");
        assert!(plan.wal, "WAL 到期");
        assert!(plan.email, "邮件跟随全量成功");
        assert!(plan.any());
    }

    #[test]
    fn plan_any_false_when_nothing_due() {
        // 全量未到期、其它方式全停用、无 run_now → 无任何方式要跑。
        let now = Utc::now();
        let state = json!({"full": {"last_success_at": now.to_rfc3339()}});
        let plan = plan_backup(&base_config(), &state, now, false);
        assert!(!plan.any());
        assert_eq!(plan.run_now, RunNowTarget::None);
    }
}
