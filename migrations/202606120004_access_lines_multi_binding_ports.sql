-- 入口管理允许一个入口绑定多个出口；兼容运行表不再按端口唯一。
-- 实际入口端口唯一性由 access_entries_node_port_transport_idx 约束。

ALTER TABLE access_lines
    DROP CONSTRAINT IF EXISTS access_lines_access_node_id_listen_host_listen_port_key;

DROP INDEX IF EXISTS access_lines_access_node_id_listen_host_listen_port_key;

CREATE INDEX IF NOT EXISTS idx_access_lines_node_host_port
    ON access_lines(access_node_id, listen_host, listen_port);
