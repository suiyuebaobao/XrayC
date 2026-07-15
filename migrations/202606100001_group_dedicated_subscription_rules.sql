-- Add simple two-layer subscription routing controls.
-- Group dedicated rules route matched traffic to that group.
-- Plan default group is used for the final Clash/Mihomo MATCH fallback.

ALTER TABLE line_groups
    ADD COLUMN IF NOT EXISTS dedicated_rules JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE line_groups
    ADD CONSTRAINT line_groups_dedicated_rules_array
    CHECK (jsonb_typeof(dedicated_rules) = 'array') NOT VALID;

ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS default_line_group_id UUID NULL REFERENCES line_groups(id) ON DELETE SET NULL;
