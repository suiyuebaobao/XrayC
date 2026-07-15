//! 健康检查流量统计图表查询。
//! 本文件从 traffic_health 拆出 line/node/exit/group 日图表。
//! SQL 原样保留本月 usage_ledgers 与 daily rollup 合并口径。
//! 入口模块仍负责按实体 ID 聚合为 JSON 数组。
//! 本模块只返回聚合点，不暴露出口凭据。
//! 分组图表共用 traffic_health_sql 中的 CTE。
//! 新增图表窗口时应保持 UTC rollup 口径一致。
//! 查询失败仍通过 sqlx 错误向上返回。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 本头部满足前十行中文注释约束。

use super::rows::*;
use super::traffic_health_sql::GROUP_LINES_SQL;
use crate::*;

impl PgStore {
    pub(crate) async fn traffic_health_line_chart(
        &self,
    ) -> Result<Vec<TrafficHealthChartRow>, DbError> {
        self.traffic_health_chart("access_line_id").await
    }

    pub(crate) async fn traffic_health_node_chart(
        &self,
    ) -> Result<Vec<TrafficHealthChartRow>, DbError> {
        Ok(sqlx::query_as::<_, TrafficHealthChartRow>(
            r#"
            WITH chart_points AS (
                SELECT l.access_node_id AS entity_id,
                       date_trunc('day', ul.collected_at) AS window_start,
                       COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::bigint AS real_bytes,
                       COALESCE(SUM(ul.billed_bytes), 0)::bigint AS billed_bytes
                FROM access_lines l
                JOIN usage_ledgers ul ON ul.access_line_id = l.id
                WHERE ul.traffic_source = 'access_line'
                  AND ul.collected_at >= date_trunc('month', now())
                GROUP BY l.access_node_id, window_start
                UNION ALL
                SELECT access_node_id AS entity_id,
                       (rollup_date::timestamp AT TIME ZONE 'UTC') AS window_start,
                       COALESCE(SUM(delta_total), 0)::bigint AS real_bytes,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_node_id IS NOT NULL
                  AND (rollup_date::timestamp AT TIME ZONE 'UTC') >= date_trunc('month', now())
                GROUP BY access_node_id, window_start
            )
            SELECT entity_id,
                   window_start,
                   COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
            FROM chart_points
            GROUP BY entity_id, window_start
            ORDER BY window_start ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub(crate) async fn traffic_health_exit_chart(
        &self,
    ) -> Result<Vec<TrafficHealthChartRow>, DbError> {
        self.traffic_health_chart("exit_endpoint_id").await
    }

    pub(crate) async fn traffic_health_group_chart(
        &self,
    ) -> Result<Vec<TrafficHealthChartRow>, DbError> {
        let query = format!(
            r#"
            WITH group_lines AS ({GROUP_LINES_SQL}),
                 chart_points AS (
                    SELECT gl.entity_id,
                           date_trunc('day', ul.collected_at) AS window_start,
                           COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::bigint AS real_bytes,
                           COALESCE(SUM(ul.billed_bytes), 0)::bigint AS billed_bytes
                    FROM group_lines gl
                    JOIN usage_ledgers ul ON ul.access_line_id = gl.access_line_id
                    WHERE ul.traffic_source = 'access_line'
                      AND ul.collected_at >= date_trunc('month', now())
                    GROUP BY gl.entity_id, window_start
                    UNION ALL
                    SELECT gl.entity_id,
                           (r.rollup_date::timestamp AT TIME ZONE 'UTC') AS window_start,
                           COALESCE(SUM(r.delta_total), 0)::bigint AS real_bytes,
                           COALESCE(SUM(r.billed_bytes), 0)::bigint AS billed_bytes
                    FROM group_lines gl
                    JOIN usage_daily_rollups r ON r.access_line_id = gl.access_line_id
                    WHERE r.traffic_source = 'access_line'
                      AND (r.rollup_date::timestamp AT TIME ZONE 'UTC') >= date_trunc('month', now())
                    GROUP BY gl.entity_id, window_start
                 )
            SELECT entity_id,
                   window_start,
                   COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
            FROM chart_points
            GROUP BY entity_id, window_start
            ORDER BY window_start ASC
            "#
        );
        Ok(sqlx::query_as::<_, TrafficHealthChartRow>(&query)
            .fetch_all(&self.pool)
            .await?)
    }

    pub(crate) async fn traffic_health_chart(
        &self,
        id_column: &str,
    ) -> Result<Vec<TrafficHealthChartRow>, DbError> {
        let not_null_filter = format!("AND {id_column} IS NOT NULL");
        let query = format!(
            r#"
            WITH chart_points AS (
                SELECT {id_column} AS entity_id,
                       date_trunc('day', collected_at) AS window_start,
                       COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS real_bytes,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  {not_null_filter}
                  AND collected_at >= date_trunc('month', now())
                GROUP BY {id_column}, window_start
                UNION ALL
                SELECT {id_column} AS entity_id,
                       (rollup_date::timestamp AT TIME ZONE 'UTC') AS window_start,
                       COALESCE(SUM(delta_total), 0)::bigint AS real_bytes,
                       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND {id_column} IS NOT NULL
                  AND (rollup_date::timestamp AT TIME ZONE 'UTC') >= date_trunc('month', now())
                GROUP BY {id_column}, window_start
            )
            SELECT entity_id,
                   window_start,
                   COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes
            FROM chart_points
            GROUP BY entity_id, window_start
            ORDER BY window_start ASC
            "#
        );
        Ok(sqlx::query_as::<_, TrafficHealthChartRow>(&query)
            .fetch_all(&self.pool)
            .await?)
    }
}
