//! 用户出口端点分配和活跃用户同步。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
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
    pub(crate) async fn sync_user_exit_assignments(&self, user_id: Uuid) -> Result<(), DbError> {
        let block_unhealthy_lines = self.subscription_blocks_unhealthy_lines().await?;
        let mut tx = self.pool.begin().await?;
        let user_active = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM users u
                JOIN user_subscriptions s ON s.user_id = u.id
                WHERE u.id = $1
                  AND u.disabled = FALSE
                  AND s.active = TRUE
                  AND s.expires_at > now()
            )
            "#,
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if !user_active {
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }

        let visible_lines = sqlx::query_as::<_, AssignedAccessLineExitPoolRow>(
            r#"
            SELECT l.id AS access_line_id, l.access_node_id, l.exit_endpoint_id, l.exit_pool_id
            FROM user_access_line_assignments a
            JOIN access_lines l ON l.id = a.access_line_id
            WHERE a.user_id = $1 AND l.enabled = TRUE
            ORDER BY a.assigned_at ASC, l.id ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        if visible_lines.is_empty() {
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }

        // 出口分配必须跟随用户入口授权，避免入口已不可用但仍保留旧出口归属。
        sqlx::query(
            r#"
            DELETE FROM user_exit_assignments uea
            WHERE uea.user_id = $1
              AND NOT EXISTS (
                SELECT 1
                FROM user_access_line_assignments a
                JOIN access_lines l ON l.id = a.access_line_id
                WHERE a.user_id = uea.user_id
                  AND a.access_line_id = uea.access_line_id
                  AND l.exit_pool_id = uea.exit_pool_id
                  AND l.enabled = TRUE
              )
            "#,
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        for line in visible_lines {
            let existing = sqlx::query_as::<_, ExistingExitAssignmentRow>(
                r#"
                SELECT exit_endpoint_id
                FROM user_exit_assignments
                WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
                "#,
            )
            .bind(user_id)
            .bind(line.access_line_id)
            .bind(line.exit_pool_id)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(existing) = &existing {
                let _still_healthy = sqlx::query_scalar::<_, bool>(
                    r#"
                    SELECT EXISTS (
                        SELECT 1
                        FROM exit_pool_members m
                        JOIN exit_pools p ON p.id = m.exit_pool_id
                        JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
                        JOIN exit_resources r ON r.id = e.exit_resource_id
                        LEFT JOIN access_exit_probe_states s
                          ON s.access_node_id = $3
                         AND s.exit_endpoint_id = e.id
                      WHERE m.exit_pool_id = $1
                        AND m.exit_endpoint_id = $2
                          AND p.enabled = TRUE
                          AND m.status IN ('healthy', 'degraded', 'draining')
                          AND (
                              $4 = FALSE
                              OR COALESCE(s.effective_status, 'healthy') <> 'offline'
                          )
                          AND e.enabled = TRUE
                          AND r.enabled = TRUE
                          AND (
                              (
                                  e.outbound_type = 'direct'
                                  AND r.ownership = 'local_direct'
                                  AND r.access_node_id = $3
                              )
                              OR (
                                  e.outbound_type IN ('socks', 'http')
                                  AND trim(e.host) <> ''
                                  AND e.port > 0
                                  AND (
                                      e.outbound_config = '{}'::jsonb
                                      OR (
                                          trim(COALESCE(e.outbound_config->>'username', e.outbound_config->>'user', '')) <> ''
                                          AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'pass', '')) <> ''
                                      )
                                  )
                              )
                              OR (
                                  e.outbound_type = 'vless'
                                  AND trim(e.host) <> ''
                                  AND e.port > 0
                                  AND trim(COALESCE(e.outbound_config->>'uuid', e.outbound_config->>'id', '')) <> ''
                                  AND (
	                                      lower(trim(COALESCE(e.outbound_config->>'security', 'tls'))) IN ('', 'tls', 'none')
                                      OR (
                                          lower(trim(COALESCE(e.outbound_config->>'security', ''))) = 'reality'
                                          AND trim(COALESCE(e.outbound_config->>'server_name', e.outbound_config->>'servername', e.outbound_config->>'sni', '')) <> ''
                                          AND trim(COALESCE(e.outbound_config->>'public_key', e.outbound_config->>'publicKey', e.outbound_config->>'reality_public_key', '')) <> ''
                                      )
                                  )
                              )
                              OR (
                                  e.outbound_type = 'trojan'
                                  AND trim(e.host) <> ''
                                  AND e.port > 0
                                  AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
                              )
                              OR (
                                  e.outbound_type = 'shadowsocks'
                                  AND trim(e.host) <> ''
                                  AND e.port > 0
                                  AND trim(COALESCE(e.outbound_config->>'method', e.outbound_config->>'cipher', '')) <> ''
                                  AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
                              )
                              OR (
                                  e.outbound_type = 'hysteria'
                                  AND trim(e.host) <> ''
                                  AND e.port > 0
                                  AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'auth', '')) <> ''
                              )
                          )
                    )
                    "#,
                )
            .bind(line.exit_pool_id)
            .bind(existing.exit_endpoint_id)
            .bind(line.access_node_id)
                .bind(block_unhealthy_lines)
                .fetch_one(&mut *tx)
                .await?;

                // 当前扁平线路模型每次同步都重新按固定顺序选择。
            }

            let pool_strategy = sqlx::query_scalar::<_, String>(
                "SELECT strategy FROM exit_pools WHERE id = $1 AND enabled = TRUE",
            )
            .bind(line.exit_pool_id)
            .fetch_optional(&mut *tx)
            .await?;

            let candidates = sqlx::query_as::<_, EligibleExitEndpointRow>(
                r#"
                SELECT e.id AS exit_endpoint_id, m.weight, m.priority
                FROM exit_pool_members m
                JOIN exit_pools p ON p.id = m.exit_pool_id
                JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
                JOIN exit_resources r ON r.id = e.exit_resource_id
                LEFT JOIN access_exit_probe_states s
                  ON s.access_node_id = $2
                 AND s.exit_endpoint_id = e.id
                        WHERE m.exit_pool_id = $1
                          AND ($4::uuid IS NULL OR m.exit_endpoint_id = $4)
                          AND p.enabled = TRUE
                  AND m.status = 'healthy'
                  AND m.allow_new_assignments = TRUE
                  AND (
                      $3 = FALSE
                      OR COALESCE(s.effective_status, 'healthy') <> 'offline'
                  )
                  AND e.enabled = TRUE
                  AND r.enabled = TRUE
                  AND (
                      (
                          e.outbound_type = 'direct'
                          AND r.ownership = 'local_direct'
                          AND r.access_node_id = $2
                      )
                      OR (
                          e.outbound_type IN ('socks', 'http')
                          AND trim(e.host) <> ''
                          AND e.port > 0
                          AND (
                              e.outbound_config = '{}'::jsonb
                              OR (
                                  trim(COALESCE(e.outbound_config->>'username', e.outbound_config->>'user', '')) <> ''
                                  AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'pass', '')) <> ''
                              )
                          )
                      )
                      OR (
                          e.outbound_type = 'vless'
                          AND trim(e.host) <> ''
                          AND e.port > 0
                          AND trim(COALESCE(e.outbound_config->>'uuid', e.outbound_config->>'id', '')) <> ''
                          AND (
	                              lower(trim(COALESCE(e.outbound_config->>'security', 'tls'))) IN ('', 'tls', 'none')
                              OR (
                                  lower(trim(COALESCE(e.outbound_config->>'security', ''))) = 'reality'
                                  AND trim(COALESCE(e.outbound_config->>'server_name', e.outbound_config->>'servername', e.outbound_config->>'sni', '')) <> ''
                                  AND trim(COALESCE(e.outbound_config->>'public_key', e.outbound_config->>'publicKey', e.outbound_config->>'reality_public_key', '')) <> ''
                              )
                          )
                      )
                      OR (
                          e.outbound_type = 'trojan'
                          AND trim(e.host) <> ''
                          AND e.port > 0
                          AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
                      )
                      OR (
                          e.outbound_type = 'shadowsocks'
                          AND trim(e.host) <> ''
                          AND e.port > 0
                          AND trim(COALESCE(e.outbound_config->>'method', e.outbound_config->>'cipher', '')) <> ''
                          AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
                      )
                      OR (
                          e.outbound_type = 'hysteria'
                          AND trim(e.host) <> ''
                          AND e.port > 0
                          AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'auth', '')) <> ''
                      )
                  )
                ORDER BY m.priority DESC, m.weight DESC, e.id ASC
                "#,
            )
            .bind(line.exit_pool_id)
            .bind(line.access_node_id)
            .bind(block_unhealthy_lines)
            .bind(line.exit_endpoint_id)
            .fetch_all(&mut *tx)
            .await?;

            let Some(chosen) = choose_exit_endpoint_for_strategy(
                user_id,
                line.access_line_id,
                pool_strategy.as_deref().unwrap_or("priority"),
                &candidates,
            ) else {
                sqlx::query(
                    r#"
                    DELETE FROM user_exit_assignments
                    WHERE user_id = $1 AND access_line_id = $2 AND exit_pool_id = $3
                    "#,
                )
                .bind(user_id)
                .bind(line.access_line_id)
                .bind(line.exit_pool_id)
                .execute(&mut *tx)
                .await?;
                continue;
            };

            let reason = if existing.is_some() {
                "failover:unavailable"
            } else {
                "initial"
            };
            if existing
                .as_ref()
                .is_some_and(|existing| existing.exit_endpoint_id == chosen)
            {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO user_exit_assignments (
                    user_id, access_line_id, exit_pool_id, exit_endpoint_id,
                    failover_reason
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
                    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                    assigned_at = now(),
                    failover_reason = EXCLUDED.failover_reason
                "#,
            )
            .bind(user_id)
            .bind(line.access_line_id)
            .bind(line.exit_pool_id)
            .bind(chosen)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn sync_assignments_for_active_users(&self) -> Result<(), DbError> {
        let user_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT u.id
            FROM users u
            JOIN user_subscriptions s ON s.user_id = u.id
            WHERE u.disabled = FALSE
              AND s.active = TRUE
              AND s.expires_at > now()
            ORDER BY u.created_at ASC, u.id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        for user_id in user_ids {
            self.sync_user_access_line_assignments(user_id).await?;
            self.sync_user_exit_assignments(user_id).await?;
        }

        Ok(())
    }
}
