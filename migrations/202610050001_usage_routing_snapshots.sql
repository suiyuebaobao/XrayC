-- 历史路由身份独立于可删除的运行配置。保存 ID 和名称，不保存地址、凭据或配置。
-- 原有活跃外键继续有效；解绑不会合并不同历史线路，也不会改动计费总量。
CREATE TABLE usage_routing_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_line_id UUID,
    access_node_id UUID,
    exit_endpoint_id UUID,
    access_line_name TEXT NOT NULL DEFAULT '',
    access_node_name TEXT NOT NULL DEFAULT '',
    exit_endpoint_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT usage_routing_snapshot_identity UNIQUE NULLS NOT DISTINCT (
        access_line_id, access_node_id, exit_endpoint_id,
        access_line_name, access_node_name, exit_endpoint_name
    )
);

CREATE FUNCTION xrayc_usage_routing_snapshot(line_id UUID, node_id UUID, endpoint_id UUID)
RETURNS UUID LANGUAGE plpgsql AS $$
DECLARE
    snapshot_id UUID;
    actual_node_id UUID := node_id;
    line_label TEXT := '';
    node_label TEXT := '';
    endpoint_label TEXT := '';
BEGIN
    SELECT name, COALESCE(node_id, access_node_id)
    INTO line_label, actual_node_id FROM access_lines WHERE id = line_id;
    -- 没有活跃线路的旧彙总仍可能记录着节点 ID。
    IF actual_node_id IS NULL THEN actual_node_id := node_id; END IF;
    SELECT name INTO node_label FROM access_nodes WHERE id = actual_node_id;
    SELECT name INTO endpoint_label FROM exit_endpoints WHERE id = endpoint_id;
    line_label := COALESCE(line_label, '');
    node_label := COALESCE(node_label, '');
    endpoint_label := COALESCE(endpoint_label, '');
    LOOP
        SELECT id INTO snapshot_id FROM usage_routing_snapshots
        WHERE access_line_id IS NOT DISTINCT FROM line_id
          AND access_node_id IS NOT DISTINCT FROM actual_node_id
          AND exit_endpoint_id IS NOT DISTINCT FROM endpoint_id
          AND access_line_name = line_label
          AND access_node_name = node_label
          AND exit_endpoint_name = endpoint_label;
        IF FOUND THEN RETURN snapshot_id; END IF;
        INSERT INTO usage_routing_snapshots (
            access_line_id, access_node_id, exit_endpoint_id,
            access_line_name, access_node_name, exit_endpoint_name
        ) VALUES (line_id, actual_node_id, endpoint_id, line_label, node_label, endpoint_label)
        ON CONFLICT ON CONSTRAINT usage_routing_snapshot_identity DO NOTHING
        RETURNING id INTO snapshot_id;
        IF FOUND THEN RETURN snapshot_id; END IF;
    END LOOP;
END;
$$;

ALTER TABLE usage_ledgers ADD COLUMN routing_snapshot_id UUID REFERENCES usage_routing_snapshots(id);
ALTER TABLE usage_daily_rollups ADD COLUMN routing_snapshot_id UUID REFERENCES usage_routing_snapshots(id);
ALTER TABLE usage_hourly_rollups ADD COLUMN routing_snapshot_id UUID REFERENCES usage_routing_snapshots(id);

-- 只补现存身份信息，金额、时间、行数保持不变；已失去的旧名称无法事后还原。
UPDATE usage_ledgers SET routing_snapshot_id = xrayc_usage_routing_snapshot(access_line_id, NULL, exit_endpoint_id);
UPDATE usage_daily_rollups SET routing_snapshot_id = xrayc_usage_routing_snapshot(access_line_id, access_node_id, exit_endpoint_id);
UPDATE usage_hourly_rollups SET routing_snapshot_id = xrayc_usage_routing_snapshot(access_line_id, access_node_id, exit_endpoint_id);

CREATE FUNCTION xrayc_capture_usage_routing_snapshot()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.routing_snapshot_id IS DISTINCT FROM OLD.routing_snapshot_id THEN
            RAISE EXCEPTION '历史路由快照不可修改';
        END IF;
    ELSIF NEW.routing_snapshot_id IS NULL THEN
        IF TG_TABLE_NAME = 'usage_ledgers' THEN
            NEW.routing_snapshot_id := xrayc_usage_routing_snapshot(NEW.access_line_id, NULL, NEW.exit_endpoint_id);
        ELSE
            NEW.routing_snapshot_id := xrayc_usage_routing_snapshot(NEW.access_line_id, NEW.access_node_id, NEW.exit_endpoint_id);
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_usage_ledgers_routing_snapshot BEFORE INSERT OR UPDATE ON usage_ledgers
FOR EACH ROW EXECUTE FUNCTION xrayc_capture_usage_routing_snapshot();
CREATE TRIGGER trg_usage_daily_routing_snapshot BEFORE INSERT OR UPDATE ON usage_daily_rollups
FOR EACH ROW EXECUTE FUNCTION xrayc_capture_usage_routing_snapshot();
CREATE TRIGGER trg_usage_hourly_routing_snapshot BEFORE INSERT OR UPDATE ON usage_hourly_rollups
FOR EACH ROW EXECUTE FUNCTION xrayc_capture_usage_routing_snapshot();

ALTER TABLE usage_ledgers ALTER COLUMN routing_snapshot_id SET NOT NULL;
ALTER TABLE usage_daily_rollups ALTER COLUMN routing_snapshot_id SET NOT NULL;
ALTER TABLE usage_hourly_rollups ALTER COLUMN routing_snapshot_id SET NOT NULL;
ALTER TABLE usage_daily_rollups DROP CONSTRAINT usage_daily_rollups_dimension_key;
ALTER TABLE usage_daily_rollups ADD CONSTRAINT usage_daily_rollups_dimension_key
UNIQUE NULLS NOT DISTINCT (traffic_source, rollup_date, user_id, xray_user_key, routing_snapshot_id);
ALTER TABLE usage_hourly_rollups DROP CONSTRAINT usage_hourly_rollups_dimension_key;
ALTER TABLE usage_hourly_rollups ADD CONSTRAINT usage_hourly_rollups_dimension_key
UNIQUE NULLS NOT DISTINCT (traffic_source, hour_start, user_id, xray_user_key, routing_snapshot_id);

CREATE INDEX idx_usage_routing_snapshot_line ON usage_routing_snapshots(access_line_id);
CREATE INDEX idx_usage_routing_snapshot_node ON usage_routing_snapshots(access_node_id);
CREATE INDEX idx_usage_routing_snapshot_endpoint ON usage_routing_snapshots(exit_endpoint_id);
