-- 为管理员审计日志分页补齐覆盖索引。
-- 冷缓存 large 压测下列表页需要避免按 created_at 排序后大量回表。

CREATE INDEX IF NOT EXISTS idx_audit_logs_created_id_page
    ON audit_logs(created_at DESC, id DESC)
    INCLUDE (actor_email, action, resource_type, result);
