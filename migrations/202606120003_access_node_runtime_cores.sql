-- 记录每个中转节点的 xray/sing-box 运行态与待执行控制任务。

CREATE TABLE IF NOT EXISTS access_node_runtime_cores (
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    core_type TEXT NOT NULL,
    desired_state TEXT NOT NULL DEFAULT 'running',
    reported_state TEXT NOT NULL DEFAULT 'unknown',
    control_action TEXT NOT NULL DEFAULT '',
    control_request_id UUID NULL,
    control_requested_at TIMESTAMPTZ NULL,
    control_completed_at TIMESTAMPTZ NULL,
    last_message TEXT NOT NULL DEFAULT '',
    last_report_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (access_node_id, core_type),
    CHECK (core_type IN ('xray', 'sing_box')),
    CHECK (desired_state IN ('running', 'stopped')),
    CHECK (reported_state IN ('running', 'stopped', 'starting', 'stopping', 'failed', 'unknown')),
    CHECK (control_action IN ('', 'start', 'stop'))
);

INSERT INTO access_node_runtime_cores (
    access_node_id, core_type, desired_state, reported_state
)
SELECT n.id, core.core_type, 'running', 'unknown'
FROM access_nodes n
CROSS JOIN (VALUES ('xray'), ('sing_box')) AS core(core_type)
ON CONFLICT (access_node_id, core_type) DO NOTHING;
