-- 允许删除中转节点时保留历史账本。
-- access_lines 被删除后，历史 usage_ledgers 不应阻塞节点删除。
-- 运营排行和计费追溯继续依赖已保留的账本字段。
ALTER TABLE usage_ledgers
    DROP CONSTRAINT IF EXISTS usage_ledgers_access_line_id_fkey;

ALTER TABLE usage_ledgers
    ALTER COLUMN access_line_id DROP NOT NULL;

ALTER TABLE usage_ledgers
    ADD CONSTRAINT usage_ledgers_access_line_id_fkey
    FOREIGN KEY (access_line_id)
    REFERENCES access_lines(id)
    ON DELETE SET NULL;
