-- 增加订阅 Token 生命周期字段。
-- 当前保留 legacy token 列用于兼容既有订阅链接和页面展示，运行校验切换到 token_hash。

ALTER TABLE subscription_tokens
    ADD COLUMN IF NOT EXISTS token_hash TEXT NULL,
    ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '10 years'),
    ADD COLUMN IF NOT EXISTS last_used_at TIMESTAMPTZ NULL;

UPDATE subscription_tokens
SET token_hash = encode(digest(token, 'sha256'), 'hex')
WHERE token_hash IS NULL OR token_hash = '';

ALTER TABLE subscription_tokens
    ALTER COLUMN token_hash SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_subscription_tokens_token_hash
    ON subscription_tokens(token_hash);

CREATE INDEX IF NOT EXISTS idx_subscription_tokens_active_user
    ON subscription_tokens(user_id)
    WHERE revoked_at IS NULL;
