-- 用户级流量日志按 recorded_at/collected_at 混合时间排序，提供表达式索引避免大用户慢分页。
CREATE INDEX IF NOT EXISTS idx_usage_ledgers_user_observed_page
    ON usage_ledgers(user_id, (COALESCE(recorded_at, collected_at)) DESC, id DESC);

-- session events 需要按保留周期清理，单独索引 observed_at 避免维护任务全表扫描。
CREATE INDEX IF NOT EXISTS idx_access_user_session_events_observed_at
    ON access_user_session_events(observed_at);
