-- 长期保留按天聚合后的用户流量统计。
-- usage_ledgers 仍是详细账本；Worker 会先写入本表再删除过期明细。

CREATE TABLE IF NOT EXISTS usage_daily_rollups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rollup_date DATE NOT NULL,
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
    CONSTRAINT usage_daily_rollups_dimension_key UNIQUE NULLS NOT DISTINCT (
        rollup_date,
        user_id,
        xray_user_key,
        access_line_id,
        access_node_id,
        exit_endpoint_id
    )
);

CREATE INDEX IF NOT EXISTS idx_usage_daily_rollups_user_date
    ON usage_daily_rollups(user_id, rollup_date DESC);

CREATE INDEX IF NOT EXISTS idx_usage_daily_rollups_line_date
    ON usage_daily_rollups(access_line_id, rollup_date DESC)
    WHERE access_line_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_daily_rollups_exit_date
    ON usage_daily_rollups(exit_endpoint_id, rollup_date DESC)
    WHERE exit_endpoint_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_daily_rollups_node_date
    ON usage_daily_rollups(access_node_id, rollup_date DESC)
    WHERE access_node_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_usage_ledgers_retention_collected
    ON usage_ledgers(collected_at, id);
