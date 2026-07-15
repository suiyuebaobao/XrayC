-- 移除字段加密功能:删除轮换任务记录表及其索引。
-- 字段加密已下线,敏感字段改为明文直存,不再需要密钥轮换任务表。
-- 历史 CREATE 迁移(202605190002)保留作为变更记录,这里幂等 DROP。

DROP INDEX IF EXISTS idx_field_encryption_rotation_jobs_status_started_at;
DROP INDEX IF EXISTS idx_field_encryption_rotation_jobs_started_at;
DROP TABLE IF EXISTS field_encryption_rotation_jobs;
