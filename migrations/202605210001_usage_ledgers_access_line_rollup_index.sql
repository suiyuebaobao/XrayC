-- 为运营中心账本排行和真实发布 large 压测补齐 access_line 聚合覆盖索引。
-- 该索引只覆盖当前主账本来源，不改变写入路径和业务模型。

CREATE INDEX IF NOT EXISTS idx_usage_ledgers_access_line_rollup
    ON usage_ledgers(access_line_id)
    INCLUDE (
        delta_uplink,
        delta_downlink,
        delta_total,
        billed_uplink,
        billed_downlink,
        billed_bytes,
        collected_at
    )
    WHERE traffic_source = 'access_line';
