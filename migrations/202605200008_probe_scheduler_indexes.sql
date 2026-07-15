-- 为中心侧定时探测调度补充反向查询索引。
-- Worker 需要从中转入口反查出口池成员，也需要按出口端点查找关联出口池。

CREATE INDEX IF NOT EXISTS idx_access_lines_enabled_pool_node
    ON access_lines(exit_pool_id, access_node_id)
    WHERE enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_exit_pool_members_endpoint_pool
    ON exit_pool_members(exit_endpoint_id, exit_pool_id);
