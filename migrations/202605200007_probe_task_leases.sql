-- 为中转出口探测任务增加领取租约，避免 heartbeat 期间重复下发同一 queued 任务。

ALTER TABLE access_exit_probes
    ADD COLUMN IF NOT EXISTS task_claimed_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS task_lease_expires_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS task_delivery_count INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_access_exit_probes_queued_leases
    ON access_exit_probes(access_node_id, task_lease_expires_at, probed_at DESC)
    WHERE status = 'queued';
