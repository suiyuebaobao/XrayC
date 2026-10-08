-- 迁移前的历史记录冻结保留；新增记录仍按原有保留策略处理。
-- PostgreSQL 的缺失列默认值保留在旧行上，随后只改变新插入行的默认值，不改金额/时间/原内容。
ALTER TABLE usage_ledgers ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE usage_ledgers ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE access_line_metric_snapshots ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE access_line_metric_snapshots ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE node_runtime_metrics ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE node_runtime_metrics ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE access_user_sessions ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE access_user_sessions ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE access_user_session_events ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE access_user_session_events ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE access_line_probes ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE access_line_probes ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
ALTER TABLE access_exit_probes ADD COLUMN preserve_on_cleanup BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE access_exit_probes ALTER COLUMN preserve_on_cleanup SET DEFAULT FALSE;
