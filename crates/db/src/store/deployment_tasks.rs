//! 部署任务持久化与进度回传。
//! 任务用于展示平台或 agent 安装进度，不执行远程 SSH 操作。
//! 明文上报 token 只在创建任务时返回给调用方，数据库只保存 hash。
//! 脚本回传只能更新状态、步骤、进度和脱敏结果摘要。
//! 本模块不保存服务器密码、代理凭据、完整命令或安装日志。
//! 管理端读取任务列表用于部署进度 UI。
//! 状态机保持简单，避免引入复杂工作流引擎。
//! 错误摘要保留开头和末尾；完整脱敏错误使用有界结果字段。
//! SQL 均通过 PgStore 连接池执行。
//! 文件头部中文注释满足仓库规则。

use super::deployment_task_progress::{deployment_error_summary, update_deployment_steps_statuses};
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEPLOYMENT_TASK_REPORT_TOKEN_BYTES: usize = 32;

impl PgStore {
    pub async fn create_deployment_task(
        &self,
        input: CreateDeploymentTaskInput,
    ) -> Result<(Value, String), DbError> {
        let token = generate_deployment_report_token();
        let token_hash = agent_token_hash(&token);
        let current_step = input
            .steps
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("key").or_else(|| item.get("title")))
            .and_then(Value::as_str)
            .unwrap_or("created")
            .to_string();
        let row = sqlx::query_as::<_, DeploymentTaskRow>(
            r#"
            INSERT INTO deployment_tasks (
                kind, status, target_type, target_id, title, summary,
                current_step, progress_percent, report_token_hash,
                safe_metadata, steps, created_by_user_id
            )
            VALUES ($1, 'waiting_for_server', $2, $3, $4, $5, $6, 5, $7, $8, $9, $10)
            RETURNING
                id, kind, status, target_type, target_id, title, summary,
                current_step, progress_percent, safe_metadata, steps, result,
                error_summary, created_by_user_id, NULL::text AS created_by_email,
                created_at, updated_at, started_at, completed_at
            "#,
        )
        .bind(normalize_deployment_task_kind(&input.kind)?)
        .bind(normalize_deployment_target_type(&input.target_type)?)
        .bind(input.target_id)
        .bind(truncate_deployment_text(&input.title, 160))
        .bind(truncate_deployment_text(&input.summary, 512))
        .bind(truncate_deployment_text(&current_step, 128))
        .bind(token_hash)
        .bind(sanitize_deployment_json(input.safe_metadata))
        .bind(sanitize_deployment_steps(input.steps))
        .bind(input.created_by_user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((deployment_task_json(row), token))
    }

