//! 用户入口授权同步。
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
use uuid::Uuid;

impl PgStore {
    pub(crate) async fn load_store_data_for_user_assignments(
        &self,
        user_id: Uuid,
    ) -> Result<StoreData, DbError> {
        let mut data = self.load_store_data().await?;
        let assignments = self.user_access_line_assignments(user_id).await?;
        data.user_access_line_assignments_authoritative = true;

        for (line_group_id, access_line_id) in assignments {
            data.user_access_line_assignments
                .push(xrayc_core::UserAccessLineAssignment {
                    user_id,
                    line_group_id,
                    access_line_id,
                });
        }

        Ok(data)
    }

    pub(crate) async fn user_access_line_assignments(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid)>, DbError> {
        let rows = sqlx::query_as::<_, UserAccessLineAssignmentRow>(
            r#"
            SELECT line_group_id, access_line_id
            FROM user_access_line_assignments
            WHERE user_id = $1
            ORDER BY line_group_id, assigned_at, access_line_id
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.line_group_id, row.access_line_id))
            .collect())
    }

    pub(crate) async fn sync_user_access_line_assignments(
        &self,
        user_id: Uuid,
    ) -> Result<(), DbError> {
        let subscription = sqlx::query_as::<_, AssignmentSubscriptionRow>(
            r#"
            SELECT s.plan_id,
                   CASE WHEN s.limit_bytes = -1 THEN 9223372036854775807 ELSE s.limit_bytes - s.used_bytes END AS remaining_bytes
            FROM user_subscriptions s
            JOIN users u ON u.id = s.user_id
            WHERE s.user_id = $1
              AND u.disabled = FALSE
              AND s.active = TRUE
              AND s.expires_at > now()
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(subscription) = subscription else {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
            return Ok(());
        };
        let block_unhealthy_lines = self.subscription_blocks_unhealthy_lines().await?;

        let policies = sqlx::query_as::<_, AssignmentPolicyRow>(
            r#"
            WITH ranked_groups AS (
                SELECT plg.line_group_id,
                       lg.sort_weight,
                       lg.name,
                       ROW_NUMBER() OVER (
                           PARTITION BY plg.line_group_id
                           ORDER BY lg.sort_weight, lg.name, plg.line_group_id
                       ) AS policy_rank
                FROM plan_line_groups plg
                JOIN plans p ON p.id = plg.plan_id
                JOIN line_groups lg ON lg.id = plg.line_group_id
                WHERE plg.plan_id = $1
                  AND p.enabled = TRUE
                  AND p.is_deleted = FALSE
                  AND lg.enabled = TRUE
            )
            SELECT line_group_id
            FROM ranked_groups
            WHERE policy_rank = 1
            ORDER BY sort_weight, name, line_group_id
            "#,
        )
        .bind(subscription.plan_id)
        .fetch_all(&self.pool)
        .await?;

        let policy_group_ids = policies
            .iter()
            .map(|policy| policy.line_group_id)
            .collect::<Vec<_>>();
        let mut tx = self.pool.begin().await?;
        if policy_group_ids.is_empty() {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        sqlx::query(
            r#"
            DELETE FROM user_access_line_assignments
            WHERE user_id = $1 AND NOT (line_group_id = ANY($2))
            "#,
        )
        .bind(user_id)
        .bind(&policy_group_ids)
        .execute(&mut *tx)
        .await?;

        let mut selected_assignments = Vec::<(Uuid, Uuid)>::new();
        for policy in policies {
            let eligible = sqlx::query_as::<_, EligibleAccessLineRow>(
                r#"
                SELECT l.id
                FROM access_lines l
                JOIN access_nodes n ON n.id = l.access_node_id
                JOIN exit_pools ep ON ep.id = l.exit_pool_id
                WHERE (
                    EXISTS (
                        SELECT 1
                        FROM line_group_binding_nodes lgbn
                        WHERE lgbn.line_group_id = $1
                          AND lgbn.entry_exit_binding_id = l.id
                    )
                    OR (
                        NOT EXISTS (
                            SELECT 1
                            FROM line_group_binding_nodes lgbn
                            WHERE lgbn.line_group_id = $1
                        )
                        AND l.line_group_id = $1
                    )
                    OR (
                        NOT EXISTS (
                            SELECT 1
                            FROM line_group_binding_nodes lgbn
                            WHERE lgbn.line_group_id = $1
                        )
                        AND l.exit_endpoint_id IS NOT NULL
                        AND EXISTS (
                            SELECT 1
                            FROM line_group_exit_endpoints lgee
                            WHERE lgee.line_group_id = $1
                              AND lgee.exit_endpoint_id = l.exit_endpoint_id
                        )
                    )
                )
                  AND l.enabled = TRUE
                  AND lower(COALESCE(NULLIF(trim(n.status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
                  AND ep.enabled = TRUE
                  AND $2 > 0
                  AND EXISTS (
                      SELECT 1
                      FROM exit_pool_members m
                      JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
                      JOIN exit_resources r ON r.id = e.exit_resource_id
                      LEFT JOIN access_exit_probe_states s
                        ON s.access_node_id = l.access_node_id
                       AND s.exit_endpoint_id = e.id
                      WHERE m.exit_pool_id = l.exit_pool_id
                        AND (
                            l.exit_endpoint_id IS NULL
                            OR m.exit_endpoint_id = l.exit_endpoint_id
                        )
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
                                AND r.access_node_id = l.access_node_id
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
                ORDER BY l.visibility_weight DESC, l.name ASC, l.id ASC
                "#,
            )
            .bind(policy.line_group_id)
            .bind(subscription.remaining_bytes.max(0))
            .bind(block_unhealthy_lines)
            .fetch_all(&mut *tx)
            .await?;

            for line in eligible {
                selected_assignments.push((policy.line_group_id, line.id));
            }
        }

        if selected_assignments.is_empty() {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("DELETE FROM user_access_line_assignments WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            for (line_group_id, access_line_id) in &selected_assignments {
                sqlx::query(
                    r#"
                INSERT INTO user_access_line_assignments (
                    user_id, line_group_id, access_line_id
                )
                VALUES ($1, $2, $3)
                ON CONFLICT (user_id, line_group_id, access_line_id) DO NOTHING
                "#,
                )
                .bind(user_id)
                .bind(line_group_id)
                .bind(access_line_id)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query(
                r#"
                DELETE FROM user_exit_assignments uea
                WHERE uea.user_id = $1
                  AND NOT EXISTS (
                      SELECT 1
                      FROM user_access_line_assignments ula
                      WHERE ula.user_id = uea.user_id
                        AND ula.access_line_id = uea.access_line_id
                  )
                "#,
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
