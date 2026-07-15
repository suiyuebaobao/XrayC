-- 用户级限速从「统一速率」扩展为「上行/下行可分别限速」（用户 2026-06-30 定）。
-- 单位 bps。在已有 rate_limit_bps（对称默认）基础上新增方向覆盖列，向后兼容：
--   两列均为 NULL 时，该用户/套餐上下行都沿用 rate_limit_bps（即旧的对称行为，0=不限速）。
--   某方向列非 NULL 时，该方向以此列为准（0 表示该方向不限速）。
-- 生效解析（见 core::effective_user_rate_limit_{up,down}_bps）：
--   up   = coalesce(user.up,   plan.up,   user.bps, plan.bps)
--   down = coalesce(user.down, plan.down, user.bps, plan.bps)

ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS rate_limit_up_bps BIGINT NULL,
    ADD COLUMN IF NOT EXISTS rate_limit_down_bps BIGINT NULL;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS rate_limit_up_bps BIGINT NULL,
    ADD COLUMN IF NOT EXISTS rate_limit_down_bps BIGINT NULL;

ALTER TABLE plans
    ADD CONSTRAINT plans_rate_limit_up_bps_non_negative
    CHECK (rate_limit_up_bps IS NULL OR rate_limit_up_bps >= 0) NOT VALID;
ALTER TABLE plans
    ADD CONSTRAINT plans_rate_limit_down_bps_non_negative
    CHECK (rate_limit_down_bps IS NULL OR rate_limit_down_bps >= 0) NOT VALID;

ALTER TABLE users
    ADD CONSTRAINT users_rate_limit_up_bps_non_negative
    CHECK (rate_limit_up_bps IS NULL OR rate_limit_up_bps >= 0) NOT VALID;
ALTER TABLE users
    ADD CONSTRAINT users_rate_limit_down_bps_non_negative
    CHECK (rate_limit_down_bps IS NULL OR rate_limit_down_bps >= 0) NOT VALID;
