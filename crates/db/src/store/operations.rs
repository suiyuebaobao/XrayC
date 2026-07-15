//! 运行态会话、指标和运营榜单接口。
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
    pub async fn access_line_sessions_json(&self, access_line_id: Uuid) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, AccessLineSessionRow>(
            r#"
            SELECT s.id, s.access_line_id, s.xray_user_key, s.client_ip_hash,
                   GREATEST(s.active_connection_count, 0)::integer AS active_connection_count,
                   CASE
                     WHEN s.status = 'offline' THEN 'offline'
                     WHEN s.last_seen_at < now() - interval '5 minutes' THEN 'expired'
                     WHEN s.status IN ('online', 'unknown') THEN s.status
                     ELSE 'unknown'
                   END AS status,
                   s.started_at, s.last_seen_at, u.id AS user_id, u.email
            FROM access_user_sessions s
            LEFT JOIN users u ON u.xray_user_key = s.xray_user_key
            WHERE s.access_line_id = $1
            ORDER BY s.last_seen_at DESC
            LIMIT 200
            "#,
        )
        .bind(access_line_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "access_line_id": access_line_id,
            "sessions": rows.into_iter().map(|row| json!({
                "id": row.id,
                "access_line_id": row.access_line_id,
                "user_id": row.user_id,
                "email": row.email.unwrap_or_default(),
                "xray_user_key": row.xray_user_key,
                "client_ip_hash": row.client_ip_hash,
                "active_connection_count": row.active_connection_count,
                "active_connections": row.active_connection_count,
                "status": row.status,
                "started_at": row.started_at,
                "last_seen_at": row.last_seen_at
            })).collect::<Vec<_>>()
        }))
    }

    pub async fn access_line_metrics_json(&self, access_line_id: Uuid) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, LineMetricLatestRow>(
            r#"
            SELECT m.access_line_id,
                   COALESCE(m.access_node_id, l.access_node_id) AS access_node_id,
                   COALESCE(m.exit_pool_id, l.exit_pool_id) AS exit_pool_id,
                   m.online_users, m.active_connections, m.unique_client_ips,
                   m.uplink_rate_bps, m.downlink_rate_bps, m.collected_at
            FROM access_line_metric_snapshots m
            JOIN access_lines l ON l.id = m.access_line_id
            WHERE m.access_line_id = $1
            ORDER BY m.collected_at DESC
            LIMIT 200
            "#,
        )
        .bind(access_line_id)
        .fetch_all(&self.pool)
        .await?;
        let totals = sqlx::query_as::<_, LineLedgerTotalsRow>(
            r#"
            SELECT COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                   COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                   COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                   COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                   COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                   COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
            FROM (
                SELECT 1::bigint AS ledger_count,
                       delta_uplink,
                       delta_downlink,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_uplink,
                       billed_downlink,
                       billed_bytes
                FROM usage_ledgers
                WHERE access_line_id = $1
                  AND traffic_source = 'access_line'
                UNION ALL
                SELECT ledger_count,
                       delta_uplink,
                       delta_downlink,
                       delta_total,
                       billed_uplink,
                       billed_downlink,
                       billed_bytes
                FROM usage_daily_rollups
                WHERE access_line_id = $1
                  AND traffic_source = 'access_line'
            ) traffic
            "#,
        )
        .bind(access_line_id)
        .fetch_one(&self.pool)
        .await?;
        let windows = sqlx::query_as::<_, LineLedgerWindowRow>(
            r#"
            SELECT window_start,
                   COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                   COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                   COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                   COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                   COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                   COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
            FROM (
                SELECT date_trunc('hour', collected_at) AS window_start,
                       COUNT(*)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
                FROM usage_ledgers
                WHERE access_line_id = $1
                  AND traffic_source = 'access_line'
                  AND collected_at >= now() - interval '168 hours'
                GROUP BY window_start
                UNION ALL
                SELECT hour_start AS window_start,
                       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
                FROM usage_hourly_rollups
                WHERE access_line_id = $1
                  AND traffic_source = 'access_line'
                  AND hour_start >= now() - interval '168 hours'
                GROUP BY window_start
            ) traffic
            GROUP BY window_start
            ORDER BY window_start DESC
            LIMIT 336
            "#,
        )
        .bind(access_line_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "access_line_id": access_line_id,
            "ledger_summary": ledger_totals_json(&totals),
            "traffic_summary": ledger_totals_json(&totals),
            "ledger_windows": windows.into_iter().map(|row| json!({
                "window_start": row.window_start,
                "ledger_count": row.ledger_count.max(0),
                "delta_uplink": row.delta_uplink.max(0),
                "delta_downlink": row.delta_downlink.max(0),
                "delta_total": row.delta_total.max(0),
                "real_bytes": row.delta_total.max(0),
                "billed_uplink": row.billed_uplink.max(0),
                "billed_downlink": row.billed_downlink.max(0),
                "billed_bytes": row.billed_bytes.max(0)
            })).collect::<Vec<_>>(),
            "metrics": rows.into_iter().map(|row| json!({
                "access_line_id": row.access_line_id,
                "access_node_id": row.access_node_id,
                "exit_pool_id": row.exit_pool_id,
                "online_users": row.online_users,
                "active_user_count": row.online_users,
                "active_connections": row.active_connections,
                "active_connection_count": row.active_connections,
                "unique_client_ips": row.unique_client_ips,
                "unique_client_ip_count": row.unique_client_ips,
                "uplink_rate_bps": row.uplink_rate_bps,
                "downlink_rate_bps": row.downlink_rate_bps,
                "observed_at": row.collected_at,
                "collected_at": row.collected_at
            })).collect::<Vec<_>>()
        }))
    }

    pub async fn operations_ledger_ranking_json(&self, limit: i64) -> Result<Value, DbError> {
        let limit = limit.clamp(1, 100);
        let rows = sqlx::query_as::<_, OperationsLedgerRankingRow>(
            r#"
            WITH retained AS (
                SELECT access_line_id,
                       COUNT(*)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(collected_at) AS latest_collected_at
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                GROUP BY access_line_id
                UNION ALL
                SELECT access_line_id,
                       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(last_collected_at) AS latest_collected_at
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                GROUP BY access_line_id
            ),
            r AS (
                SELECT access_line_id,
                       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(latest_collected_at) AS latest_collected_at
                FROM retained
                GROUP BY access_line_id
            )
            SELECT r.access_line_id,
                   COALESCE(l.name, r.access_line_id::text) AS access_line_name,
                   COALESCE(n.name, '') AS access_node_name,
                   COALESCE(l.region_code, '') AS region_code,
                   COALESCE(l.listen_host, '') AS listen_host,
                   COALESCE(l.listen_port, 0)::integer AS listen_port,
                   r.ledger_count,
                   r.delta_uplink,
                   r.delta_downlink,
                   r.delta_total,
                   r.billed_uplink,
                   r.billed_downlink,
                   r.billed_bytes,
                   r.latest_collected_at
            FROM r
            LEFT JOIN access_lines l ON l.id = r.access_line_id
            LEFT JOIN access_nodes n ON n.id = l.access_node_id
            ORDER BY r.billed_bytes DESC,
                     r.delta_total DESC,
                     r.ledger_count DESC,
                     access_line_name ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let totals = sqlx::query_as::<_, OperationsLedgerRankingTotalsRow>(
            r#"
            WITH retained AS (
                SELECT access_line_id,
                       COUNT(*)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(collected_at) AS latest_collected_at
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                GROUP BY access_line_id
                UNION ALL
                SELECT access_line_id,
                       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(last_collected_at) AS latest_collected_at
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                GROUP BY access_line_id
            ),
            r AS (
                SELECT access_line_id,
                       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                       MAX(latest_collected_at) AS latest_collected_at
                FROM retained
                GROUP BY access_line_id
            )
            SELECT COUNT(*)::bigint AS access_line_count,
                   COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
                   COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
                   COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
                   COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
                   COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
                   COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
                   MAX(latest_collected_at) AS latest_collected_at
            FROM r
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(json!({
            "source": "usage_ledgers+usage_daily_rollups",
            "limit": limit,
            "generated_at": Utc::now(),
            "totals": operations_ledger_ranking_totals_json(&totals),
            "items": rows
                .iter()
                .enumerate()
                .map(|(index, row)| operations_ledger_ranking_row_json(index + 1, row))
                .collect::<Vec<_>>()
        }))
    }

    pub async fn clear_access_line_runtime(
        &self,
        access_node_id: Uuid,
        access_line_id: Uuid,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM access_line_metric_snapshots WHERE access_line_id = $1")
            .bind(access_line_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM access_user_sessions WHERE access_node_id = $1 AND access_line_id = $2",
        )
        .bind(access_node_id)
        .bind(access_line_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
