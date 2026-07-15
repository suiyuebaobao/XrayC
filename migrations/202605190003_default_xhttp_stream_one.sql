-- 统一 XHTTP 新建线路默认模式，并给接入线路运行配置补齐约束。
-- 历史显式 auto 继续保留；空值或非法值归一到 stream-one，避免运行时下发非法配置。

ALTER TABLE access_lines
    ALTER COLUMN xhttp_mode SET DEFAULT 'stream-one';

UPDATE access_lines
SET xhttp_mode = lower(btrim(xhttp_mode));

UPDATE access_lines
SET xhttp_mode = 'stream-one'
WHERE xhttp_mode = ''
   OR xhttp_mode NOT IN ('auto', 'packet-up', 'stream-up', 'stream-one');

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_access_lines_xhttp_mode'
    ) THEN
        ALTER TABLE access_lines
            ADD CONSTRAINT chk_access_lines_xhttp_mode
            CHECK (xhttp_mode IN ('auto', 'packet-up', 'stream-up', 'stream-one'));
    END IF;
END $$;

ALTER TABLE field_encryption_rotation_jobs
    ADD COLUMN IF NOT EXISTS access_lines_scanned BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS rewritten_access_lines BIGINT NOT NULL DEFAULT 0;
