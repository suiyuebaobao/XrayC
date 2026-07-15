-- 清理历史重复探针，再用唯一表达式索引保证 agent 重试并发幂等。
WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY access_line_id, status, COALESCE(latency_ms, -1), error_summary, probed_at
               ORDER BY id
           ) AS rn
    FROM access_line_probes
)
DELETE FROM access_line_probes p
USING ranked r
WHERE p.id = r.id AND r.rn > 1;

WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY access_node_id, exit_endpoint_id, status,
                   COALESCE(latency_ms, -1), error_summary, probed_at
               ORDER BY id
           ) AS rn
    FROM access_exit_probes
)
DELETE FROM access_exit_probes p
USING ranked r
WHERE p.id = r.id AND r.rn > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_access_line_probes_idempotency
    ON access_line_probes (
        access_line_id, status, COALESCE(latency_ms, -1), error_summary, probed_at
    );

CREATE UNIQUE INDEX IF NOT EXISTS idx_access_exit_probes_idempotency
    ON access_exit_probes (
        access_node_id, exit_endpoint_id, status,
        COALESCE(latency_ms, -1), error_summary, probed_at
    );
