//! run_now 立即触发标记的并发安全处理:持久化保留 + 按 requested_at 条件清除。
//! 红线(真机踩到的问题):备份轮把整份 `database_backup_state` 反复写回 DB;若轮进行中
//! API 端新设了 run_now(管理员在上一轮长 rsync 未完时又点「立即运行」),会被轮内旧内存
//! state 的写回覆盖丢失——表现为「点了立即运行却没反应」。故本模块提供两个纯函数:
//! ① preserve_run_now:轮内每次写回都以 DB 现存 run_now 为准注入,写回永不覆盖新标记;
//! ② should_clear_run_now:轮末只清「本轮消费的那条」(requested_at 一致才清),
//!    轮内新设的(requested_at 不同)留给下一轮消费,不被误清。
//! 本模块只做纯 JSON 决策,不碰 DB/IO;实际读写由 mod.rs 薄接线。

use serde_json::Value;

/// 读 run_now 标记的 requested_at(某次触发请求的唯一标识);无标记或无该字段返回 None。
pub(super) fn run_now_requested_at(state: &Value) -> Option<String> {
    state
        .get("run_now")
        .and_then(|run_now| run_now.get("requested_at"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 把 DB 现存的 run_now 注入到「待写回的 state」:DB 有则覆盖、无则移除。
/// 使备份轮的整份写回永远以 DB 最新 run_now 为准,不会用轮内旧内存 state 覆盖掉
/// 「轮进行中 API 新设的 run_now」。只动 run_now 键,其它模式态原样保留。纯函数。
pub(super) fn preserve_run_now(mut to_write: Value, db_state: &Value) -> Value {
    let db_run_now = db_state.get("run_now").cloned();
    if let Some(object) = to_write.as_object_mut() {
        match db_run_now {
            Some(run_now) => {
                object.insert("run_now".to_string(), run_now);
            }
            None => {
                object.remove("run_now");
            }
        }
    }
    to_write
}

/// 是否清除 run_now:仅当「DB 现存 requested_at」与「本轮消费的 requested_at」一致时才清。
/// 不一致 → 轮内被 API 换了新标记,不清、留给下一轮;任一为 None → 不清(无可消费/已被清)。纯函数。
pub(super) fn should_clear_run_now(consumed: Option<&str>, current: Option<&str>) -> bool {
    matches!((consumed, current), (Some(c), Some(cur)) if c == cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserve_run_now_keeps_db_marker_over_stale_in_memory() {
        // 轮内内存 state 带旧 run_now(A);DB 已被 API 换成新的(B)→ 写回应保留 B、不回退成 A,
        // 且模式态(full)不受影响。
        let to_write = json!({
            "full": {"status": "success"},
            "run_now": {"mode": "full", "requested_at": "A"}
        });
        let db = json!({"run_now": {"mode": "full", "requested_at": "B"}});
        let out = preserve_run_now(to_write, &db);
        assert_eq!(out["run_now"]["requested_at"], "B");
        assert_eq!(out["full"]["status"], "success");
    }

    #[test]
    fn preserve_run_now_removes_when_db_has_none() {
        // DB 已无 run_now(上一轮已清)→ 待写回里的旧 run_now 应被移除,避免复活已消费标记。
        let to_write = json!({"full": {}, "run_now": {"mode": "full", "requested_at": "A"}});
        let db = json!({"full": {}});
        let out = preserve_run_now(to_write, &db);
        assert!(out.get("run_now").is_none());
    }

    #[test]
    fn should_clear_only_when_requested_at_matches_consumed() {
        assert!(should_clear_run_now(Some("A"), Some("A")));
        assert!(!should_clear_run_now(Some("A"), Some("B"))); // 轮内被换 → 留给下一轮
        assert!(!should_clear_run_now(Some("A"), None)); // 已被清 → 无需再清
        assert!(!should_clear_run_now(None, Some("B")));
        assert!(!should_clear_run_now(None, None));
    }

    #[test]
    fn run_now_requested_at_reads_field_or_none() {
        let with = json!({"run_now": {"mode": "full", "requested_at": "2026-07-01T00:00:00Z"}});
        assert_eq!(
            run_now_requested_at(&with).as_deref(),
            Some("2026-07-01T00:00:00Z")
        );
        assert_eq!(run_now_requested_at(&json!({})), None);
        assert_eq!(
            run_now_requested_at(&json!({"run_now": {"mode": "full"}})),
            None
        );
    }
}
