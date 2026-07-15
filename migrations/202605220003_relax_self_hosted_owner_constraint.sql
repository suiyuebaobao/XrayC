-- 普通自建出口可以不绑定中转节点，本机出口服务才需要绑定承载节点。
-- local_direct 仍只允许历史兼容记录绑定节点，third_party 仍保持无节点归属。

ALTER TABLE exit_resources
    DROP CONSTRAINT IF EXISTS chk_exit_resources_local_direct_owner;

ALTER TABLE exit_resources
    ADD CONSTRAINT chk_exit_resources_local_direct_owner
    CHECK (
        (ownership = 'local_direct' AND access_node_id IS NOT NULL)
        OR (ownership = 'self_hosted')
        OR (ownership = 'third_party' AND access_node_id IS NULL)
    ) NOT VALID;
