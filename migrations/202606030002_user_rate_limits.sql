-- Reintroduce user-level bandwidth limiting as a new transit-node limiter model.
-- Values are bits per second. A plan value of 0 means unlimited.
-- A user NULL value inherits the plan; a user value of 0 overrides to unlimited.

ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS rate_limit_bps BIGINT NOT NULL DEFAULT 0;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS rate_limit_bps BIGINT NULL;

ALTER TABLE plans
    ADD CONSTRAINT plans_rate_limit_bps_non_negative
    CHECK (rate_limit_bps >= 0) NOT VALID;

ALTER TABLE users
    ADD CONSTRAINT users_rate_limit_bps_non_negative
    CHECK (rate_limit_bps IS NULL OR rate_limit_bps >= 0) NOT VALID;
