-- 历史清理：删除已停用且没有具体线路 endpoint 的中转入口。
-- 这类记录通常来自旧的 ON DELETE SET NULL 外键或旧池绑定路径。
-- 当前主路径要求中转入口绑定具体 exit_endpoint_id；保留这些记录会让后台仍显示已删除线路的旧入口。

WITH deleted_lines AS (
    DELETE FROM access_lines
    WHERE exit_endpoint_id IS NULL
      AND enabled = FALSE
    RETURNING access_node_id
)
UPDATE access_nodes n
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'pruned_disabled_endpointless_access_lines'
WHERE n.id IN (SELECT access_node_id FROM deleted_lines);
