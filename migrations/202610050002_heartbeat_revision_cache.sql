-- 仅缓存配置生成时刻与已下发用户 ID，不复制用户凭据或完整运行配置。
-- 空生成时刻强制首次重建；配置变更仍沿用已有 dirty/hash 握手。
ALTER TABLE access_nodes ADD COLUMN config_built_at TIMESTAMPTZ;
ALTER TABLE access_nodes ADD COLUMN config_authorized_user_ids UUID[] NOT NULL DEFAULT '{}';
