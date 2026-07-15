-- 商业客户端 V1 设备绑定表。
-- 设备凭据只保存哈希，客户端专用 profile 通过用户 JWT 和设备凭据校验。

CREATE TABLE IF NOT EXISTS client_devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_fingerprint TEXT NOT NULL,
    device_name TEXT NOT NULL DEFAULT '',
    platform TEXT NOT NULL DEFAULT '',
    arch TEXT NOT NULL DEFAULT '',
    client_version TEXT NOT NULL DEFAULT '',
    core_version TEXT NOT NULL DEFAULT '',
    device_token_hash TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    last_seen_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, device_fingerprint)
);

CREATE INDEX IF NOT EXISTS idx_client_devices_user_status
    ON client_devices(user_id, status);

CREATE INDEX IF NOT EXISTS idx_client_devices_last_seen
    ON client_devices(last_seen_at DESC);
