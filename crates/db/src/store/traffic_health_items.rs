//! 健康检查流量统计列表查询。
//! 本文件从 traffic_health 拆出 line/node/exit/group 列表统计。
//! SQL 原样保留 usage_ledgers 与 rollup 的合并口径。
//! 入口模块仍负责拼装最终 JSON。
//! 本模块只返回聚合行，不暴露出口凭据。
//! 分组统计共用 traffic_health_sql 中的 CTE。
//! 查询失败仍转换为 DbError 供运营接口展示。
//! 新增实体维度时应保持月度排行排序一致。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 本头部满足前十行中文注释约束。

use super::rows::*;
use super::traffic_health_sql::GROUP_LINES_SQL;
use crate::*;

impl PgStore {
    pub(crate) async fn traffic_health_line_items(
        &self,
    ) -> Result<Vec<TrafficHealthEntityRow>, DbError> {
        self.traffic_health_items(
            "access_lines",
            r#"
            SELECT l.id AS entity_id,
                   COALESCE(NULLIF(l.name, ''), l.id::text) AS entity_name
            FROM access_lines l
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_real_bytes,
                   COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_billed_bytes
            FROM (
                SELECT access_line_id AS entity_id,
                       collected_at AS observed_at,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                UNION ALL
                SELECT access_line_id AS entity_id,
                       (rollup_date::timestamp AT TIME ZONE 'UTC') AS observed_at,
                       delta_total,
                       billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total), 0)::bigint AS total_real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS total_billed_bytes
            FROM (
                SELECT access_line_id AS entity_id,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
                UNION ALL
                SELECT access_line_id AS entity_id, delta_total, billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_line_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(MAX(real_bytes), 0)::bigint AS peak_hour_real_bytes,
                   COALESCE(MIN(NULLIF(real_bytes, 0)), 0)::bigint AS low_hour_real_bytes
            FROM (
                SELECT entity_id,
                       hour_start,
                       COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes
                FROM (
                    SELECT access_line_id AS entity_id,
                           date_trunc('hour', collected_at) AS hour_start,
                           COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS real_bytes
                    FROM usage_ledgers
                    WHERE traffic_source = 'access_line'
                      AND access_line_id IS NOT NULL
                      AND collected_at >= date_trunc('month', now())
                    GROUP BY access_line_id, hour_start
                    UNION ALL
                    SELECT access_line_id AS entity_id,
                           hour_start,
                           COALESCE(SUM(delta_total), 0)::bigint AS real_bytes
                    FROM usage_hourly_rollups
                    WHERE traffic_source = 'access_line'
                      AND access_line_id IS NOT NULL
                      AND hour_start >= date_trunc('month', now())
                    GROUP BY access_line_id, hour_start
                ) hourly_points
                GROUP BY entity_id, hour_start
            ) hourly
            GROUP BY entity_id
            "#,
        )
        .await
    }

    pub(crate) async fn traffic_health_node_items(
        &self,
    ) -> Result<Vec<TrafficHealthEntityRow>, DbError> {
        self.traffic_health_items(
            "access_nodes",
            r#"
            SELECT n.id AS entity_id,
                   COALESCE(NULLIF(n.name, ''), n.id::text) AS entity_name
            FROM access_nodes n
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_real_bytes,
                   COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_billed_bytes
            FROM (
                SELECT l.access_node_id AS entity_id,
                       ul.collected_at AS observed_at,
                       CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END AS delta_total,
                       ul.billed_bytes
                FROM access_lines l
                JOIN usage_ledgers ul ON ul.access_line_id = l.id
                WHERE ul.traffic_source = 'access_line'
                UNION ALL
                SELECT access_node_id AS entity_id,
                       (rollup_date::timestamp AT TIME ZONE 'UTC') AS observed_at,
                       delta_total,
                       billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_node_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total), 0)::bigint AS total_real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS total_billed_bytes
            FROM (
                SELECT l.access_node_id AS entity_id,
                       CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END AS delta_total,
                       ul.billed_bytes
                FROM access_lines l
                JOIN usage_ledgers ul ON ul.access_line_id = l.id
                WHERE ul.traffic_source = 'access_line'
                UNION ALL
                SELECT access_node_id AS entity_id, delta_total, billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND access_node_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(MAX(real_bytes), 0)::bigint AS peak_hour_real_bytes,
                   COALESCE(MIN(NULLIF(real_bytes, 0)), 0)::bigint AS low_hour_real_bytes
            FROM (
                SELECT entity_id,
                       hour_start,
                       COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes
                FROM (
                    SELECT l.access_node_id AS entity_id,
                           date_trunc('hour', ul.collected_at) AS hour_start,
                           COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::bigint AS real_bytes
                    FROM access_lines l
                    JOIN usage_ledgers ul ON ul.access_line_id = l.id
                    WHERE ul.traffic_source = 'access_line'
                      AND ul.collected_at >= date_trunc('month', now())
                    GROUP BY l.access_node_id, hour_start
                    UNION ALL
                    SELECT access_node_id AS entity_id,
                           hour_start,
                           COALESCE(SUM(delta_total), 0)::bigint AS real_bytes
                    FROM usage_hourly_rollups
                    WHERE traffic_source = 'access_line'
                      AND access_node_id IS NOT NULL
                      AND hour_start >= date_trunc('month', now())
                    GROUP BY access_node_id, hour_start
                ) hourly_points
                GROUP BY entity_id, hour_start
            ) hourly
            GROUP BY entity_id
            "#,
        )
        .await
    }

