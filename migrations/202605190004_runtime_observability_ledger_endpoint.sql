ALTER TABLE access_line_metric_snapshots
    ADD COLUMN IF NOT EXISTS access_node_id UUID NULL REFERENCES access_nodes(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS exit_pool_id UUID NULL REFERENCES exit_pools(id) ON DELETE SET NULL;

UPDATE access_line_metric_snapshots m
SET access_node_id = l.access_node_id,
    exit_pool_id = l.exit_pool_id
FROM access_lines l
WHERE m.access_line_id = l.id
  AND (m.access_node_id IS NULL OR m.exit_pool_id IS NULL);

ALTER TABLE access_user_sessions
    ADD COLUMN IF NOT EXISTS active_connection_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'online';

ALTER TABLE access_user_sessions
    DROP CONSTRAINT IF EXISTS chk_access_user_sessions_active_connection_count;

ALTER TABLE access_user_sessions
    ADD CONSTRAINT chk_access_user_sessions_active_connection_count
    CHECK (active_connection_count >= 0);

ALTER TABLE access_user_sessions
    DROP CONSTRAINT IF EXISTS chk_access_user_sessions_status;

ALTER TABLE access_user_sessions
    ADD CONSTRAINT chk_access_user_sessions_status
    CHECK (status IN ('online', 'offline', 'expired', 'unknown'));

ALTER TABLE usage_ledgers
    ADD COLUMN IF NOT EXISTS exit_endpoint_id UUID NULL REFERENCES exit_endpoints(id) ON DELETE SET NULL;

UPDATE usage_ledgers ul
SET exit_endpoint_id = uea.exit_endpoint_id
FROM access_lines l
JOIN user_exit_assignments uea
  ON uea.access_line_id = l.id
 AND uea.exit_pool_id = l.exit_pool_id
WHERE ul.access_line_id = l.id
  AND ul.user_id = uea.user_id
  AND ul.exit_endpoint_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_access_line_metric_snapshots_node_pool_time
    ON access_line_metric_snapshots(access_node_id, exit_pool_id, collected_at DESC);

CREATE INDEX IF NOT EXISTS idx_access_user_sessions_status_seen
    ON access_user_sessions(access_line_id, status, last_seen_at DESC);

CREATE INDEX IF NOT EXISTS idx_usage_ledgers_endpoint_time
    ON usage_ledgers(exit_endpoint_id, collected_at DESC);
