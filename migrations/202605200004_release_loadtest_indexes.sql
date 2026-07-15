-- 补齐真实发布压测中暴露的审计、探测和运行快照查询索引。
-- 这些索引只服务读路径和清理路径，不改变业务数据模型。

CREATE INDEX IF NOT EXISTS idx_audit_logs_created_id_desc
    ON audit_logs(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_access_traffic_snapshots_collected_at
    ON access_traffic_snapshots(collected_at);

CREATE INDEX IF NOT EXISTS idx_access_exit_probe_states_updated_at
    ON access_exit_probe_states(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_access_nodes_activity_order
    ON access_nodes((COALESCE(last_heartbeat_at, created_at)) DESC, id);

CREATE INDEX IF NOT EXISTS idx_access_user_sessions_online_last_seen
    ON access_user_sessions(access_node_id, access_line_id, last_seen_at DESC)
    WHERE status = 'online';
