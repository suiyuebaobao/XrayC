-- 为本机 direct 出口资源记录所属接入节点。
-- 该字段用于阻止某台接入节点的本机出口被其他接入节点误绑定。

ALTER TABLE exit_resources
    ADD COLUMN IF NOT EXISTS access_node_id UUID NULL REFERENCES access_nodes(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_exit_resources_access_node
    ON exit_resources(access_node_id)
    WHERE access_node_id IS NOT NULL;

