-- 独立用户节点入站凭据，订阅重置时可轮换，不再暴露稳定 user_id。
ALTER TABLE users ADD COLUMN IF NOT EXISTS access_credential TEXT;

UPDATE users
SET access_credential = id::text
WHERE access_credential IS NULL OR btrim(access_credential) = '';

ALTER TABLE users ALTER COLUMN access_credential SET DEFAULT gen_random_uuid()::text;
ALTER TABLE users ALTER COLUMN access_credential SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_access_credential_unique
    ON users (access_credential);

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS chk_users_access_credential_not_blank;
ALTER TABLE users
    ADD CONSTRAINT chk_users_access_credential_not_blank
    CHECK (btrim(access_credential) <> '') NOT VALID;
ALTER TABLE users VALIDATE CONSTRAINT chk_users_access_credential_not_blank;
