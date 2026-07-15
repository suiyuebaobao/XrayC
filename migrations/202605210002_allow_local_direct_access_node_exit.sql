-- 允许中转节点通过专用接口创建“本机出网”资源。
-- 通用线路池接口仍由代码层拒绝 direct/local_direct，避免误把 direct 当三方线路添加。
-- 该迁移只移除历史 guard 约束，实际写入入口仍必须走 /access-nodes/{id}/local-direct-exit。
-- 本机出网资源必须绑定 access_node_id，且只能生成 direct endpoint。

ALTER TABLE exit_endpoints
    DROP CONSTRAINT IF EXISTS chk_exit_endpoints_no_new_direct;

ALTER TABLE exit_resources
    DROP CONSTRAINT IF EXISTS chk_exit_resources_no_new_local_direct;
