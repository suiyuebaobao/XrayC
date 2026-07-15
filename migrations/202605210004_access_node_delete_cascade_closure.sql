-- 修正中转节点删除闭环中的本机出口资源外键。
-- 早期迁移曾用 IF NOT EXISTS 声明 access_node_id，导致新库保留 SET NULL 行为。
-- 删除中转节点时本机出口资源应随节点级联删除，避免资源孤儿和统计失真。

ALTER TABLE exit_resources
    DROP CONSTRAINT IF EXISTS exit_resources_access_node_id_fkey;

ALTER TABLE exit_resources
    ADD CONSTRAINT exit_resources_access_node_id_fkey
    FOREIGN KEY (access_node_id)
    REFERENCES access_nodes(id)
    ON DELETE CASCADE;
