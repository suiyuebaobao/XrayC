//! 备份调度判定:决定"是否到期该跑一次备份"。
//! 按 schedule 对象支持 interval(分钟)/cron/after_full 三种调度语义,
//! 并综合失败退避 next_attempt_at 给出 full_backup_decision(供 mod.rs 编排层直接映射结果)。
//! 纯函数、无副作用,便于单测覆盖各种边界。本头部满足中文说明约束。

use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use serde_json::Value;

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// 缺省全量间隔(分钟):schedule 对象缺失/非法时的兜底值,对齐 spec §4 的 1440(每天)。
const DEFAULT_INTERVAL_MINUTES: i64 = 1_440;

/// 备份调度种类(标准化后的判 due 依据)。AfterFull 分支目前仅在
/// kind=="after_full" 时构造(异地/邮件跟随全量成功触发的后续 task 才会走时间外触发)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleKind {
    /// 固定间隔:`now >= last_success + interval_minutes`(至少 1 分钟)。
    Interval { interval_minutes: i64 },
    /// cron 表达式:按"上次触发点"判 due(见 schedule_is_due)。expr 保存原始串。
    Cron { expr: String },
    /// 跟随全量成功即触发:不走时间判定,永远返回 not due(由编排层主动触发)。
    AfterFull,
}

/// 解析 backup_config 里的 schedule 对象(`{kind,interval_minutes,cron}`)→ ScheduleKind。
/// 非法回退:未知/缺失 kind、cron 缺 expr 或 expr 语法非法,统一回退到默认 Interval;
/// interval_minutes 缺失取默认 1440,取值 <1 归一到 1。
pub fn schedule_from_json(value: &Value) -> ScheduleKind {
    let default = ScheduleKind::Interval {
        interval_minutes: DEFAULT_INTERVAL_MINUTES,
    };
    match value.get("kind").and_then(Value::as_str) {
        Some("interval") => {
            // 缺 interval_minutes 取默认 1440;<1 归一到 1(至少每分钟一次,避免 0/负数)。
            let interval_minutes = value
                .get("interval_minutes")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_INTERVAL_MINUTES)
                .max(1);
            ScheduleKind::Interval { interval_minutes }
        }
        Some("cron") => {
            let expr = value.get("cron").and_then(Value::as_str).unwrap_or("");
            // 空/语法非法一律回退默认 Interval,保证 ScheduleKind::Cron 内恒为可解析表达式。
            if parse_cron(expr).is_some() {
                ScheduleKind::Cron {
                    expr: expr.trim().to_string(),
                }
            } else {
                default
            }
        }
        Some("after_full") => ScheduleKind::AfterFull,
        // 未知/缺失 kind → 安全回退默认 Interval。
        _ => default,
    }
}

/// 是否到期。
/// - last_success 为 None:一律 due(还没成功过)。
/// - Interval:`now >= last_success + interval_minutes`。
/// - Cron:存在触发点 t 满足 `last_success < t <= now` 即 due。
/// - AfterFull:恒 false(由编排在全量成功后主动触发,不走时间判定)。
pub fn schedule_is_due(
    kind: &ScheduleKind,
    last_success: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    match kind {
        ScheduleKind::Interval { interval_minutes } => match last_success {
            None => true,
            Some(last) => now >= last + Duration::minutes((*interval_minutes).max(1)),
        },
        ScheduleKind::Cron { expr } => {
            // 从未成功过一律 due。
            let Some(last) = last_success else {
                return true;
            };
            // 表达式不可解析时 fail-safe:不判 due(避免误触发);写入侧 schedule_from_json 已拦非法。
            let Some(schedule) = parse_cron(expr) else {
                return false;
            };
            // after 返回严格晚于 last 的首个触发点 t(last < t);t <= now 即 (last, now] 内有触发点。
            match schedule.after(&last).next() {
                Some(next_fire) => next_fire <= now,
                None => false,
            }
        }
        // 跟随全量成功即触发:不走时间判定。
        ScheduleKind::AfterFull => false,
    }
}

/// 全量备份的到期判定结果(供 plan 决策层映射为「是否跑全量」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FullBackupDecision {
    /// full.enabled=false:整体停用,不跑全量。
    Disabled,
    /// 未到期(schedule 未触发或失败退避未过)。
    NotDue,
    /// 到期该跑一次全量。
    Due,
}

