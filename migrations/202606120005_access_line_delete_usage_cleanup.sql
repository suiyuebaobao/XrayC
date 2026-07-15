-- 修复删除 access_lines 时 usage_ledgers 汇总触发器的父行竞态。
-- 旧触发器在 ON DELETE SET NULL 期间可能按已删除的 access_line_id 重建汇总。
-- 新逻辑只重建仍存在的 access_lines，并显式修正汇总表外键为级联删除。

ALTER TABLE access_line_usage_rollups
    DROP CONSTRAINT IF EXISTS access_line_usage_rollups_access_line_id_fkey;

ALTER TABLE access_line_usage_rollups
    ADD CONSTRAINT access_line_usage_rollups_access_line_id_fkey
    FOREIGN KEY (access_line_id)
    REFERENCES access_lines(id)
    ON DELETE CASCADE;

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
    JOIN access_lines al ON al.id = ul.access_line_id
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
    JOIN access_lines al ON al.id = ul.access_line_id
    WHERE ul.traffic_source = 'access_line'
      AND ul.access_line_id IS NOT NULL
    GROUP BY ul.access_line_id;
    RETURN NULL;
END;
$$;
