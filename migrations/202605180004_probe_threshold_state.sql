-- 维护接入节点到出口端点的组合探测状态。
-- 原始探测流水仍保存在 access_exit_probes，本表只保存阈值状态机的当前结果。

CREATE TABLE IF NOT EXISTS access_exit_probe_states (
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    effective_status TEXT NOT NULL DEFAULT 'unknown',
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    consecutive_successes INTEGER NOT NULL DEFAULT 0,
    last_probe_status TEXT NOT NULL DEFAULT '',
    last_latency_ms INTEGER NULL,
    last_error_summary TEXT NOT NULL DEFAULT '',
    last_probe_at TIMESTAMPTZ NULL,
    status_changed_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (access_node_id, exit_endpoint_id)
);

CREATE INDEX IF NOT EXISTS idx_access_exit_probe_states_endpoint
    ON access_exit_probe_states(exit_endpoint_id, effective_status);
