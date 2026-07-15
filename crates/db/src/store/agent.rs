//! Agent 心跳、配置回执和令牌校验。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用；不引入 API、前端或脚本层行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn record_agent_tls_status(
        &self,
        access_node_id: Uuid,
        certificates: Vec<AgentTlsCertificateReport>,
        renew_result: Option<AgentTlsRenewResult>,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        // 先抽出每条上报的 (域名, 归一化状态),供后续按域名刷新 node_domains.cert_status(修 #60)。
        // 复用与 tls_certificates blob 相同的状态归一化,保证 blob 与 cert_status 取值一致;
        // 空/非法状态条目跳过,不写入。
        let domain_statuses = node_domain_cert_statuses(&certificates)?;
        let certificates_json = sanitize_tls_certificates(certificates)?;
        sqlx::query(
            r#"
            UPDATE access_nodes
            SET tls_certificates = $2,
                tls_cert_last_report_at = now()
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .bind(certificates_json)
        .execute(&mut *tx)
        .await?;

        // 按域名把证书状态回写到 node_domains.cert_status:每条上报只更新该节点同名域名行,
        // 不在上报里的 node_domains 不被触及(保持原值,不清成 unknown),修 #60。
        for (domain, status) in domain_statuses {
            sqlx::query(
                r#"
                UPDATE node_domains
                SET cert_status = $3
                WHERE access_node_id = $1
                  AND domain = $2
                "#,
            )
            .bind(access_node_id)
            .bind(domain)
            .bind(status)
            .execute(&mut *tx)
            .await?;
        }

        if let Some(result) = renew_result {
            let status = sanitize_tls_renew_status(&result.status)?;
            let message = truncate_tls_text(&result.message, 512);
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET tls_renew_status = $3,
                    tls_renew_message = $4,
                    tls_renew_completed_at = now()
                WHERE id = $1
                  AND tls_renew_request_id = $2
                "#,
            )
            .bind(access_node_id)
            .bind(result.request_id)
            .bind(status)
            .bind(message)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn request_access_node_tls_renewal(
        &self,
        access_node_id: Uuid,
    ) -> Result<AccessNodeTlsRenewalResult, DbError> {
        let certificates = sqlx::query_scalar::<_, Value>(
            r#"
            SELECT tls_certificates
            FROM access_nodes
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::InvalidInput("中转节点不存在".to_string()))?;

        let mut domains = tls_domains_from_certificates(&certificates);
        if domains.is_empty() {
            domains = self.access_node_tls_cert_domains(access_node_id).await?;
        }
        domains.sort();
        domains.dedup();
        if domains.is_empty() {
            return Err(DbError::InvalidInput(
                "当前节点没有可续期的 SSL 证书域名".to_string(),
            ));
        }

        let request_id = Uuid::new_v4();
        sqlx::query(
            r#"
            UPDATE access_nodes
            SET tls_renew_request_id = $2,
                tls_renew_requested_at = now(),
                tls_renew_completed_at = NULL,
                tls_renew_status = 'queued',
                tls_renew_message = '等待节点执行 SSL 证书续期'
            WHERE id = $1
            "#,
        )
        .bind(access_node_id)
        .bind(request_id)
        .execute(&self.pool)
        .await?;

        Ok(AccessNodeTlsRenewalResult {
            request_id,
            status: "queued".to_string(),
            domains,
        })
    }

    pub async fn heartbeat_json(
        &self,
        reported_node_id: Option<Uuid>,
        applied_config_hash: Option<&str>,
    ) -> Result<serde_json::Value, DbError> {
        let block_unhealthy_lines = self.subscription_blocks_unhealthy_lines().await?;
        self.sync_assignments_for_active_users().await?;
        let data = self.load_store_data().await?;
        let node_id = selected_node_id(&data, reported_node_id);
        let node = node_id.and_then(|node_id| data.access_nodes.get(&node_id));
        let access_config =
            build_access_config_with_probe_policy(&data, node_id, block_unhealthy_lines);
        let desired_config_hash = node.map(|node| {
            access_config
                .as_ref()
                .map(access_config_hash)
                .unwrap_or_else(|| empty_access_config_hash(node.id))
        });

        if let Some(node_id) = node_id {
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET last_heartbeat_at = now(),
                    desired_config_hash = COALESCE($2, desired_config_hash),
                    applied_config_hash = COALESCE($3, applied_config_hash)
                WHERE id = $1
                "#,
            )
            .bind(node_id)
            .bind(&desired_config_hash)
            .bind(applied_config_hash)
            .execute(&self.pool)
            .await?;
        }

        let mut response = heartbeat_json_with_probe_policy(
            &data,
            node_id,
            applied_config_hash,
            block_unhealthy_lines,
        );
        if let Some(node_id) = node_id {
            let probe_tasks = self.pending_probe_tasks_json(node_id).await?;
            // 多域名 Phase 3:把本节点 node_domains 清单下发给 agent,供其遍历逐域名签/续证书。
            let node_domains = self.heartbeat_node_domains_json(node_id).await?;
            if let Some(object) = response.as_object_mut() {
                object.insert("probe_tasks".to_string(), json!(probe_tasks));
                object.insert("node_domains".to_string(), node_domains);
                let renew_task = self.pending_tls_renew_task_json(node_id).await?;
                object.insert(
                    "tls_renew_task".to_string(),
                    renew_task.unwrap_or(Value::Null),
                );
                // 整机重启任务(§7.7.1):有待执行 queued 请求即下发 reboot_task,agent 自检后执行。
                let reboot_task = self.pending_reboot_task_json(node_id).await?;
                object.insert(
                    "reboot_task".to_string(),
                    reboot_task.unwrap_or(Value::Null),
                );
            }
        }

        Ok(response)
    }

    /// 心跳下发的节点 node_domains 清单(多域名 Phase 3)。
    ///
    /// 每项给 agent 签证书所需字段:domain + kind(direct/cf)+ cf_cert_mode + acme_email。
    /// agent 据此遍历:direct→HTTP-01,cf 且 dns01→DNS-01,各域名各签;reuse_direct 的 cf 复用直连证书。
    /// 字段名 `node_domains` 与 agent 子任务对齐;空清单返回 [],保证读侧必含键。
    pub(crate) async fn heartbeat_node_domains_json(
        &self,
        access_node_id: Uuid,
    ) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
            r#"
            SELECT domain, kind, cf_cert_mode, acme_email
            FROM node_domains
            WHERE access_node_id = $1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(access_node_id)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|(domain, kind, cf_cert_mode, acme_email)| {
                json!({
                    "domain": domain,
                    "kind": kind,
                    "cf_cert_mode": cf_cert_mode,
                    "acme_email": acme_email,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!(items))
    }

    pub(crate) async fn pending_tls_renew_task_json(
        &self,
        access_node_id: Uuid,
    ) -> Result<Option<Value>, DbError> {
        let row = sqlx::query_as::<_, (Uuid, Value)>(
            r#"
            SELECT tls_renew_request_id, tls_certificates
            FROM access_nodes
            WHERE id = $1
              AND tls_renew_request_id IS NOT NULL
              AND tls_renew_completed_at IS NULL
              AND tls_renew_status IN ('queued', 'running')
            "#,
        )
        .bind(access_node_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((request_id, certificates)) = row else {
            return Ok(None);
        };
        let mut domains = tls_domains_from_certificates(&certificates);
        if domains.is_empty() {
            domains = self.access_node_tls_cert_domains(access_node_id).await?;
        }
        domains.sort();
        domains.dedup();
        if domains.is_empty() {
            return Ok(None);
        }

        Ok(Some(json!({
            "request_id": request_id,
            "domains": domains
        })))
    }

    pub(crate) async fn pending_probe_tasks_json(
        &self,
        access_node_id: Uuid,
    ) -> Result<Vec<Value>, DbError> {
        let policy = self.access_probe_policy().await?;
        let rows = sqlx::query_as::<_, PendingProbeTaskRow>(
            r#"
            WITH ranked AS (
                SELECT p.id,
                       p.exit_endpoint_id,
                       p.probed_at,
                       row_number() OVER (
                           PARTITION BY p.exit_endpoint_id
                           ORDER BY p.probed_at DESC, p.id DESC
                       ) AS task_rank
                FROM access_exit_probes p
                WHERE p.access_node_id = $1
                  AND p.status = 'queued'
                  AND (
                      p.task_lease_expires_at IS NULL
                      OR p.task_lease_expires_at <= now()
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM access_exit_probes r
                      WHERE r.access_node_id = p.access_node_id
                        AND r.exit_endpoint_id = p.exit_endpoint_id
                        AND r.status <> 'queued'
                        AND r.probed_at >= p.probed_at
                  )
            ),
            claimable AS (
                SELECT id
                FROM ranked
                WHERE task_rank = 1
                ORDER BY probed_at DESC, id DESC
                LIMIT GREATEST($3::BIGINT, 1)
            ),
            locked AS (
                SELECT p.id
                FROM access_exit_probes p
                JOIN claimable c ON c.id = p.id
                FOR UPDATE OF p SKIP LOCKED
            ),
            claimed AS (
                UPDATE access_exit_probes p
                SET task_claimed_at = now(),
                    task_lease_expires_at = now() + ($2::BIGINT * interval '1 second'),
                    task_delivery_count = p.task_delivery_count + 1
                FROM locked, exit_endpoints e
                WHERE p.id = locked.id
                  AND e.id = p.exit_endpoint_id
                RETURNING p.exit_endpoint_id, p.probed_at AS requested_at,
                          e.outbound_type::text AS outbound_type, e.host, e.port
            )
            SELECT exit_endpoint_id, requested_at, outbound_type, host, port
            FROM claimed
            ORDER BY requested_at DESC, exit_endpoint_id
            "#,
        )
        .bind(access_node_id)
        .bind(PROBE_TASK_LEASE_SECONDS)
        .bind(policy.max_pending_probe_tasks)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let mut task = json!({
                    "exit_endpoint_id": row.exit_endpoint_id,
                    "requested_at": row.requested_at
                });
                if let Some(port) = probe_task_port(row.port) {
                    if !row.host.trim().is_empty() {
                        task["target"] = json!({
                            "protocol": row.outbound_type,
                            "address": row.host,
                            "port": port
                        });
                    }
                }
                task
            })
            .collect())
    }

    pub async fn record_config_result(
        &self,
        reported_node_id: Option<Uuid>,
        config_version: Option<&str>,
        success: bool,
        message: Option<&str>,
    ) -> Result<(), DbError> {
        let Some(node_id) = reported_node_id else {
            return Ok(());
        };

        if success {
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET applied_config_hash = COALESCE($2, applied_config_hash),
                    config_dirty = CASE
                        WHEN $2 IS NOT NULL
                         AND desired_config_hash IS NOT NULL
                         AND desired_config_hash = $2
                         AND (
                            config_dirty_at IS NULL
                            OR last_heartbeat_at IS NULL
                            OR last_heartbeat_at >= config_dirty_at
                         )
                        THEN FALSE ELSE config_dirty END,
                    config_dirty_at = CASE
                        WHEN $2 IS NOT NULL
                         AND desired_config_hash IS NOT NULL
                         AND desired_config_hash = $2
                         AND (
                            config_dirty_at IS NULL
                            OR last_heartbeat_at IS NULL
                            OR last_heartbeat_at >= config_dirty_at
                         )
                        THEN NULL ELSE config_dirty_at END,
                    config_dirty_reason = CASE
                        WHEN $2 IS NOT NULL
                         AND desired_config_hash IS NOT NULL
                         AND desired_config_hash = $2
                         AND (
                            config_dirty_at IS NULL
                            OR last_heartbeat_at IS NULL
                            OR last_heartbeat_at >= config_dirty_at
                         )
                        THEN '' ELSE config_dirty_reason END
                WHERE id = $1
                "#,
            )
            .bind(node_id)
            .bind(config_version)
            .execute(&self.pool)
            .await?;
        } else {
            let reason = message
                .map(|value| value.chars().take(500).collect::<String>())
                .unwrap_or_else(|| "config_apply_failed".to_string());
            sqlx::query(
                r#"
                UPDATE access_nodes
                SET config_dirty = TRUE,
                    config_dirty_at = now(),
                    desired_config_hash = NULL,
                    config_dirty_reason = $2
                WHERE id = $1
                "#,
            )
            .bind(node_id)
            .bind(reason)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn verify_agent_token(
        &self,
        reported_node_id: Option<Uuid>,
        token: &str,
    ) -> Result<bool, DbError> {
        if token.trim().is_empty() {
            return Ok(false);
        }

        let Some(node_id) = reported_node_id else {
            return Ok(false);
        };
        let expected = sqlx::query_scalar::<_, String>(
            r#"SELECT agent_token_hash FROM access_nodes WHERE id = $1"#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        let token_hash = agent_token_hash(token);
        Ok(expected
            .is_some_and(|expected| constant_time_eq(expected.as_bytes(), token_hash.as_bytes())))
    }
}

fn sanitize_tls_certificates(
    certificates: Vec<AgentTlsCertificateReport>,
) -> Result<Value, DbError> {
    let mut values = Vec::with_capacity(certificates.len());
    for certificate in certificates.into_iter().take(50) {
        let domain = truncate_tls_text(&certificate.domain, 255);
        if domain.trim().is_empty() {
            continue;
        }
        values.push(json!({
            "domain": domain,
            "status": sanitize_tls_certificate_status(&certificate.status)?,
            "not_before": certificate.not_before.map(|value| truncate_tls_text(&value, 64)),
            "not_after": certificate.not_after.map(|value| truncate_tls_text(&value, 64)),
            "days_remaining": certificate.days_remaining,
            "error_summary": truncate_tls_text(&certificate.error_summary, 512),
        }));
    }
    Ok(Value::Array(values))
}

/// 从上报证书列表抽出 (域名, 归一化状态) 对,供刷新 node_domains.cert_status(修 #60)。
/// 域名 trim 后空则跳过;状态走与 blob 相同的归一化(valid/expiring/expired/missing/invalid),
/// 保证 cert_status 取值与 tls_certificates blob 一致、与读模型/前端展示对齐。
/// 同域名多条以最后一条为准(实际上报每域名至多一条)。
fn node_domain_cert_statuses(
    certificates: &[AgentTlsCertificateReport],
) -> Result<Vec<(String, &'static str)>, DbError> {
    let mut pairs = Vec::with_capacity(certificates.len());
    for certificate in certificates {
        let domain = certificate.domain.trim();
        if domain.is_empty() {
            continue;
        }
        let status = sanitize_tls_certificate_status(&certificate.status)?;
        pairs.push((domain.to_string(), status));
    }
    Ok(pairs)
}

fn sanitize_tls_certificate_status(status: &str) -> Result<&'static str, DbError> {
    match status.trim().to_ascii_lowercase().as_str() {
        "valid" => Ok("valid"),
        "expiring" => Ok("expiring"),
        "expired" => Ok("expired"),
        "missing" => Ok("missing"),
        "invalid" => Ok("invalid"),
        _ => Err(DbError::InvalidAgentPayload("SSL 证书状态无效".to_string())),
    }
}

fn sanitize_tls_renew_status(status: &str) -> Result<&'static str, DbError> {
    match status.trim().to_ascii_lowercase().as_str() {
        "success" => Ok("success"),
        "failed" => Ok("failed"),
        "running" => Ok("running"),
        _ => Err(DbError::InvalidAgentPayload("SSL 续期状态无效".to_string())),
    }
}

// 截断脱敏文本到最大字符数(供 TLS 证书/续期/内核重启等上报字段统一截断);跨模块复用故 pub(crate)。
pub(crate) fn truncate_tls_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn tls_domains_from_certificates(certificates: &Value) -> Vec<String> {
    certificates
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("domain").and_then(Value::as_str))
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn probe_task_port(port: i32) -> Option<u16> {
    (1..=i32::from(u16::MAX))
        .contains(&port)
        .then_some(port as u16)
}
