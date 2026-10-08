//! 部署事件串行落库；远端安装成功与控制面登记成功分开，取消/结束后拒绝迟到推进。
use super::deployment_task_progress::{deployment_error_summary, update_deployment_steps_statuses};
use super::deployment_tasks::*;
use super::json_util::merge_json;
use super::rows::*;
use super::security::*;
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    pub async fn record_deployment_task_report(
        &self,
        input: DeploymentTaskReportInput,
    ) -> Result<Value, DbError> {
        self.record_deployment_task_report_inner(input, false).await
    }
    pub async fn record_internal_deployment_task_report(
        &self,
        input: DeploymentTaskReportInput,
    ) -> Result<Value, DbError> {
        self.record_deployment_task_report_inner(input, true).await
    }
    /// 正在执行的 SSH 任务续租；明确取消/删除才停止，不把短暂数据库错误推断为取消。
    pub async fn touch_active_deployment_task(&self, task_id: Uuid) -> Result<bool, DbError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT status,error_summary FROM deployment_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, error)) = row else {
            return Ok(false);
        };
        if status == "canceled" || error == "管理员取消" {
            return Ok(false);
        }
        sqlx::query("UPDATE deployment_tasks SET updated_at=now() WHERE id=$1 AND status IN ('running','waiting_for_server')")
            .bind(task_id).execute(&self.pool).await?;
        Ok(true)
    }
    pub async fn deployment_task_allows_registration(
        &self,
        task_id: Uuid,
    ) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deployment_tasks WHERE id=$1 AND status IN ('running','waiting_for_server'))")
            .bind(task_id).fetch_one(&self.pool).await?)
    }
    async fn record_deployment_task_report_inner(
        &self,
        input: DeploymentTaskReportInput,
        internal: bool,
    ) -> Result<Value, DbError> {
        let mut tx = self.pool.begin().await?;
        let Some((token_hash, current_steps, metadata, previous_result, previous_status, previous_error)) = sqlx::query_as::<_, (String, Value, Value, Value, String, String)>(
            "SELECT report_token_hash, steps, safe_metadata, result, status, error_summary FROM deployment_tasks WHERE id = $1 FOR UPDATE",
        )
        .bind(input.task_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Err(DbError::InvalidInput("部署任务不存在".to_string()));
        };
        if token_hash != agent_token_hash(input.report_token.trim()) {
            return Err(DbError::InvalidInput("部署任务上报鉴权失败".to_string()));
        }

        let mut normalized_status = input
            .status
            .as_deref()
            .map(normalize_deployment_task_status)
            .transpose()?
            .unwrap_or_else(|| "running".to_string());
        if !internal && metadata["mode"] == "one_click" && normalized_status == "succeeded" {
            normalized_status = "running".to_string();
        }
        if previous_error == "管理员取消"
            || previous_status == "canceled"
            || (matches!(previous_status.as_str(), "failed" | "succeeded")
                && normalized_status != previous_status)
        {
            return Err(DbError::InvalidInput(
                "部署任务已结束，不能由迟到报告重新开启".to_string(),
            ));
        }
        let current_step = input
            .step
            .as_deref()
            .map(|value| truncate_deployment_text(value, 128))
            .unwrap_or_else(|| normalized_status.clone());
        let progress = input
            .progress_percent
            .unwrap_or_else(|| default_deployment_progress(&normalized_status))
            .clamp(
                0,
                if normalized_status == "running" {
                    95
                } else {
                    100
                },
            );
        let error_summary = if normalized_status == "failed" {
            deployment_error_summary(
                input
                    .message
                    .as_deref()
                    .unwrap_or(if previous_error.is_empty() {
                        "部署任务失败"
                    } else {
                        &previous_error
                    }),
                512,
            )
        } else {
            String::new()
        };
        let result_patch = input.result.unwrap_or_else(|| {
            input
                .message
                .as_ref()
                .map(|message| json!({"message":message}))
                .unwrap_or_else(|| json!({}))
        });
        let safe_result = sanitize_deployment_json(merge_json(previous_result, result_patch));
        let updated_steps = update_deployment_steps_statuses(
            current_steps,
            &current_step,
            &normalized_status,
            input.message.as_deref().unwrap_or(""),
        );
        let row = sqlx::query_as::<_, DeploymentTaskRow>(
            r#"
            UPDATE deployment_tasks
            SET status = $2,
                current_step = $3,
                progress_percent = GREATEST(progress_percent, $4),
                result = $5,
                error_summary = $6,
                steps = $7,
                started_at = COALESCE(started_at, now()),
                completed_at = CASE
                    WHEN $2 IN ('succeeded', 'failed', 'canceled') THEN COALESCE(completed_at, now())
                    ELSE NULL
                END,
                updated_at = now()
            WHERE id = $1
            RETURNING
                id, kind, status, target_type, target_id, title, summary,
                current_step, progress_percent, safe_metadata, steps, result,
                error_summary, created_by_user_id, NULL::text AS created_by_email,
                created_at, updated_at, started_at, completed_at
        "#,
        )
        .bind(input.task_id)
        .bind(normalized_status)
        .bind(current_step)
        .bind(progress)
        .bind(safe_result)
        .bind(error_summary)
        .bind(updated_steps)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(deployment_task_json(row))
    }
}
