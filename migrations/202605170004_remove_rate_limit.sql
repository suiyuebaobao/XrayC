-- 移除已下线的用户级限速结构。
-- 当前 MVP 不再保留限速入口、端口池和 tc/ifb 相关数据模型，
-- 新部署环境不应继续创建或依赖这些废弃表与字段。

DROP TABLE IF EXISTS user_rate_limit_assignments;
DROP TABLE IF EXISTS rate_limit_profiles;

ALTER TABLE access_nodes
    DROP COLUMN IF EXISTS rate_limit_port_start,
    DROP COLUMN IF EXISTS rate_limit_port_end;
