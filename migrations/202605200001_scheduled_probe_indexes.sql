-- 为中心侧定时探测调度补充查询索引。
-- Worker 会按中转节点和出口端点组合查找待探测对象，queued 和最新结果分开索引。

CREATE INDEX IF NOT EXISTS idx_access_exit_probes_queued_tasks
    ON access_exit_probes(access_node_id, exit_endpoint_id, probed_at DESC)
    WHERE status = 'queued';

CREATE INDEX IF NOT EXISTS idx_access_exit_probes_result_latest
    ON access_exit_probes(access_node_id, exit_endpoint_id, probed_at DESC)
    WHERE status <> 'queued';

CREATE INDEX IF NOT EXISTS idx_access_line_metric_snapshots_collected_at
    ON access_line_metric_snapshots(collected_at);

CREATE INDEX IF NOT EXISTS idx_access_user_sessions_last_seen_at
    ON access_user_sessions(last_seen_at);

CREATE INDEX IF NOT EXISTS idx_access_line_probes_probed_at
    ON access_line_probes(probed_at);

CREATE INDEX IF NOT EXISTS idx_access_exit_probes_probed_at
    ON access_exit_probes(probed_at);
