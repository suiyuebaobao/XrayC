-- Reusable subscription rule sets.
-- Rule sets replace direct line_groups.dedicated_rules editing.
-- Existing dedicated rules are migrated into per-group historical rule sets.

CREATE TABLE IF NOT EXISTS subscription_rule_sets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    rules JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT subscription_rule_sets_name_not_empty CHECK (btrim(name) <> ''),
    CONSTRAINT subscription_rule_sets_rules_array CHECK (jsonb_typeof(rules) = 'array')
);

CREATE INDEX IF NOT EXISTS subscription_rule_sets_enabled_name_idx
    ON subscription_rule_sets(enabled, name, id);

CREATE TABLE IF NOT EXISTS line_group_rule_set_bindings (
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    rule_set_id UUID NOT NULL REFERENCES subscription_rule_sets(id) ON DELETE RESTRICT,
    position INTEGER NOT NULL DEFAULT 100,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (line_group_id, rule_set_id)
);

CREATE INDEX IF NOT EXISTS line_group_rule_set_bindings_group_position_idx
    ON line_group_rule_set_bindings(line_group_id, position, rule_set_id);

CREATE INDEX IF NOT EXISTS line_group_rule_set_bindings_rule_set_idx
    ON line_group_rule_set_bindings(rule_set_id);

WITH historical_groups AS (
    SELECT id AS line_group_id,
           name,
           dedicated_rules,
           gen_random_uuid() AS rule_set_id,
           row_number() OVER (ORDER BY sort_weight, name, id) AS rn
    FROM line_groups
    WHERE jsonb_typeof(dedicated_rules) = 'array'
      AND jsonb_array_length(dedicated_rules) > 0
      AND NOT EXISTS (
          SELECT 1
          FROM line_group_rule_set_bindings b
          WHERE b.line_group_id = line_groups.id
      )
),
inserted_rule_sets AS (
    INSERT INTO subscription_rule_sets (id, name, description, enabled, rules)
    SELECT rule_set_id,
           name || '-历史规则',
           '由旧线路分组专用规则自动迁移',
           TRUE,
           dedicated_rules
    FROM historical_groups
    ORDER BY rn
    RETURNING id
)
INSERT INTO line_group_rule_set_bindings (line_group_id, rule_set_id, position, enabled)
SELECT historical_groups.line_group_id,
       historical_groups.rule_set_id,
       100,
       TRUE
FROM historical_groups
JOIN inserted_rule_sets ON inserted_rule_sets.id = historical_groups.rule_set_id
ON CONFLICT (line_group_id, rule_set_id) DO NOTHING;
