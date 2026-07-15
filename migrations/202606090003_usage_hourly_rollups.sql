-- 详细账本清理后仍需保留小时级峰值/低谷和近小时窗口统计。

CREATE TABLE IF NOT EXISTS usage_hourly_rollups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    traffic_source TEXT NOT NULL DEFAULT 'access_line',
    hour_start TIMESTAMPTZ NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    xray_user_key TEXT NOT NULL,
    access_line_id UUID NULL REFERENCES access_lines(id) ON DELETE SET NULL,
    access_node_id UUID NULL REFERENCES access_nodes(id) ON DELETE SET NULL,
    exit_endpoint_id UUID NULL REFERENCES exit_endpoints(id) ON DELETE SET NULL,
    ledger_count BIGINT NOT NULL DEFAULT 0,
    delta_uplink BIGINT NOT NULL DEFAULT 0,
    delta_downlink BIGINT NOT NULL DEFAULT 0,
    delta_total BIGINT NOT NULL DEFAULT 0,
    billed_uplink BIGINT NOT NULL DEFAULT 0,
    billed_downlink BIGINT NOT NULL DEFAULT 0,
    billed_bytes BIGINT NOT NULL DEFAULT 0,
    first_collected_at TIMESTAMPTZ NULL,
    last_collected_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT usage_hourly_rollups_dimension_key UNIQUE NULLS NOT DISTINCT (
        traffic_source,
        hour_start,
        user_id,
        xray_user_key,
        access_line_id,
        access_node_id,
        exit_endpoint_id
    )
);

CREATE INDEX IF NOT EXISTS idx_usage_hourly_rollups_user_hour
    ON usage_hourly_rollups(user_id, hour_start DESC);

CREATE INDEX IF NOT EXISTS idx_usage_hourly_rollups_line_hour
    ON usage_hourly_rollups(access_line_id, hour_start DESC)
    WHERE access_line_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_hourly_rollups_exit_hour
    ON usage_hourly_rollups(exit_endpoint_id, hour_start DESC)
    WHERE exit_endpoint_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_hourly_rollups_node_hour
    ON usage_hourly_rollups(access_node_id, hour_start DESC)
    WHERE access_node_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_hourly_rollups_source_hour
    ON usage_hourly_rollups(traffic_source, hour_start DESC);