    pub async fn deployment_tasks_json(&self) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, DeploymentTaskRow>(
            r#"
            SELECT
                t.id, t.kind, t.status, t.target_type, t.target_id, t.title, t.summary,
                t.current_step, t.progress_percent, t.safe_metadata, t.steps, t.result,
                t.error_summary, t.created_by_user_id, u.email AS created_by_email,
                t.created_at, t.updated_at, t.started_at, t.completed_at
            FROM deployment_tasks t
            LEFT JOIN users u ON u.id = t.created_by_user_id
            ORDER BY t.updated_at DESC, t.created_at DESC
            LIMIT 100
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "items": rows.into_iter().map(deployment_task_json).collect::<Vec<_>>()
        }))
    }

    /// 删除任意一条部署任务记录,供管理员清理失败/卡死残留。
    /// 仅删记录本身,不级联其它实体(任务表不持有外部副作用)。
    pub async fn delete_deployment_task(&self, task_id: Uuid) -> Result<(), DbError> {
        let affected = sqlx::query("DELETE FROM deployment_tasks WHERE id = $1")
            .bind(task_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected == 0 {
            return Err(DbError::InvalidInput("部署任务不存在".to_string()));
        }
        Ok(())
    }

    /// 取消一条未完成任务:仅 waiting_for_server/running 等非终态可改为 failed。
    /// 终态(succeeded/failed/canceled)保持不变并原样返回,保证状态不回退。
    pub async fn cancel_deployment_task(&self, task_id: Uuid) -> Result<Value, DbError> {
        let row = sqlx::query_as::<_, DeploymentTaskRow>(
            r#"
            UPDATE deployment_tasks AS t
            SET status = 'failed',
                progress_percent = 100,
                error_summary = '管理员取消',
                completed_at = COALESCE(t.completed_at, now()),
                updated_at = now()
            WHERE t.id = $1
              AND t.status NOT IN ('succeeded', 'failed', 'canceled')
            RETURNING
                id, kind, status, target_type, target_id, title, summary,
                current_step, progress_percent, safe_metadata, steps, result,
                error_summary, created_by_user_id, NULL::text AS created_by_email,
                created_at, updated_at, started_at, completed_at
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return Ok(deployment_task_json(row));
        }
        // 已处于终态或不存在:回读现状,存在则原样返回,不存在则报错。
        let current = sqlx::query_as::<_, DeploymentTaskRow>(
            r#"
            SELECT
                id, kind, status, target_type, target_id, title, summary,
                current_step, progress_percent, safe_metadata, steps, result,
                error_summary, created_by_user_id, NULL::text AS created_by_email,
                created_at, updated_at, started_at, completed_at
            FROM deployment_tasks
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::InvalidInput("部署任务不存在".to_string()))?;
        Ok(deployment_task_json(current))
    }

    /// 卡死自愈:把非终态任务里 updated_at/created_at 超过阈值秒数的标记为 failed。
    /// 原因记"超时未回连";succeeded/failed/canceled 终态一律不动,避免改写历史结果。
    /// 阈值取 updated_at 与 created_at 的较新者,防止刚被推进过的任务被误杀。
    /// 返回受影响行数,供 worker 日志统计。
    pub async fn mark_stale_deployment_tasks_failed(
        &self,
        threshold_seconds: i64,
    ) -> Result<u64, DbError> {
        let threshold_seconds = threshold_seconds.max(1);
        let affected = sqlx::query(
            r#"
            UPDATE deployment_tasks
            SET status = 'failed',
                progress_percent = 100,
                error_summary = '超时未回连',
                completed_at = COALESCE(completed_at, now()),
                updated_at = now()
            WHERE status NOT IN ('succeeded', 'failed', 'canceled')
              AND GREATEST(updated_at, created_at) <= now() - ($1::BIGINT * interval '1 second')
            "#,
        )
        .bind(threshold_seconds)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(affected)
    }
}

pub(super) fn deployment_task_json(row: DeploymentTaskRow) -> Value {
    let steps = deployment_steps_for_read(
        row.steps,
        &row.current_step,
        &row.status,
        row.error_summary.as_str(),
    );
    json!({
        "id": row.id,
        "kind": row.kind,
        "status": row.status,
        "target_type": row.target_type,
        "target_id": row.target_id,
        "title": row.title,
        "summary": row.summary,
        "current_step": row.current_step,
        "progress_percent": row.progress_percent.clamp(0, 100),
        "safe_metadata": row.safe_metadata,
        "steps": steps,
        "result": row.result,
        "error_summary": row.error_summary,
        "created_by_user_id": row.created_by_user_id,
        "created_by_email": row.created_by_email.unwrap_or_default(),
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "started_at": row.started_at,
        "completed_at": row.completed_at
    })
}

fn deployment_steps_for_read(
    steps: Value,
    current_step: &str,
    task_status: &str,
    message: &str,
) -> Value {
    if matches!(task_status, "succeeded" | "failed" | "canceled") {
        update_deployment_steps_statuses(steps, current_step, task_status, message)
    } else {
        steps
    }
}

fn generate_deployment_report_token() -> String {
    let mut bytes = [0_u8; DEPLOYMENT_TASK_REPORT_TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn normalize_deployment_task_kind(value: &str) -> Result<String, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "agent_install" | "platform_install" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err(DbError::InvalidInput("部署任务类型无效".to_string())),
    }
}

fn normalize_deployment_target_type(value: &str) -> Result<String, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "access_agent" | "platform" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err(DbError::InvalidInput("部署目标类型无效".to_string())),
    }
}

