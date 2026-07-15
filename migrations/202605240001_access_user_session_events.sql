-- 用户级流量日志需要把会话 IP 作为追加事件保存。
-- usage_ledgers 负责精确流量，session events 负责按时间关联访问 IP。
CREATE TABLE IF NOT EXISTS access_user_session_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    access_line_id UUID NULL REFERENCES access_lines(id) ON DELETE SET NULL,
    user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    xray_user_key TEXT NOT NULL,
    client_ip TEXT NOT NULL DEFAULT '',
    client_ip_hash TEXT NOT NULL DEFAULT '',
    active_connection_count INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'online',
    observed_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_access_user_session_events_active_connection_count
        CHECK (active_connection_count >= 0),
    CONSTRAINT chk_access_user_session_events_status
        CHECK (status IN ('online', 'offline', 'expired', 'unknown'))
);

CREATE INDEX IF NOT EXISTS idx_access_user_session_events_user_time
    ON access_user_session_events(user_id, observed_at DESC);

CREATE INDEX IF NOT EXISTS idx_access_user_session_events_line_user_time
    ON access_user_session_events(access_line_id, xray_user_key, observed_at DESC);
