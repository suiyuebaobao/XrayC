-- 允许本机出口服务以 self_hosted 资源绑定承载它的中转节点。
-- local_direct 继续只服务历史 direct 兼容路径，third_party 仍不能绑定节点。

ALTER TABLE exit_resources
    DROP CONSTRAINT IF EXISTS chk_exit_resources_local_direct_owner;

ALTER TABLE exit_resources
    ADD CONSTRAINT chk_exit_resources_local_direct_owner
    CHECK (
        (ownership IN ('local_direct', 'self_hosted') AND access_node_id IS NOT NULL)
        OR (ownership = 'third_party' AND access_node_id IS NULL)
    ) NOT VALID;
