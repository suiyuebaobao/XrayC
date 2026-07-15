-- 为运营中心账本排行新增线路级增量汇总。
-- usage_ledgers 仍保存明细，本表只服务大数据量读取路径。

CREATE TABLE IF NOT EXISTS access_line_usage_rollups (
    access_line_id UUID PRIMARY KEY REFERENCES access_lines(id) ON DELETE CASCADE,
    ledger_count BIGINT NOT NULL DEFAULT 0,
    delta_uplink BIGINT NOT NULL DEFAULT 0,
    delta_downlink BIGINT NOT NULL DEFAULT 0,
    delta_total BIGINT NOT NULL DEFAULT 0,
    billed_uplink BIGINT NOT NULL DEFAULT 0,
    billed_downlink BIGINT NOT NULL DEFAULT 0,
    billed_bytes BIGINT NOT NULL DEFAULT 0,
    latest_collected_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_access_line_usage_rollups_ranking
    ON access_line_usage_rollups(
        billed_bytes DESC,
        delta_total DESC,
        ledger_count DESC,
        access_line_id
    );

CREATE OR REPLACE FUNCTION xrayc_usage_ledgers_rollup_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO access_line_usage_rollups (
        access_line_id, ledger_count, delta_uplink, delta_downlink,
        delta_total, billed_uplink, billed_downlink, billed_bytes,
        latest_collected_at, updated_at
    )
    SELECT
        access_line_id,
        COUNT(*)::BIGINT,
        COALESCE(SUM(delta_uplink), 0)::BIGINT,
        COALESCE(SUM(delta_downlink), 0)::BIGINT,
        COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::BIGINT,
        COALESCE(SUM(billed_uplink), 0)::BIGINT,
        COALESCE(SUM(billed_downlink), 0)::BIGINT,
        COALESCE(SUM(billed_bytes), 0)::BIGINT,
        MAX(collected_at),
        now()
    FROM new_usage_ledgers
    WHERE traffic_source = 'access_line'
      AND access_line_id IS NOT NULL
    GROUP BY access_line_id
    ON CONFLICT (access_line_id) DO UPDATE SET
        ledger_count = access_line_usage_rollups.ledger_count + EXCLUDED.ledger_count,
        delta_uplink = access_line_usage_rollups.delta_uplink + EXCLUDED.delta_uplink,
        delta_downlink = access_line_usage_rollups.delta_downlink + EXCLUDED.delta_downlink,
        delta_total = access_line_usage_rollups.delta_total + EXCLUDED.delta_total,
        billed_uplink = access_line_usage_rollups.billed_uplink + EXCLUDED.billed_uplink,
        billed_downlink = access_line_usage_rollups.billed_downlink + EXCLUDED.billed_downlink,
        billed_bytes = access_line_usage_rollups.billed_bytes + EXCLUDED.billed_bytes,
        latest_collected_at = GREATEST(access_line_usage_rollups.latest_collected_at, EXCLUDED.latest_collected_at),
        updated_at = now();
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION xrayc_usage_ledgers_rollup_rebuild_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM access_line_usage_rollups r
    USING (
        SELECT DISTINCT access_line_id
        FROM old_usage_ledgers
        WHERE traffic_source = 'access_line'
          AND access_line_id IS NOT NULL
    ) affected
    WHERE r.access_line_id = affected.access_line_id;

    INSERT INTO access_line_usage_rollups (
        access_line_id, ledger_count, delta_uplink, delta_downlink,
        delta_total, billed_uplink, billed_downlink, billed_bytes,
        latest_collected_at, updated_at
    )
    SELECT
        ul.access_line_id,
        COUNT(*)::BIGINT,
        COALESCE(SUM(ul.delta_uplink), 0)::BIGINT,
        COALESCE(SUM(ul.delta_downlink), 0)::BIGINT,
        COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::BIGINT,
        COALESCE(SUM(ul.billed_uplink), 0)::BIGINT,
        COALESCE(SUM(ul.billed_downlink), 0)::BIGINT,
        COALESCE(SUM(ul.billed_bytes), 0)::BIGINT,
        MAX(ul.collected_at),
        now()
    FROM usage_ledgers ul
    JOIN (
        SELECT DISTINCT access_line_id
        FROM old_usage_ledgers
        WHERE traffic_source = 'access_line'
          AND access_line_id IS NOT NULL
    ) affected ON affected.access_line_id = ul.access_line_id
    WHERE ul.traffic_source = 'access_line'
      AND ul.access_line_id IS NOT NULL
    GROUP BY ul.access_line_id;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION xrayc_usage_ledgers_rollup_rebuild_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM access_line_usage_rollups r
    USING (
        SELECT access_line_id FROM old_usage_ledgers WHERE traffic_source = 'access_line' AND access_line_id IS NOT NULL
        UNION
        SELECT access_line_id FROM new_usage_ledgers WHERE traffic_source = 'access_line' AND access_line_id IS NOT NULL
    ) affected
    WHERE r.access_line_id = affected.access_line_id;

    INSERT INTO access_line_usage_rollups (
        access_line_id, ledger_count, delta_uplink, delta_downlink,
        delta_total, billed_uplink, billed_downlink, billed_bytes,
        latest_collected_at, updated_at
    )
    SELECT
        ul.access_line_id,
        COUNT(*)::BIGINT,
        COALESCE(SUM(ul.delta_uplink), 0)::BIGINT,
        COALESCE(SUM(ul.delta_downlink), 0)::BIGINT,
        COALESCE(SUM(CASE WHEN ul.delta_total > 0 THEN ul.delta_total ELSE ul.delta_uplink + ul.delta_downlink END), 0)::BIGINT,
        COALESCE(SUM(ul.billed_uplink), 0)::BIGINT,
        COALESCE(SUM(ul.billed_downlink), 0)::BIGINT,
        COALESCE(SUM(ul.billed_bytes), 0)::BIGINT,
        MAX(ul.collected_at),
        now()
    FROM usage_ledgers ul
    JOIN (
        SELECT access_line_id FROM old_usage_ledgers WHERE traffic_source = 'access_line' AND access_line_id IS NOT NULL
        UNION
        SELECT access_line_id FROM new_usage_ledgers WHERE traffic_source = 'access_line' AND access_line_id IS NOT NULL
    ) affected ON affected.access_line_id = ul.access_line_id
    WHERE ul.traffic_source = 'access_line'
      AND ul.access_line_id IS NOT NULL
    GROUP BY ul.access_line_id;
    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS trg_usage_ledgers_rollup_insert ON usage_ledgers;
CREATE TRIGGER trg_usage_ledgers_rollup_insert
AFTER INSERT ON usage_ledgers
REFERENCING NEW TABLE AS new_usage_ledgers
FOR EACH STATEMENT
EXECUTE FUNCTION xrayc_usage_ledgers_rollup_insert();

DROP TRIGGER IF EXISTS trg_usage_ledgers_rollup_delete ON usage_ledgers;
CREATE TRIGGER trg_usage_ledgers_rollup_delete
AFTER DELETE ON usage_ledgers
REFERENCING OLD TABLE AS old_usage_ledgers
FOR EACH STATEMENT
EXECUTE FUNCTION xrayc_usage_ledgers_rollup_rebuild_delete();

DROP TRIGGER IF EXISTS trg_usage_ledgers_rollup_update ON usage_ledgers;
CREATE TRIGGER trg_usage_ledgers_rollup_update
AFTER UPDATE ON usage_ledgers
REFERENCING OLD TABLE AS old_usage_ledgers NEW TABLE AS new_usage_ledgers
FOR EACH STATEMENT
EXECUTE FUNCTION xrayc_usage_ledgers_rollup_rebuild_update();

TRUNCATE TABLE access_line_usage_rollups;
INSERT INTO access_line_usage_rollups (
    access_line_id, ledger_count, delta_uplink, delta_downlink,
    delta_total, billed_uplink, billed_downlink, billed_bytes,
    latest_collected_at, updated_at
)
SELECT
    access_line_id,
    COUNT(*)::BIGINT,
    COALESCE(SUM(delta_uplink), 0)::BIGINT,
    COALESCE(SUM(delta_downlink), 0)::BIGINT,
    COALESCE(SUM(CASE WHEN delta_total > 0 THEN delta_total ELSE delta_uplink + delta_downlink END), 0)::BIGINT,
    COALESCE(SUM(billed_uplink), 0)::BIGINT,
    COALESCE(SUM(billed_downlink), 0)::BIGINT,
    COALESCE(SUM(billed_bytes), 0)::BIGINT,
    MAX(collected_at),
    now()
FROM usage_ledgers
WHERE traffic_source = 'access_line'
  AND access_line_id IS NOT NULL
GROUP BY access_line_id;
