-- 为中转节点保存独立 SSH IP/域名。
-- public_host 继续作为客户连接地址进入订阅和入口默认值。
-- ssh_host 只用于管理后台运维、一键安装重试预填和人工记录。

ALTER TABLE access_nodes
    ADD COLUMN IF NOT EXISTS ssh_host TEXT NOT NULL DEFAULT '';
