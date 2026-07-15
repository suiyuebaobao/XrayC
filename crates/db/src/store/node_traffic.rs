//! 监控中心「节点资源」流量聚合只读接口。
//! 本模块只读 usage_ledgers 原始上下行差值(delta_uplink/delta_downlink)。
//! 通过 access_line_id 关联 access_lines,把线路流量折算到中转节点(access_node_id)。
//! total = uplink + downlink,绝不使用乘过倍率的 billed_bytes。
//! collected_at 落在半开区间 [from, to) 内才计入,to 为排他上界。
//! 汇总视图返回所有中转节点(无流量填 0),便于前端与 access_nodes 对齐。
//! 趋势视图按 date_trunc(bucket) 分桶(hour/day),points 升序,summary 为整段合计。
//! 仅做只读 SELECT 聚合,不写库、不改状态、不引 ORM。
//! 注释使用中文,方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束,单文件保持低于 550 行。
use crate::*;
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

// 单个中转节点在区间内的流量汇总行。
#[derive(Debug, FromRow)]
struct NodeTrafficSummaryRow {
    access_node_id: Uuid,
    access_node_name: String,
    uplink_bytes: i64,
    downlink_bytes: i64,
    total_bytes: i64,
}

// 单节点趋势分桶行,bucket_start 为 date_trunc 后的桶起点(UTC)。
#[derive(Debug, FromRow)]
struct NodeTrafficTrendPointRow {
    bucket_start: DateTime<Utc>,
    uplink_bytes: i64,
    downlink_bytes: i64,
    total_bytes: i64,
}

// 单节点在区间内的合计行,用于趋势响应的 summary。
#[derive(Debug, FromRow)]
struct NodeTrafficTotalsRow {
    uplink_bytes: i64,
    downlink_bytes: i64,
    total_bytes: i64,
}

impl PgStore {
    /// 节点流量汇总:按中转节点聚合区间内 usage_ledgers 的真实上下行。
    /// 返回所有中转节点(即便区间零流量也出现,数值为 0),便于前端与节点清单对齐。
    /// from/to 为半开区间 [from, to);仅统计 traffic_source='access_line' 的真实转发。
    pub async fn node_traffic_summary_json(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, NodeTrafficSummaryRow>(
            r#"
            SELECT n.id AS access_node_id,
                   n.name AS access_node_name,
                   COALESCE(agg.uplink_bytes, 0)::bigint AS uplink_bytes,
                   COALESCE(agg.downlink_bytes, 0)::bigint AS downlink_bytes,
                   (COALESCE(agg.uplink_bytes, 0) + COALESCE(agg.downlink_bytes, 0))::bigint AS total_bytes
            FROM access_nodes n
            LEFT JOIN (
                SELECT l.access_node_id AS access_node_id,
                       COALESCE(SUM(ul.delta_uplink), 0)::bigint AS uplink_bytes,
                       COALESCE(SUM(ul.delta_downlink), 0)::bigint AS downlink_bytes
                FROM usage_ledgers ul
                JOIN access_lines l ON l.id = ul.access_line_id
                WHERE ul.traffic_source = 'access_line'
                  AND ul.collected_at >= $1
                  AND ul.collected_at < $2
                GROUP BY l.access_node_id
            ) agg ON agg.access_node_id = n.id
            ORDER BY total_bytes DESC, n.name ASC, n.id ASC
            "#,
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "nodes": rows
                .into_iter()
                .map(|row| json!({
                    "access_node_id": row.access_node_id,
                    "access_node_name": row.access_node_name,
                    "uplink_bytes": row.uplink_bytes.max(0),
                    "downlink_bytes": row.downlink_bytes.max(0),
                    "total_bytes": row.total_bytes.max(0)
                }))
                .collect::<Vec<_>>()
        }))
    }

    /// 单节点流量趋势:按 date_trunc(bucket) 分桶给出每桶上下行,并附整段 summary。
    /// bucket 仅接受 'hour' 或 'day'(调用方已校验);points 按桶起点升序。
    /// from/to 为半开区间 [from, to);仅统计 traffic_source='access_line' 的真实转发。
    pub async fn node_traffic_trend_json(
        &self,
        node_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket: &str,
    ) -> Result<Value, DbError> {
        let points = sqlx::query_as::<_, NodeTrafficTrendPointRow>(
            r#"
            SELECT date_trunc($1::text, ul.collected_at) AS bucket_start,
                   COALESCE(SUM(ul.delta_uplink), 0)::bigint AS uplink_bytes,
                   COALESCE(SUM(ul.delta_downlink), 0)::bigint AS downlink_bytes,
                   (COALESCE(SUM(ul.delta_uplink), 0) + COALESCE(SUM(ul.delta_downlink), 0))::bigint AS total_bytes
            FROM usage_ledgers ul
            JOIN access_lines l ON l.id = ul.access_line_id
            WHERE l.access_node_id = $2
              AND ul.traffic_source = 'access_line'
              AND ul.collected_at >= $3
              AND ul.collected_at < $4
            GROUP BY bucket_start
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(bucket)
        .bind(node_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        let summary = sqlx::query_as::<_, NodeTrafficTotalsRow>(
            r#"
            SELECT COALESCE(SUM(ul.delta_uplink), 0)::bigint AS uplink_bytes,
                   COALESCE(SUM(ul.delta_downlink), 0)::bigint AS downlink_bytes,
                   (COALESCE(SUM(ul.delta_uplink), 0) + COALESCE(SUM(ul.delta_downlink), 0))::bigint AS total_bytes
            FROM usage_ledgers ul
            JOIN access_lines l ON l.id = ul.access_line_id
            WHERE l.access_node_id = $1
              AND ul.traffic_source = 'access_line'
              AND ul.collected_at >= $2
              AND ul.collected_at < $3
            "#,
        )
        .bind(node_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(json!({
            "points": points
                .into_iter()
                .map(|row| json!({
                    "bucket_start_ms": row.bucket_start.timestamp_millis(),
                    "uplink_bytes": row.uplink_bytes.max(0),
                    "downlink_bytes": row.downlink_bytes.max(0),
                    "total_bytes": row.total_bytes.max(0)
                }))
                .collect::<Vec<_>>(),
            "summary": {
                "uplink_bytes": summary.uplink_bytes.max(0),
                "downlink_bytes": summary.downlink_bytes.max(0),
                "total_bytes": summary.total_bytes.max(0)
            }
        }))
    }
}
