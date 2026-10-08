//! 部署步骤只随实际报告的事件推进，不把未知错误之前的待执行步骤推定为成功。
//! 错误摘要同时保留上下文和末尾原因，避免包管理器输出掩盖最终失败。

use serde_json::{json, Value};

pub(super) fn deployment_error_summary(value: &str, limit: usize) -> String {
    let chars: Vec<char> = value.trim().chars().collect();
    if chars.len() <= limit {
        return chars.into_iter().collect();
    }
    let head = limit / 4;
    let tail = limit.saturating_sub(head + 5);
    format!(
        "{}\n[…]\n{}",
        chars[..head].iter().collect::<String>(),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

pub(super) fn update_deployment_steps_statuses(
    steps: Value,
    current_key: &str,
    task_status: &str,
    message: &str,
) -> Value {
    let mut items = steps.as_array().cloned().unwrap_or_default();
    let known_index = items
        .iter()
        .position(|item| item["key"].as_str() == Some(current_key));
    // 未知失败往往来自 SSH 包装层，只结束正在执行的步骤，后续步骤保持待执行。
    for (index, item) in items.iter_mut().enumerate() {
        if item["status"] != "current" || Some(index) == known_index {
            continue;
        }
        item["status"] = json!(match task_status {
            "failed" => "failed",
            "canceled" => "canceled",
            "running" | "succeeded" => "done",
            _ => "current",
        });
    }
    let status = match task_status {
        "succeeded" => "done",
        "failed" => "failed",
        "canceled" => "canceled",
        "running" => "current",
        _ => "pending",
    };
    if let Some(index) = known_index {
        items[index]["status"] = json!(status);
        if !message.trim().is_empty() {
            items[index]["detail"] = json!(deployment_error_summary(message, 2048));
        }
    } else if !current_key.trim().is_empty() {
        let title = match current_key {
            "preflight" => "中心地址与鉴权预检",
            "artifacts_download" => "下载安装文件",
            "runtime_prepare" => "准备运行环境",
            "ssh_install_failed" => "SSH 安装失败",
            "auth_code_missing" => "缺少节点鉴权码",
            "auth_code_invalid" => "节点鉴权码无效",
            "node_register_failed" => "登记中转节点失败",
            _ => "安装事件",
        };
        items.push(
            json!({"key": current_key.chars().take(80).collect::<String>(),
            "title": title, "detail": deployment_error_summary(message, 2048), "status": status}),
        );
    }
    Value::Array(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_failure_does_not_complete_unexecuted_steps() {
        let steps = json!([
            {"key":"created", "status":"done"},
            {"key":"ssh_connect", "status":"current"},
            {"key":"artifacts_downloaded", "status":"pending"},
            {"key":"node_registered", "status":"pending"}
        ]);
        let result =
            update_deployment_steps_statuses(steps, "ssh_install_failed", "failed", "清单下载失败");
        assert_eq!(result[0]["status"], "done");
        assert_eq!(result[1]["status"], "failed");
        assert_eq!(result[2]["status"], "pending");
        assert_eq!(result[3]["status"], "pending");
        assert_eq!(result[4]["detail"], "清单下载失败");
    }

    #[test]
    fn next_report_completes_observed_step_only() {
        let steps = json!([
            {"key":"ssh_connect", "status":"current"},
            {"key":"optional", "status":"pending"},
            {"key":"artifacts_downloaded", "status":"pending"}
        ]);
        let result =
            update_deployment_steps_statuses(steps, "artifacts_downloaded", "running", "下载完成");
        assert_eq!(result[0]["status"], "done");
        assert_eq!(result[1]["status"], "pending");
        assert_eq!(result[2]["status"], "current");
    }

    #[test]
    fn error_summary_preserves_terminal_cause_and_unicode() {
        let log = format!("安装开始{}清单下载失败", "依赖已安装\n".repeat(300));
        let summary = deployment_error_summary(&log, 512);
        assert!(summary.starts_with("安装开始"));
        assert!(summary.ends_with("清单下载失败"));
        assert!(summary.chars().count() <= 512);
    }
}
