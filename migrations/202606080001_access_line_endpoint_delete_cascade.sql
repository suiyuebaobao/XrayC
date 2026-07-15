-- 删除线路池线路时，中转节点入口绑定必须同步删除。
-- 旧约束使用 ON DELETE SET NULL，会留下无出口的 access_lines 孤儿记录。
-- 先清理历史孤儿入口，再把外键改成级联删除，防止后台或 SQL 直删再次残留。

DELETE FROM access_lines
WHERE exit_endpoint_id IS NULL
  AND line_group_id IS NULL;

ALTER TABLE access_lines
    DROP CONSTRAINT IF EXISTS access_lines_exit_endpoint_id_fkey;

ALTER TABLE access_lines
    ADD CONSTRAINT access_lines_exit_endpoint_id_fkey
    FOREIGN KEY (exit_endpoint_id)
    REFERENCES exit_endpoints(id)
    ON DELETE CASCADE;