/// 全量备份是否到期的纯逻辑(读配置→判定,便于单测,不碰 DB)。
/// 综合三层判据:
/// 1. gate——`full.enabled=false` 直接 Disabled(不跑全量);
/// 2. 失败退避——运行态 `next_attempt_at` 未到即使调度到期也 NotDue(沿用旧 backup_due 退避语义);
/// 3. 调度——按 `full.schedule` 判到期,`last_success` 取运行态 `last_success_at`(无则一律到期)。
pub(super) fn full_backup_decision(
    full: &Value,
    state: &Value,
    now: DateTime<Utc>,
) -> FullBackupDecision {
    // gate:整体停用时不读调度、不跑全量。缺字段按默认启用(与 normalize 默认一致)。
    let enabled = full.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    if !enabled {
        return FullBackupDecision::Disabled;
    }
    // 失败退避优先:重试时刻未到就不跑,避免失败后每个 tick 猛冲。
    if backoff_active(state, now) {
        return FullBackupDecision::NotDue;
    }
    let schedule = schedule_from_json(full.get("schedule").unwrap_or(&Value::Null));
    let last_success = state
        .get("last_success_at")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_utc);
    if schedule_is_due(&schedule, last_success, now) {
        FullBackupDecision::Due
    } else {
        FullBackupDecision::NotDue
    }
}

/// 失败退避是否仍生效:运行态 `next_attempt_at` 存在且晚于 now 即处于退避窗口内。
fn backoff_active(state: &Value, now: DateTime<Utc>) -> bool {
    state
        .get("next_attempt_at")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_utc)
        .is_some_and(|next_attempt_at| now < next_attempt_at)
}

