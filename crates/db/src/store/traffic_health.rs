//! 健康检查流量统计读模型。
//! 本文件负责生成 operations summary 的 traffic_health 字段。
//! 统计口径以真实 usage_ledgers 与清理后的 daily rollup 为准，不生成模拟流量。
//! line_items 按中转入口独立展示，避免中转节点聚合遮蔽单入口问题。
//! node_items 保留中转服务器聚合，便于判断服务器总负载。
//! exit_items 按真实出口 endpoint 展示，便于定位出口线路消耗。
//! 图表按本月每日聚合，峰值按本月小时聚合。
//! 所有字段只返回聚合数字和名称，不暴露出口凭据。
//! 新增 SQL 时保持只读查询，不修改运行态数据。
//! 本头部满足前十行中文注释约束。
use super::rows::*;
use crate::*;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

impl PgStore {
    pub(crate) async fn operations_traffic_health_json(&self) -> Result<Value, DbError> {
        let windows = self.traffic_health_windows().await?;
        let line_items = self.traffic_health_line_items().await?;
        let line_chart = self.traffic_health_line_chart().await?;
        let node_items = self.traffic_health_node_items().await?;
        let node_chart = self.traffic_health_node_chart().await?;
        let exit_items = self.traffic_health_exit_items().await?;
        let exit_chart = self.traffic_health_exit_chart().await?;
        let group_items = self.traffic_health_group_items().await?;
        let group_chart = self.traffic_health_group_chart().await?;

        Ok(json!({
            "generated_at": Utc::now(),
            "windows": traffic_windows_json(&windows),
            "line_items": traffic_entity_items_json(
                line_items,
                "access_line_id",
                group_traffic_chart_rows(line_chart),
            ),
            "node_items": traffic_entity_items_json(
                node_items,
                "access_node_id",
                group_traffic_chart_rows(node_chart),
            ),
            "exit_items": traffic_entity_items_json(
                exit_items,
                "exit_endpoint_id",
                group_traffic_chart_rows(exit_chart),
            ),
            "group_items": traffic_entity_items_json(
                group_items,
                "line_group_id",
                group_traffic_chart_rows(group_chart),
            )
        }))
    }

    async fn traffic_health_windows(&self) -> Result<TrafficHealthWindowRow, DbError> {
        Ok(sqlx::query_as::<_, TrafficHealthWindowRow>(
            r#"
            WITH traffic AS (
                SELECT collected_at AS observed_at,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                UNION ALL
                SELECT (rollup_date::timestamp AT TIME ZONE 'UTC') AS observed_at,
                       delta_total,
                       billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
            )
            SELECT
                COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_real_bytes,
                COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_billed_bytes,
                COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_real_bytes,
                COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_billed_bytes,
                COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_real_bytes,
                COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_billed_bytes,
                COALESCE(SUM(delta_total), 0)::bigint AS total_real_bytes,
                COALESCE(SUM(billed_bytes), 0)::bigint AS total_billed_bytes
            FROM traffic
            "#,
        )
        .fetch_one(&self.pool)
        .await?)
    }
}

fn traffic_windows_json(row: &TrafficHealthWindowRow) -> Value {
    json!({
        "today": traffic_pair_json(row.today_real_bytes, row.today_billed_bytes),
        "week": traffic_pair_json(row.week_real_bytes, row.week_billed_bytes),
        "month": traffic_pair_json(row.month_real_bytes, row.month_billed_bytes),
        "total": traffic_pair_json(row.total_real_bytes, row.total_billed_bytes)
    })
}

fn traffic_pair_json(real_bytes: i64, billed_bytes: i64) -> Value {
    json!({
        "real_bytes": real_bytes.max(0),
        "billed_bytes": billed_bytes.max(0)
    })
}

fn group_traffic_chart_rows(
    rows: Vec<TrafficHealthChartRow>,
) -> HashMap<Uuid, Vec<TrafficHealthChartRow>> {
    let mut grouped = HashMap::<Uuid, Vec<TrafficHealthChartRow>>::new();
    for row in rows {
        grouped.entry(row.entity_id).or_default().push(row);
    }
    grouped
}

fn traffic_entity_items_json(
    rows: Vec<TrafficHealthEntityRow>,
    id_field: &str,
    grouped_chart: HashMap<Uuid, Vec<TrafficHealthChartRow>>,
) -> Vec<Value> {
    rows.into_iter()
        .map(|row| {
            let chart = grouped_chart
                .get(&row.entity_id)
                .map(|points| {
                    points
                        .iter()
                        .map(|point| {
                            json!({
                                "window_start": point.window_start,
                                "real_bytes": point.real_bytes.max(0),
                                "billed_bytes": point.billed_bytes.max(0)
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            json!({
                id_field: row.entity_id,
                "name": row.entity_name,
                "today_real_bytes": row.today_real_bytes.max(0),
                "week_real_bytes": row.week_real_bytes.max(0),
                "month_real_bytes": row.month_real_bytes.max(0),
                "month_billed_bytes": row.month_billed_bytes.max(0),
                "total_real_bytes": row.total_real_bytes.max(0),
                "total_billed_bytes": row.total_billed_bytes.max(0),
                "peak_hour_real_bytes": row.peak_hour_real_bytes.max(0),
                "low_hour_real_bytes": row.low_hour_real_bytes.max(0),
                "average_daily_real_bytes": row.average_daily_real_bytes.max(0),
                "chart": chart
            })
        })
        .collect()
}
