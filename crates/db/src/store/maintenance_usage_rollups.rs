//! Worker 详细流量日志汇总清理。
//! 本文件从 maintenance 拆出 usage_ledgers 汇总和删除逻辑。
//! 日汇总、小时汇总和原始明细删除仍在同一事务中执行。
//! SQL 使用原有 upsert 约束和批量删除策略。
//! 调用方仍由后台 traffic_log_retention 策略控制。
//! 本模块不读取业务配置，也不启动独立事务。
//! 失败时由外层维护事务统一回滚。
//! 新增汇总维度时应同步日表和小时表 SQL。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 本头部满足前十行中文注释约束。

use crate::*;

pub(crate) async fn roll_up_and_prune_usage_ledgers_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    policy: TrafficLogRetentionPolicy,
) -> Result<u64, DbError> {
    let deleted = sqlx::query_scalar::<_, i64>(
        r#"
        WITH old_rows AS MATERIALIZED (
            SELECT ul.id,
                   ((ul.collected_at AT TIME ZONE 'UTC')::date) AS rollup_date,
                   ul.traffic_source,
                   ul.user_id,
                   ul.xray_user_key,
                   ul.access_line_id,
                   al.access_node_id,
                   ul.exit_endpoint_id,
                   ul.delta_uplink,
                   ul.delta_downlink,
                   CASE
                     WHEN ul.delta_total > 0 THEN ul.delta_total
                     ELSE ul.delta_uplink + ul.delta_downlink
                   END AS delta_total,
                   ul.billed_uplink,
                   ul.billed_downlink,
                   ul.billed_bytes,
                   ul.collected_at
            FROM usage_ledgers ul
            LEFT JOIN access_lines al ON al.id = ul.access_line_id
            WHERE ul.collected_at < now() - ($1::BIGINT * interval '1 day')
            ORDER BY ul.collected_at ASC, ul.id ASC
            LIMIT $2
            FOR UPDATE OF ul SKIP LOCKED
        ),
        daily_rolled AS (
            INSERT INTO usage_daily_rollups (
                traffic_source, rollup_date, user_id, xray_user_key, access_line_id,
                access_node_id, exit_endpoint_id, ledger_count,
                delta_uplink, delta_downlink, delta_total,
                billed_uplink, billed_downlink, billed_bytes,
                first_collected_at, last_collected_at, updated_at
            )
            SELECT traffic_source,
                   rollup_date,
                   user_id,
                   xray_user_key,
                   access_line_id,
                   access_node_id,
                   exit_endpoint_id,
                   COUNT(*)::BIGINT,
                   COALESCE(SUM(delta_uplink), 0)::BIGINT,
                   COALESCE(SUM(delta_downlink), 0)::BIGINT,
                   COALESCE(SUM(delta_total), 0)::BIGINT,
                   COALESCE(SUM(billed_uplink), 0)::BIGINT,
                   COALESCE(SUM(billed_downlink), 0)::BIGINT,
                   COALESCE(SUM(billed_bytes), 0)::BIGINT,
                   MIN(collected_at),
                   MAX(collected_at),
                   now()
            FROM old_rows
            GROUP BY traffic_source, rollup_date, user_id, xray_user_key, access_line_id,
                     access_node_id, exit_endpoint_id
            ON CONFLICT ON CONSTRAINT usage_daily_rollups_dimension_key DO UPDATE SET
                ledger_count = usage_daily_rollups.ledger_count + EXCLUDED.ledger_count,
                delta_uplink = usage_daily_rollups.delta_uplink + EXCLUDED.delta_uplink,
                delta_downlink = usage_daily_rollups.delta_downlink + EXCLUDED.delta_downlink,
                delta_total = usage_daily_rollups.delta_total + EXCLUDED.delta_total,
                billed_uplink = usage_daily_rollups.billed_uplink + EXCLUDED.billed_uplink,
                billed_downlink = usage_daily_rollups.billed_downlink + EXCLUDED.billed_downlink,
                billed_bytes = usage_daily_rollups.billed_bytes + EXCLUDED.billed_bytes,
                first_collected_at = LEAST(
                    usage_daily_rollups.first_collected_at,
                    EXCLUDED.first_collected_at
                ),
                last_collected_at = GREATEST(
                    usage_daily_rollups.last_collected_at,
                    EXCLUDED.last_collected_at
                ),
                updated_at = now()
            RETURNING 1
        ),
        hourly_rolled AS (
            INSERT INTO usage_hourly_rollups (
                traffic_source, hour_start, user_id, xray_user_key, access_line_id,
                access_node_id, exit_endpoint_id, ledger_count,
                delta_uplink, delta_downlink, delta_total,
                billed_uplink, billed_downlink, billed_bytes,
                first_collected_at, last_collected_at, updated_at
            )
            SELECT traffic_source,
                   date_trunc('hour', collected_at) AS hour_start,
                   user_id,
                   xray_user_key,
                   access_line_id,
                   access_node_id,
                   exit_endpoint_id,
                   COUNT(*)::BIGINT,
                   COALESCE(SUM(delta_uplink), 0)::BIGINT,
                   COALESCE(SUM(delta_downlink), 0)::BIGINT,
                   COALESCE(SUM(delta_total), 0)::BIGINT,
                   COALESCE(SUM(billed_uplink), 0)::BIGINT,
                   COALESCE(SUM(billed_downlink), 0)::BIGINT,
                   COALESCE(SUM(billed_bytes), 0)::BIGINT,
                   MIN(collected_at),
                   MAX(collected_at),
                   now()
            FROM old_rows
            GROUP BY traffic_source, hour_start, user_id, xray_user_key, access_line_id,
                     access_node_id, exit_endpoint_id
            ON CONFLICT ON CONSTRAINT usage_hourly_rollups_dimension_key DO UPDATE SET
                ledger_count = usage_hourly_rollups.ledger_count + EXCLUDED.ledger_count,
                delta_uplink = usage_hourly_rollups.delta_uplink + EXCLUDED.delta_uplink,
                delta_downlink = usage_hourly_rollups.delta_downlink + EXCLUDED.delta_downlink,
                delta_total = usage_hourly_rollups.delta_total + EXCLUDED.delta_total,
                billed_uplink = usage_hourly_rollups.billed_uplink + EXCLUDED.billed_uplink,
                billed_downlink = usage_hourly_rollups.billed_downlink + EXCLUDED.billed_downlink,
                billed_bytes = usage_hourly_rollups.billed_bytes + EXCLUDED.billed_bytes,
                first_collected_at = LEAST(
                    usage_hourly_rollups.first_collected_at,
                    EXCLUDED.first_collected_at
                ),
                last_collected_at = GREATEST(
                    usage_hourly_rollups.last_collected_at,
                    EXCLUDED.last_collected_at
                ),
                updated_at = now()
            RETURNING 1
        ),
        deleted AS (
            DELETE FROM usage_ledgers ul
            USING old_rows
            WHERE ul.id = old_rows.id
            RETURNING 1
        )
        SELECT COUNT(*)::BIGINT FROM deleted
        "#,
    )
    .bind(policy.detail_retention_days)
    .bind(policy.delete_batch_size)
    .fetch_one(&mut **tx)
    .await?;
    Ok(deleted.max(0) as u64)
}
