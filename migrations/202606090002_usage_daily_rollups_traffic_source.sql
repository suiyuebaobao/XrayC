-- 历史流量日汇总必须保留账本来源，避免非 access_line 数据污染运营统计。

ALTER TABLE usage_daily_rollups
    ADD COLUMN IF NOT EXISTS traffic_source TEXT NOT NULL DEFAULT 'access_line';

ALTER TABLE usage_daily_rollups
    DROP CONSTRAINT IF EXISTS usage_daily_rollups_dimension_key;

ALTER TABLE usage_daily_rollups
    ADD CONSTRAINT usage_daily_rollups_dimension_key UNIQUE NULLS NOT DISTINCT (
        traffic_source,
        rollup_date,
        user_id,
        xray_user_key,
        access_line_id,
        access_node_id,
        exit_endpoint_id
    );

CREATE INDEX IF NOT EXISTS idx_usage_daily_rollups_source_date
    ON usage_daily_rollups(traffic_source, rollup_date DESC);
