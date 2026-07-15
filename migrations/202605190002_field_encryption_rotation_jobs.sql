-- 字段加密密钥轮换任务记录。
-- 只记录 kid、计数和错误摘要，不保存 key material、密文或明文。

CREATE TABLE IF NOT EXISTS field_encryption_rotation_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    dry_run BOOLEAN NOT NULL DEFAULT TRUE,
    primary_kid TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'completed', 'failed')),
    key_count INTEGER NOT NULL DEFAULT 0,
    site_settings_scanned BIGINT NOT NULL DEFAULT 0,
    exit_endpoints_scanned BIGINT NOT NULL DEFAULT 0,
    rewritten_site_settings BIGINT NOT NULL DEFAULT 0,
    rewritten_exit_endpoints BIGINT NOT NULL DEFAULT 0,
    rotated_fields BIGINT NOT NULL DEFAULT 0,
    plaintext_fields_sealed BIGINT NOT NULL DEFAULT 0,
    already_primary_fields BIGINT NOT NULL DEFAULT 0,
    error_summary TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_field_encryption_rotation_jobs_started_at
    ON field_encryption_rotation_jobs(started_at DESC);

CREATE INDEX IF NOT EXISTS idx_field_encryption_rotation_jobs_status_started_at
    ON field_encryption_rotation_jobs(status, started_at DESC);
