//! 节点内核能力软状态落库与整机重启请求/命令通道(§7.7.1)。
//! 本文件从 agent.rs 拆出,守单文件 550 行硬上限,保持原有 SQL 与流程。
//! 内核状态:agent 自检 act_connmark 可加载性 + 是否已装新内核待重启,经心跳上报落库。
//! 整机重启:管理员从面板触发,记 queued 请求经心跳 reboot_task 下发,agent 自检后执行。
//! api 侧绝不直接 SSH、不存任何凭据(§13);只在节点行写软状态与请求标记。
//! 重启结果据 request_id 幂等清理待执行请求,迟到/重复结果不误清新请求。
//! 状态只接受归一化白名单,非法状态拒绝入库。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 注释使用中文,方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::agent::truncate_tls_text;
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    /// 落库 agent 上报的内核能力软状态 + 整机重启执行结果(§7.7.1)。
    ///
    /// 内核字段缺省(旧 agent 不上报)时用 COALESCE 保留原值,不误清成不可用/待重启;
    /// 有上报则刷新并记 kernel_status_last_report_at。重启结果据 request_id 幂等清理:
    /// 只更新 reboot_request_id 匹配且未完成的行,把状态/摘要落库并置 completed_at,
    /// 避免迟到/重复结果误清新一轮请求。状态只接受归一化白名单,非法状态拒绝入库。
    pub async fn record_agent_kernel_and_reboot_status(
        &self,
        access_node_id: Uuid,
        kernel_connmark_available: Option<bool>,
        kernel_upgrade_pending: Option<bool>,
        reboot_result: Option<(Uuid, String, String)>,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        // 内核软状态:只有本次上报带值才刷新,缺省保留原值(向后兼容旧 agent)。
        // 任一字段有值即更新 kernel_status_last_report_at,供面板展示"最近上报时刻"。
        if kernel_connmark_available.is_some() || kernel_upgrade_pending.is_some() {
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET kernel_connmark_available = COALESCE($2, kernel_connmark_available),
                    kernel_upgrade_pending = COALESCE($3, kernel_upgrade_pending),
                    kernel_status_last_report_at = now()
                WHERE id = $1
                "#,
            )
            .bind(access_node_id)
            .bind(kernel_connmark_available)
            .bind(kernel_upgrade_pending)
            .execute(&mut *tx)
            .await?;
        }

        if let Some((request_id, status, message)) = reboot_result {
            let status = normalized_reboot_result(&status, &message)?;
            let message = truncate_tls_text(&message, 512);
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET reboot_status = $3,
                    reboot_message = $4,
                    reboot_completed_at = CASE WHEN $3 IN ('success', 'failed') THEN now() ELSE NULL END
                WHERE id = $1
                  AND reboot_request_id = $2
                  AND reboot_completed_at IS NULL
                "#,
            )
            .bind(access_node_id)
            .bind(request_id)
            .bind(status)
            .bind(message)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// 管理员从面板触发整机重启:记一个待执行重启请求(经心跳命令通道下发给 agent)。
    ///
    /// api 侧绝不直接 SSH、不存任何凭据(§13);只在节点行上写 reboot_request_id +
    /// queued 状态,由 agent 心跳取出 reboot_task、做安全自检后执行,再回报结果清理。
    /// 节点不存在时拒绝;已有未完成 queued/running 请求时返回原请求(幂等不堆叠)。
    pub async fn request_access_node_reboot(
        &self,
        access_node_id: Uuid,
    ) -> Result<AccessNodeRebootResult, DbError> {
        // 已有未完成的重启请求则不重复下发,返回现有请求(幂等,避免堆叠多次重启)。
        let existing = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT reboot_request_id
            FROM access_nodes
            WHERE id = $1
              AND reboot_request_id IS NOT NULL
              AND reboot_completed_at IS NULL
              AND reboot_status IN ('queued', 'running')
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(request_id) = existing {
            return Ok(AccessNodeRebootResult {
                request_id,
                status: "queued".to_string(),
            });
        }

        let request_id = Uuid::new_v4();
        let affected = sqlx::query(
            r#"
            UPDATE access_nodes
            SET reboot_request_id = $2,
                reboot_requested_at = now(),
                reboot_completed_at = NULL,
                reboot_status = 'queued',
                reboot_message = '等待节点执行整机重启(agent 安全自检后执行)'
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .bind(request_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(DbError::InvalidInput("中转节点不存在".to_string()));
        }

        Ok(AccessNodeRebootResult {
            request_id,
            status: "queued".to_string(),
        })
    }

    /// 心跳下发的待执行整机重启任务(§7.7.1):只带 request_id,不含任何凭据。
    /// 只取该节点已下请求但未完成的 queued 行;无则 None,agent 据此不重启。
    pub(crate) async fn pending_reboot_task_json(
        &self,
        access_node_id: Uuid,
    ) -> Result<Option<Value>, DbError> {
        let row = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT reboot_request_id
            FROM access_nodes
            WHERE id = $1
              AND reboot_request_id IS NOT NULL
              AND reboot_completed_at IS NULL
              AND reboot_status IN ('queued', 'running')
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|request_id| json!({ "request_id": request_id })))
    }
}

/// 归一化 agent 回报的整机重启状态:只接受白名单,非法状态拒绝入库(§7.7.1)。
fn sanitize_reboot_status(status: &str) -> Result<&'static str, DbError> {
    match status.trim().to_ascii_lowercase().as_str() {
        "success" => Ok("success"),
        "failed" => Ok("failed"),
        "running" => Ok("running"),
        _ => Err(DbError::InvalidAgentPayload("整机重启状态无效".to_string())),
    }
}

/// 兼容旧 Agent 的空命令误报，控制台不把明确未执行的重启显示为成功。
fn normalized_reboot_result(status: &str, message: &str) -> Result<&'static str, DbError> {
    let status = sanitize_reboot_status(status)?;
    Ok(if status == "success" && message.contains("未实际重启") {
        "failed"
    } else {
        status
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_empty_reboot_command_cannot_be_successful() {
        assert_eq!(
            normalized_reboot_result("success", "重启自检通过;重启命令未配置,未实际重启").unwrap(),
            "failed"
        );
        assert_eq!(
            normalized_reboot_result("success", "安全自检通过,已下达整机重启").unwrap(),
            "success"
        );
    }
}
