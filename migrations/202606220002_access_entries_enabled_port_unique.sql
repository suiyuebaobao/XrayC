-- 入口端口冲突收敛:同一中转节点下 enabled 入口的 listen_port 必须唯一。
-- 背景:Xray 同端口无法承载多协议入站,同节点同端口入口会让 reload bind 失败回滚。
-- 旧索引 access_entries_node_port_transport_idx 按 (节点,端口,协议,传输,path) 唯一,
-- 既允许跨协议同端口(实际会 bind 冲突),又把 disabled 入口纳入唯一性(不应占用端口)。
-- 这里改成协议无关、仅对 enabled 入口生效的部分唯一索引,作为端口唯一性的数据库兜底;
-- 应用层另有更友好的预校验返回"端口已被占用",二者口径一致。
DROP INDEX IF EXISTS access_entries_node_port_transport_idx;

CREATE UNIQUE INDEX IF NOT EXISTS access_entries_node_enabled_port_idx
    ON access_entries (access_node_id, listen_port)
    WHERE enabled = true;
