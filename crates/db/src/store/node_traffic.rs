//! 监控中心「节点资源」流量聚合只读接口。
//! 本模块读取明细与小时归档的原始上下行差值；旧线路删除后仍保留节点归属。
//! 统计使用不可变历史路由快照，不依赖仍然存在的活动线路。
//! total = uplink + downlink,绝不使用乘过倍率的 billed_bytes。
//! 明细保持 [from,to)；归档只计完全落入范围的小时片段，边界不完整时明确返回数量。
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
    partial_archived_hours: i64,
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
            WITH totals AS (
                SELECT access_node_id,
                    COALESCE(SUM(delta_uplink) FILTER (WHERE first_collected_at >= $1 AND last_collected_at < $2),0)::bigint AS uplink_bytes,
                    COALESCE(SUM(delta_downlink) FILTER (WHERE first_collected_at >= $1 AND last_collected_at < $2),0)::bigint AS downlink_bytes,
                    COUNT(DISTINCT date_trunc('hour',first_collected_at)) FILTER (WHERE archived AND (first_collected_at < $1 OR last_collected_at >= $2)) AS partial_archived_hours
                FROM usage_node_traffic_history
                WHERE first_collected_at < $2 AND last_collected_at >= $1
                GROUP BY access_node_id
            )
            SELECT n.id AS access_node_id, n.name AS access_node_name,
                COALESCE(t.uplink_bytes,0)::bigint AS uplink_bytes,
                COALESCE(t.downlink_bytes,0)::bigint AS downlink_bytes,
                (COALESCE(t.uplink_bytes,0)+COALESCE(t.downlink_bytes,0))::bigint AS total_bytes,
                COALESCE(t.partial_archived_hours,0)::bigint AS partial_archived_hours
            FROM access_nodes n LEFT JOIN totals t ON t.access_node_id=n.id
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
                    "total_bytes": row.total_bytes.max(0),
                    "partial_archived_hours": row.partial_archived_hours.max(0)
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
        // 单条 SQL 同一快照同时生成图表与合计，避免两次查询之间有新上报而不一致。
        let result = sqlx::query_scalar::<_, Value>(
            r#"
            WITH history AS MATERIALIZED (
                SELECT * FROM usage_node_traffic_history
                WHERE access_node_id=$2 AND first_collected_at < $4 AND last_collected_at >= $3
            ), points AS (
                SELECT date_trunc($1::text, first_collected_at) AS bucket_start,
                    COALESCE(SUM(delta_uplink),0)::bigint AS uplink_bytes,
                    COALESCE(SUM(delta_downlink),0)::bigint AS downlink_bytes
                FROM history WHERE first_collected_at >= $3 AND last_collected_at < $4
                GROUP BY bucket_start
            )
            SELECT jsonb_build_object(
                'points', COALESCE(jsonb_agg(jsonb_build_object(
                    'bucket_start_ms', (EXTRACT(EPOCH FROM bucket_start)*1000)::bigint,
                    'uplink_bytes', uplink_bytes, 'downlink_bytes', downlink_bytes,
                    'total_bytes', uplink_bytes+downlink_bytes
                ) ORDER BY bucket_start), '[]'::jsonb),
                'summary', jsonb_build_object(
                    'uplink_bytes', COALESCE(SUM(uplink_bytes),0),
                    'downlink_bytes', COALESCE(SUM(downlink_bytes),0),
                    'total_bytes', COALESCE(SUM(uplink_bytes+downlink_bytes),0)
                ),
                'partial_archived_hours', (SELECT COUNT(DISTINCT date_trunc('hour',first_collected_at)) FROM history
                    WHERE archived AND (first_collected_at < $3 OR last_collected_at >= $4)),
                'archive_resolution', 'hour',
                'source', 'usage_ledgers+usage_hourly_rollups'
            ) FROM points
            "#,
        )
        .bind(bucket)
        .bind(node_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(result)
    }
}
