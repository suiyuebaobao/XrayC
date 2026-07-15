-- 记录用户订阅拉取 IP，用于后台“设备管理”观察列表。
-- 本表只保存拉取事件，不恢复客户端设备绑定或专属订阅。
CREATE TABLE IF NOT EXISTS subscription_pull_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL DEFAULT '',
    client_ip TEXT NOT NULL DEFAULT '',
    client_ip_hash TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_subscription_pull_events_user_time
    ON subscription_pull_events(user_id, observed_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_subscription_pull_events_user_ip
    ON subscription_pull_events(user_id, client_ip, client_ip_hash, observed_at DESC);