    pub(crate) async fn traffic_health_exit_items(
        &self,
    ) -> Result<Vec<TrafficHealthEntityRow>, DbError> {
        self.traffic_health_items(
            "exit_endpoints",
            r#"
            SELECT e.id AS entity_id,
                   COALESCE(NULLIF(e.name, ''), NULLIF(r.name, ''), e.id::text) AS entity_name
            FROM exit_endpoints e
            LEFT JOIN exit_resources r ON r.id = e.exit_resource_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_real_bytes,
                   COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_billed_bytes
            FROM (
                SELECT exit_endpoint_id AS entity_id,
                       collected_at AS observed_at,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND exit_endpoint_id IS NOT NULL
                UNION ALL
                SELECT exit_endpoint_id AS entity_id,
                       (rollup_date::timestamp AT TIME ZONE 'UTC') AS observed_at,
                       delta_total,
                       billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND exit_endpoint_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(SUM(delta_total), 0)::bigint AS total_real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS total_billed_bytes
            FROM (
                SELECT exit_endpoint_id AS entity_id,
                       CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END AS delta_total,
                       billed_bytes
                FROM usage_ledgers
                WHERE traffic_source = 'access_line'
                  AND exit_endpoint_id IS NOT NULL
                UNION ALL
                SELECT exit_endpoint_id AS entity_id, delta_total, billed_bytes
                FROM usage_daily_rollups
                WHERE traffic_source = 'access_line'
                  AND exit_endpoint_id IS NOT NULL
            ) traffic
            GROUP BY entity_id
            "#,
            r#"
            SELECT entity_id,
                   COALESCE(MAX(real_bytes), 0)::bigint AS peak_hour_real_bytes,
                   COALESCE(MIN(NULLIF(real_bytes, 0)), 0)::bigint AS low_hour_real_bytes
            FROM (
                SELECT entity_id,
                       hour_start,
                       COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes
                FROM (
                    SELECT exit_endpoint_id AS entity_id,
                           date_trunc('hour', collected_at) AS hour_start,
                           COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::bigint AS real_bytes
                    FROM usage_ledgers
                    WHERE traffic_source = 'access_line'
                      AND exit_endpoint_id IS NOT NULL
                      AND collected_at >= date_trunc('month', now())
                    GROUP BY exit_endpoint_id, hour_start
                    UNION ALL
                    SELECT exit_endpoint_id AS entity_id,
                           hour_start,
                           COALESCE(SUM(delta_total), 0)::bigint AS real_bytes
                    FROM usage_hourly_rollups
                    WHERE traffic_source = 'access_line'
                      AND exit_endpoint_id IS NOT NULL
                      AND hour_start >= date_trunc('month', now())
                    GROUP BY exit_endpoint_id, hour_start
                ) hourly_points
                GROUP BY entity_id, hour_start
            ) hourly
            GROUP BY entity_id
            "#,
        )
        .await
    }

    pub(crate) async fn traffic_health_group_items(
        &self,
    ) -> Result<Vec<TrafficHealthEntityRow>, DbError> {
        let period_sql = format!(
            r#"
            WITH group_lines AS ({GROUP_LINES_SQL})
            SELECT entity_id,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('day', now())), 0)::bigint AS today_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('week', now())), 0)::bigint AS week_real_bytes,
                   COALESCE(SUM(delta_total) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_real_bytes,
                   COALESCE(SUM(billed_bytes) FILTER (WHERE observed_at >= date_trunc('month', now())), 0)::bigint AS month_billed_bytes
            FROM (
                SELECT gl.entity_id,
                       ul.collected_at AS observed_at,
                       CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END AS delta_total,
                       ul.billed_bytes
                FROM group_lines gl
                JOIN usage_ledgers ul ON ul.access_line_id = gl.access_line_id
                WHERE ul.traffic_source = 'access_line'
                UNION ALL
                SELECT gl.entity_id,
                       (r.rollup_date::timestamp AT TIME ZONE 'UTC') AS observed_at,
                       r.delta_total,
                       r.billed_bytes
                FROM group_lines gl
                JOIN usage_daily_rollups r ON r.access_line_id = gl.access_line_id
                WHERE r.traffic_source = 'access_line'
            ) traffic
            GROUP BY entity_id
            "#
        );
        let total_sql = format!(
            r#"
            WITH group_lines AS ({GROUP_LINES_SQL})
            SELECT entity_id,
                   COALESCE(SUM(delta_total), 0)::bigint AS total_real_bytes,
                   COALESCE(SUM(billed_bytes), 0)::bigint AS total_billed_bytes
            FROM (
                SELECT gl.entity_id,
                       CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END AS delta_total,
                       ul.billed_bytes
                FROM group_lines gl
                JOIN usage_ledgers ul ON ul.access_line_id = gl.access_line_id
                WHERE ul.traffic_source = 'access_line'
                UNION ALL
                SELECT gl.entity_id, r.delta_total, r.billed_bytes
                FROM group_lines gl
                JOIN usage_daily_rollups r ON r.access_line_id = gl.access_line_id
                WHERE r.traffic_source = 'access_line'
            ) traffic
            GROUP BY entity_id
            "#
        );
        let hourly_sql = format!(
            r#"
            WITH group_lines AS ({GROUP_LINES_SQL})
            SELECT entity_id,
                   COALESCE(MAX(real_bytes), 0)::bigint AS peak_hour_real_bytes,
                   COALESCE(MIN(NULLIF(real_bytes, 0)), 0)::bigint AS low_hour_real_bytes
            FROM (
                SELECT entity_id,
                       hour_start,
                       COALESCE(SUM(real_bytes), 0)::bigint AS real_bytes
                FROM (
                    SELECT gl.entity_id,
                           date_trunc('hour', ul.collected_at) AS hour_start,
                           COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::bigint AS real_bytes
                    FROM group_lines gl
                    JOIN usage_ledgers ul ON ul.access_line_id = gl.access_line_id
                    WHERE ul.traffic_source = 'access_line'
                      AND ul.collected_at >= date_trunc('month', now())
                    GROUP BY gl.entity_id, hour_start
                    UNION ALL
                    SELECT gl.entity_id,
                           r.hour_start,
                           COALESCE(SUM(r.delta_total), 0)::bigint AS real_bytes
                    FROM group_lines gl
                    JOIN usage_hourly_rollups r ON r.access_line_id = gl.access_line_id
                    WHERE r.traffic_source = 'access_line'
                      AND r.hour_start >= date_trunc('month', now())
                    GROUP BY gl.entity_id, r.hour_start
                ) hourly_points
                GROUP BY entity_id, hour_start
            ) hourly
            GROUP BY entity_id
            "#
        );
        self.traffic_health_items(
            "line_groups",
            r#"
            SELECT g.id AS entity_id,
                   COALESCE(NULLIF(g.name, ''), g.id::text) AS entity_name
            FROM line_groups g
            WHERE g.enabled = TRUE
            "#,
            &period_sql,
            &total_sql,
            &hourly_sql,
        )
        .await
    }

    pub(crate) async fn traffic_health_items(
        &self,
        label: &str,
        entities_sql: &str,
        period_sql: &str,
        total_sql: &str,
        hourly_sql: &str,
    ) -> Result<Vec<TrafficHealthEntityRow>, DbError> {
        let query = format!(
            r#"
            WITH entities AS ({entities_sql}),
                 period_totals AS ({period_sql}),
                 all_totals AS ({total_sql}),
                 hourly_stats AS ({hourly_sql})
            SELECT e.entity_id,
                   e.entity_name,
                   COALESCE(pt.today_real_bytes, 0)::bigint AS today_real_bytes,
                   COALESCE(pt.week_real_bytes, 0)::bigint AS week_real_bytes,
                   COALESCE(pt.month_real_bytes, 0)::bigint AS month_real_bytes,
                   COALESCE(pt.month_billed_bytes, 0)::bigint AS month_billed_bytes,
                   COALESCE(at.total_real_bytes, 0)::bigint AS total_real_bytes,
                   COALESCE(at.total_billed_bytes, 0)::bigint AS total_billed_bytes,
                   COALESCE(hs.peak_hour_real_bytes, 0)::bigint AS peak_hour_real_bytes,
                   COALESCE(hs.low_hour_real_bytes, 0)::bigint AS low_hour_real_bytes,
                   (COALESCE(pt.month_real_bytes, 0) / GREATEST(EXTRACT(day FROM now())::bigint, 1))::bigint AS average_daily_real_bytes
            FROM entities e
            LEFT JOIN period_totals pt ON pt.entity_id = e.entity_id
            LEFT JOIN all_totals at ON at.entity_id = e.entity_id
            LEFT JOIN hourly_stats hs ON hs.entity_id = e.entity_id
            ORDER BY month_real_bytes DESC, entity_name ASC
            LIMIT 200
            "#
        );
        sqlx::query_as::<_, TrafficHealthEntityRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                DbError::InvalidAgentPayload(format!("{label} traffic query failed: {error}"))
            })
    }
}
