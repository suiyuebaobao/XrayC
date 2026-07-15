-- 补齐后台套餐 CRUD 需要的软删除过滤和线路组授权策略字段。
-- selection_strategy 目前作为后端最小兼容字段保存，订阅生成仍沿用可见数量逻辑。

ALTER TABLE plan_line_groups
    ADD COLUMN IF NOT EXISTS selection_strategy TEXT NOT NULL DEFAULT 'all';

UPDATE plan_line_groups
SET selection_strategy = 'all'
WHERE selection_strategy = '';

CREATE INDEX IF NOT EXISTS idx_plans_visible_admin
    ON plans(is_deleted, enabled, sort_weight, created_at);

CREATE INDEX IF NOT EXISTS idx_user_subscriptions_plan_active
    ON user_subscriptions(plan_id, active);
