//! Agent 流量上报和账本写入。
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
    pub async fn apply_report(
        &self,
        report: TrafficReport,
    ) -> Result<TrafficReportResult, DbError> {
        self.apply_report_for_node(None, report).await
    }

    pub async fn apply_report_for_node(
        &self,
        access_node_id: Option<Uuid>,
        report: TrafficReport,
    ) -> Result<TrafficReportResult, DbError> {
        let mut tx = self.pool.begin().await?;

        let line = sqlx::query_as::<_, BillingLineRow>(
            r#"
            SELECT access_node_id, exit_pool_id, enabled
            FROM access_lines
            WHERE id = $1
            "#,
        )
        .bind(report.access_line_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::AccessLineNotFound)?;
        if access_node_id.is_some_and(|node_id| node_id != line.access_node_id) {
            return Err(DbError::AccessLineNodeMismatch);
        }
        touch_access_node_traffic_in_tx(&mut tx, line.access_node_id).await?;

        let user = sqlx::query_as::<_, BillingUserRow>(
            r#"SELECT id, xray_user_key, disabled FROM users WHERE xray_user_key = $1"#,
        )
        .bind(&report.xray_user_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::UserNotFound)?;
        let line_authorized = if user.disabled || !line.enabled {
            false
        } else {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
	                    SELECT 1
	                    FROM user_access_line_assignments a
	                    JOIN user_subscriptions s ON s.user_id = a.user_id
	                    JOIN access_lines al ON al.id = a.access_line_id
	                    WHERE a.user_id = $1
	                      AND a.access_line_id = $2
	                      AND s.active = TRUE
	                      AND s.expires_at > now()
	                      AND EXISTS (
	                          SELECT 1
	                          FROM plan_line_groups plg
	                          JOIN line_groups lg ON lg.id = plg.line_group_id
	                          WHERE plg.plan_id = s.plan_id
	                            AND plg.line_group_id = a.line_group_id
	                            AND lg.enabled = TRUE
	                      )
		                      AND (
		                          (
		                              EXISTS (
		                                  SELECT 1
		                                  FROM line_group_binding_nodes lgbn
		                                  WHERE lgbn.line_group_id = a.line_group_id
		                                    AND lgbn.entry_exit_binding_id = a.access_line_id
		                              )
		                          )
		                          OR (
		                              NOT EXISTS (
		                                  SELECT 1
		                                  FROM line_group_binding_nodes lgbn
		                                  WHERE lgbn.line_group_id = a.line_group_id
		                              )
		                              AND (
		                                  (
		                                      al.exit_endpoint_id IS NOT NULL
		                                      AND EXISTS (
		                                          SELECT 1
		                                          FROM line_group_exit_endpoints lgee
		                                          WHERE lgee.line_group_id = a.line_group_id
		                                            AND lgee.exit_endpoint_id = al.exit_endpoint_id
		                                      )
		                                  )
		                                  OR al.line_group_id = a.line_group_id
		                              )
		                          )
		                      )
	                )
                "#,
            )
            .bind(user.id)
            .bind(report.access_line_id)
            .fetch_one(&mut *tx)
            .await?
        };

        let previous = sqlx::query_as::<_, SnapshotRow>(
            r#"
            SELECT access_line_id, xray_user_key, uplink_total, downlink_total, collected_at
            FROM access_traffic_snapshots
            WHERE access_line_id = $1 AND xray_user_key = $2
            FOR UPDATE
            "#,
        )
        .bind(report.access_line_id)
        .bind(&report.xray_user_key)
        .fetch_optional(&mut *tx)
        .await?;

        if previous.is_none() {
            upsert_snapshot(&mut tx, line.access_node_id, &report).await?;
            if !line_authorized {
                mark_access_node_dirty_in_tx(
                    &mut tx,
                    line.access_node_id,
                    traffic_refresh_reason(line.enabled, user.disabled),
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: true,
                baseline_only: true,
                delta_uplink: 0,
                delta_downlink: 0,
                billed_bytes: 0,
                config_refresh_required: !line_authorized,
            });
        }

        let previous = previous.expect("checked above");
        let previous_uplink = to_u64(previous.uplink_total);
        let previous_downlink = to_u64(previous.downlink_total);

        if report.collected_at <= previous.collected_at {
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: false,
                baseline_only: false,
                delta_uplink: 0,
                delta_downlink: 0,
                billed_bytes: 0,
                config_refresh_required: false,
            });
        }

        if report.uplink_total < previous_uplink || report.downlink_total < previous_downlink {
            upsert_snapshot(&mut tx, line.access_node_id, &report).await?;
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: true,
                baseline_only: true,
                delta_uplink: 0,
                delta_downlink: 0,
                billed_bytes: 0,
                config_refresh_required: false,
            });
        }

        upsert_snapshot(&mut tx, line.access_node_id, &report).await?;

        let delta_uplink = report.uplink_total - previous_uplink;
        let delta_downlink = report.downlink_total - previous_downlink;
        let delta_total = delta_uplink.saturating_add(delta_downlink);
        if delta_total == 0 || !line_authorized {
            if !line_authorized {
                mark_access_node_dirty_in_tx(
                    &mut tx,
                    line.access_node_id,
                    traffic_refresh_reason(line.enabled, user.disabled),
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: true,
                baseline_only: false,
                delta_uplink,
                delta_downlink,
                billed_bytes: 0,
                config_refresh_required: !line_authorized,
            });
        }

        let subscription = sqlx::query_as::<_, BillingSubscriptionRow>(
            r#"
            WITH locked_subscription AS (
                SELECT s.user_id, s.plan_id, s.used_bytes, s.limit_bytes
                FROM user_subscriptions s
                WHERE s.user_id = $1 AND s.active = TRUE
                  AND s.expires_at > now()
                FOR UPDATE
            ),
            line_group_multiplier AS (
                SELECT GREATEST(
                    COALESCE(MAX(plg.billing_multiplier::float8), 1.0),
                    1.0
                ) AS billing_multiplier
                FROM locked_subscription s
                JOIN plan_line_groups plg ON plg.plan_id = s.plan_id
                JOIN access_lines al ON al.id = $2
	                  AND (
	                    (
	                      EXISTS (
	                        SELECT 1
	                        FROM line_group_binding_nodes lgbn
	                        WHERE lgbn.line_group_id = plg.line_group_id
	                          AND lgbn.entry_exit_binding_id = al.id
	                      )
	                    )
	                    OR (
	                      NOT EXISTS (
	                        SELECT 1
	                        FROM line_group_binding_nodes lgbn
	                        WHERE lgbn.line_group_id = plg.line_group_id
	                      )
	                      AND (
	                        (
	                          al.exit_endpoint_id IS NOT NULL
	                          AND EXISTS (
	                            SELECT 1
	                            FROM line_group_exit_endpoints lgee
	                            WHERE lgee.line_group_id = plg.line_group_id
	                              AND lgee.exit_endpoint_id = al.exit_endpoint_id
	                          )
	                        )
	                        OR al.line_group_id = plg.line_group_id
	                      )
	                    )
	                  )
                JOIN line_groups lg ON lg.id = plg.line_group_id
                  AND lg.enabled = TRUE
            )
            SELECT s.used_bytes, s.limit_bytes,
                   GREATEST(
                       p.billing_multiplier::float8,
                       line_group_multiplier.billing_multiplier
                   ) AS billing_multiplier
            FROM locked_subscription s
            JOIN plans p ON p.id = s.plan_id
            CROSS JOIN line_group_multiplier
            "#,
        )
        .bind(user.id)
        .bind(report.access_line_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(subscription) = subscription else {
            mark_access_node_dirty_in_tx(
                &mut tx,
                line.access_node_id,
                "traffic_subscription_missing",
            )
            .await?;
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: true,
                baseline_only: false,
                delta_uplink,
                delta_downlink,
                billed_bytes: 0,
                config_refresh_required: true,
            });
        };

        let remaining = if subscription.limit_bytes == -1 {
            u64::MAX
        } else {
            to_u64(subscription.limit_bytes).saturating_sub(to_u64(subscription.used_bytes))
        };
        let multiplier = subscription.billing_multiplier;
        if remaining == 0 {
            mark_access_node_dirty_in_tx(&mut tx, line.access_node_id, "traffic_quota_exhausted")
                .await?;
            tx.commit().await?;
            return Ok(TrafficReportResult {
                accepted: true,
                baseline_only: false,
                delta_uplink,
                delta_downlink,
                billed_bytes: 0,
                config_refresh_required: true,
            });
        }
        let requested_bill = ((delta_total as f64) * multiplier).ceil() as u64;
        let billed_bytes = requested_bill.min(remaining);
        let billed_uplink = billed_bytes
            .saturating_mul(delta_uplink)
            .checked_div(delta_total)
            .unwrap_or(0);
        let billed_downlink = billed_bytes.saturating_sub(billed_uplink);

        if billed_bytes > 0 {
            let exit_endpoint_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT exit_endpoint_id
                FROM user_exit_assignments
                WHERE user_id = $1
                  AND access_line_id = $2
                  AND exit_pool_id = $3
                "#,
            )
            .bind(user.id)
            .bind(report.access_line_id)
            .bind(line.exit_pool_id)
            .fetch_optional(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                UPDATE user_subscriptions
                SET used_bytes = used_bytes + $2, updated_at = now()
                WHERE user_id = $1
                "#,
            )
            .bind(user.id)
            .bind(to_i64(billed_bytes))
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO usage_ledgers (
                    access_line_id, user_id, xray_user_key, traffic_source,
                    delta_uplink, delta_downlink, billing_multiplier,
                    billed_bytes, delta_total, billed_uplink, billed_downlink,
                    collected_at, recorded_at, exit_endpoint_id
                )
                VALUES (
                    $1, $2, $3, 'access_line', $4, $5, $6,
                    $7, $8, $9, $10, $11, $11, $12
                )
                "#,
            )
            .bind(report.access_line_id)
            .bind(user.id)
            .bind(&report.xray_user_key)
            .bind(to_i64(delta_uplink))
            .bind(to_i64(delta_downlink))
            .bind(multiplier)
            .bind(to_i64(billed_bytes))
            .bind(to_i64(delta_total))
            .bind(to_i64(billed_uplink))
            .bind(to_i64(billed_downlink))
            .bind(report.collected_at)
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
        }

        if billed_bytes == remaining && remaining > 0 {
            mark_access_node_dirty_in_tx(&mut tx, line.access_node_id, "traffic_quota_exhausted")
                .await?;
        }

        tx.commit().await?;
        Ok(TrafficReportResult {
            accepted: true,
            baseline_only: false,
            delta_uplink,
            delta_downlink,
            billed_bytes,
            config_refresh_required: billed_bytes == remaining && remaining > 0,
        })
    }
}

async fn touch_access_node_traffic_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE access_nodes
        SET last_traffic_report_at = now(),
            last_traffic_success_at = now()
        WHERE id = $1
        "#,
    )
    .bind(access_node_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
