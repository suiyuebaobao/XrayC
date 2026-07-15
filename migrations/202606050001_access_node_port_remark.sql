-- 为中转节点补充默认入口端口和管理员备注。
-- public_port 仅作为前端绑定入口的默认端口，不改变已有入口监听端口。

ALTER TABLE access_nodes
    ADD COLUMN IF NOT EXISTS public_port INTEGER NOT NULL DEFAULT 443,
    ADD COLUMN IF NOT EXISTS remark TEXT NOT NULL DEFAULT '';

ALTER TABLE access_nodes
    ADD CONSTRAINT access_nodes_public_port_range
    CHECK (public_port BETWEEN 1 AND 65535) NOT VALID;
