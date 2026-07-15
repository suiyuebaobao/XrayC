-- Agent 在线终端快照需要按节点、线路、用户和客户端哈希天然幂等。
-- 旧实现由应用层 DELETE+INSERT 维持唯一性，高并发上报时会出现锁等待和重复快照窗口。
WITH ranked AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY access_node_id, access_line_id, xray_user_key, client_ip_hash
               ORDER BY last_seen_at DESC, started_at DESC, id DESC
           ) AS rn
    FROM access_user_sessions
)
DELETE FROM access_user_sessions s
USING ranked r
WHERE s.id = r.id
  AND r.rn > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_access_user_sessions_unique_snapshot
    ON access_user_sessions(access_node_id, access_line_id, xray_user_key, client_ip_hash);