/// 把 backup_config 的 cron 串标准化后交给 `cron` crate 解析。
/// spec §6 采用 5 段(min hour dom month dow),而 `cron` crate 要求带秒的 6/7 段,
/// 故 5 段时前置 "0"(秒=0)补齐;6/7 段或 `@` 简写原样透传。解析失败返回 None。
fn parse_cron(expr: &str) -> Option<Schedule> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if trimmed.starts_with('@') || trimmed.split_whitespace().count() != 5 {
        trimmed.to_string()
    } else {
        format!("0 {trimmed}")
    };
    Schedule::from_str(&normalized).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试小工具:把 RFC3339 字面量解析成 `DateTime<Utc>`。
    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn interval_due_without_last_success() {
        let kind = ScheduleKind::Interval {
            interval_minutes: 60,
        };
        assert!(schedule_is_due(&kind, None, Utc::now()));
    }

    #[test]
    fn interval_not_due_before_interval_elapsed() {
        let now = Utc::now();
        let kind = ScheduleKind::Interval {
            interval_minutes: 60,
        };
        assert!(!schedule_is_due(
            &kind,
            Some(now - Duration::minutes(30)),
            now
        ));
    }

    #[test]
    fn interval_due_after_interval_elapsed() {
        let now = Utc::now();
        let kind = ScheduleKind::Interval {
            interval_minutes: 60,
        };
        // 超过间隔 → due
        assert!(schedule_is_due(
            &kind,
            Some(now - Duration::minutes(90)),
            now
        ));
        // 恰好等于间隔 → 也算到期
        assert!(schedule_is_due(
            &kind,
            Some(now - Duration::minutes(60)),
            now
        ));
    }

    #[test]
    fn after_full_is_never_due_by_time() {
        let now = Utc::now();
        assert!(!schedule_is_due(&ScheduleKind::AfterFull, None, now));
        assert!(!schedule_is_due(
            &ScheduleKind::AfterFull,
            Some(now - Duration::days(30)),
            now
        ));
    }

    #[test]
    fn cron_due_without_last_success() {
        let kind = ScheduleKind::Cron {
            expr: "0 0 * * *".to_string(),
        };
        assert!(schedule_is_due(&kind, None, Utc::now()));
    }

    #[test]
    fn cron_due_when_trigger_between_last_success_and_now() {
        // 每天 00:00 触发;上次成功在 06-30 12:00,现在已过 07-01 00:00 触发点 → due。
        let kind = ScheduleKind::Cron {
            expr: "0 0 * * *".to_string(),
        };
        let last = dt("2026-06-30T12:00:00Z");
        let now = dt("2026-07-01T01:00:00Z");
        assert!(schedule_is_due(&kind, Some(last), now));
    }

    #[test]
    fn cron_not_due_when_no_trigger_since_last_success() {
        // 恰在 07-01 00:00 触发点成功,当天再无触发点(下一是 07-02 00:00)→ not due。
        let kind = ScheduleKind::Cron {
            expr: "0 0 * * *".to_string(),
        };
        let last = dt("2026-07-01T00:00:00Z");
        let now = dt("2026-07-01T06:00:00Z");
        assert!(!schedule_is_due(&kind, Some(last), now));
    }

    #[test]
    fn schedule_from_json_parses_interval() {
        let value = json!({"kind": "interval", "interval_minutes": 30, "cron": ""});
        assert_eq!(
            schedule_from_json(&value),
            ScheduleKind::Interval {
                interval_minutes: 30
            }
        );
    }

    #[test]
    fn schedule_from_json_parses_cron() {
        let value = json!({"kind": "cron", "interval_minutes": 1440, "cron": "0 0 * * *"});
        assert_eq!(
            schedule_from_json(&value),
            ScheduleKind::Cron {
                expr: "0 0 * * *".to_string()
            }
        );
    }

    #[test]
    fn schedule_from_json_parses_after_full() {
        assert_eq!(
            schedule_from_json(&json!({"kind": "after_full"})),
            ScheduleKind::AfterFull
        );
    }

    #[test]
    fn schedule_from_json_falls_back_to_interval_on_invalid() {
        let default = ScheduleKind::Interval {
            interval_minutes: DEFAULT_INTERVAL_MINUTES,
        };
        // 未知 kind → 默认 Interval
        assert_eq!(schedule_from_json(&json!({"kind": "nope"})), default);
        // 缺 kind → 默认 Interval
        assert_eq!(schedule_from_json(&json!({})), default);
        // interval 缺 interval_minutes → 默认 1440
        assert_eq!(schedule_from_json(&json!({"kind": "interval"})), default);
        // interval_minutes < 1 → 归一到 1
        assert_eq!(
            schedule_from_json(&json!({"kind": "interval", "interval_minutes": 0})),
            ScheduleKind::Interval {
                interval_minutes: 1
            }
        );
        // cron 缺 expr / expr 为空 → 回退默认 Interval
        assert_eq!(schedule_from_json(&json!({"kind": "cron"})), default);
        assert_eq!(
            schedule_from_json(&json!({"kind": "cron", "cron": ""})),
            default
        );
        // cron expr 语法非法 → 回退默认 Interval
        assert_eq!(
            schedule_from_json(&json!({"kind": "cron", "cron": "not a cron"})),
            default
        );
    }

    /// 构造 spec §4 的默认 full 配置片段(schedule interval 1440 分钟 = 每天)。
    fn default_full() -> Value {
        json!({
            "enabled": true,
            "exclude_traffic_logs": false,
            "schedule": {"kind": "interval", "interval_minutes": 1440, "cron": ""},
            "retention_days": 30
        })
    }

    #[test]
    fn full_backup_disabled_when_enabled_false() {
        let mut full = default_full();
        full["enabled"] = json!(false);
        assert_eq!(
            full_backup_decision(&full, &json!({}), Utc::now()),
            FullBackupDecision::Disabled
        );
    }

    #[test]
    fn full_backup_due_without_success_state() {
        // 启用且从未成功过 → 立即到期。
        assert_eq!(
            full_backup_decision(&default_full(), &json!({}), Utc::now()),
            FullBackupDecision::Due
        );
    }

    #[test]
    fn full_backup_default_schedule_matches_daily_behavior() {
        // 默认 1440 分钟调度与旧「每天一次」等价:12h 前成功 → 未到期;2 天前成功 → 到期。
        let now = Utc::now();
        let recent = json!({"last_success_at": (now - Duration::hours(12)).to_rfc3339()});
        assert_eq!(
            full_backup_decision(&default_full(), &recent, now),
            FullBackupDecision::NotDue
        );
        let stale = json!({"last_success_at": (now - Duration::days(2)).to_rfc3339()});
        assert_eq!(
            full_backup_decision(&default_full(), &stale, now),
            FullBackupDecision::Due
        );
    }

    #[test]
    fn full_backup_respects_failure_backoff() {
        // 失败退避:next_attempt_at 未到即使从未成功也不跑;退避已过则恢复到期。
        let now = Utc::now();
        let pending = json!({"next_attempt_at": (now + Duration::minutes(30)).to_rfc3339()});
        assert_eq!(
            full_backup_decision(&default_full(), &pending, now),
            FullBackupDecision::NotDue
        );
        let elapsed = json!({"next_attempt_at": (now - Duration::minutes(1)).to_rfc3339()});
        assert_eq!(
            full_backup_decision(&default_full(), &elapsed, now),
            FullBackupDecision::Due
        );
    }
}