pub(super) fn normalize_deployment_task_status(value: &str) -> Result<String, DbError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "waiting_for_server" | "running" | "succeeded" | "failed" | "canceled" => {
            Ok(value.trim().to_ascii_lowercase())
        }
        _ => Err(DbError::InvalidInput("部署任务状态无效".to_string())),
    }
}

pub(super) fn default_deployment_progress(status: &str) -> i32 {
    match status {
        "waiting_for_server" => 5,
        "running" => 50,
        "succeeded" => 100,
        "failed" | "canceled" => 100,
        _ => 0,
    }
}

fn sanitize_deployment_steps(value: Value) -> Value {
    let Some(items) = value.as_array() else {
        return json!([]);
    };
    json!(items
        .iter()
        .take(20)
        .map(|item| {
            json!({
                "key": truncate_deployment_text(
                    item.get("key").or_else(|| item.get("title")).and_then(Value::as_str).unwrap_or("step"),
                    80
                ),
                "title": truncate_deployment_text(
                    item.get("title").and_then(Value::as_str).unwrap_or("步骤"),
                    120
                ),
                "detail": truncate_deployment_text(
                    item.get("detail").or_else(|| item.get("message")).and_then(Value::as_str).unwrap_or(""),
                    512
                ),
                "status": truncate_deployment_text(
                    item.get("status").and_then(Value::as_str).unwrap_or("pending"),
                    32
                )
            })
        })
        .collect::<Vec<_>>())
}

pub(super) fn sanitize_deployment_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.to_ascii_lowercase().as_str(),
                        "token"
                            | "password"
                            | "secret"
                            | "authorization"
                            | "agent_token"
                            | "report_token"
                            | "deploy_artifact_token"
                            | "command"
                            | "install_command"
                            | "ssh_password"
                            | "ssh_private_key"
                            | "cf_api_token"
                            | "auth_code"
                    )
                })
                .map(|(key, value)| (key, sanitize_deployment_json(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .take(50)
                .map(sanitize_deployment_json)
                .collect(),
        ),
        Value::String(value) => Value::String(deployment_error_summary(&value, 16_384)),
        other => other,
    }
}

pub(super) fn truncate_deployment_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_result_redacts_nested_secrets_and_keeps_failure_tail() {
        let result = sanitize_deployment_json(json!({
            "nested": [{"PASSWORD": "hidden-password", "status": "failed"}],
            "error": format!("{}final-download-failure", "dependency output\n".repeat(1500))
        }));
        assert!(!result.to_string().contains("hidden-password"));
        assert!(result["error"]
            .as_str()
            .unwrap()
            .ends_with("final-download-failure"));
        assert_eq!(result["nested"][0]["status"], "failed");
    }

    #[test]
    fn deployment_task_json_reconciles_stale_steps_with_terminal_status() {
        let now = Utc::now();
        let row = DeploymentTaskRow {
            id: Uuid::new_v4(),
            kind: "agent_install".to_string(),
            status: "succeeded".to_string(),
            target_type: "access_agent".to_string(),
            target_id: None,
            title: "Agent 一键安装".to_string(),
            summary: "Agent 一键安装完成".to_string(),
            current_step: "node_registered".to_string(),
            progress_percent: 100,
            safe_metadata: json!({"mode": "one_click"}),
            steps: json!([
                {"key": "one_click_requested", "title": "创建安装任务", "detail": "", "status": "done"},
                {"key": "ssh_connect", "title": "连接服务器", "detail": "", "status": "current"},
                {"key": "node_registered", "title": "登记中转节点", "detail": "", "status": "pending"}
            ]),
            result: json!({}),
            error_summary: String::new(),
            created_by_user_id: None,
            created_by_email: None,
            created_at: now,
            updated_at: now,
            started_at: Some(now),
            completed_at: Some(now),
        };

        let task = deployment_task_json(row);
        let steps = task["steps"].as_array().unwrap();
        assert_eq!(steps[1]["status"], "done");
        assert_eq!(steps[2]["status"], "done");
    }
}
