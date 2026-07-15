-- 当前 V2 主线不再允许新增本机 direct/local_direct 出口。
-- 保留 enum 值仅用于读取历史数据；NOT VALID 允许既有遗留行存在，但所有新写入必须走普通上游出口。

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_exit_endpoints_no_new_direct'
    ) THEN
        ALTER TABLE exit_endpoints
            ADD CONSTRAINT chk_exit_endpoints_no_new_direct
            CHECK (outbound_type <> 'direct') NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_exit_resources_no_new_local_direct'
    ) THEN
        ALTER TABLE exit_resources
            ADD CONSTRAINT chk_exit_resources_no_new_local_direct
            CHECK (ownership <> 'local_direct') NOT VALID;
    END IF;
END $$;
