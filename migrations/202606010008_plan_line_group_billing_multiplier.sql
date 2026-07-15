ALTER TABLE plan_line_groups
    ADD COLUMN IF NOT EXISTS billing_multiplier NUMERIC(10, 3);

UPDATE plan_line_groups plg
SET billing_multiplier = GREATEST(COALESCE(lg.billing_multiplier, 1.000), 1.000)
FROM line_groups lg
WHERE lg.id = plg.line_group_id
  AND (
      plg.billing_multiplier IS NULL
      OR plg.billing_multiplier <= 0
      OR (plg.billing_multiplier = 1.000 AND lg.billing_multiplier <> 1.000)
  );

UPDATE plan_line_groups
SET billing_multiplier = 1.000
WHERE billing_multiplier IS NULL OR billing_multiplier <= 0;

ALTER TABLE plan_line_groups
    ALTER COLUMN billing_multiplier SET DEFAULT 1.000,
    ALTER COLUMN billing_multiplier SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'plan_line_groups_billing_multiplier_positive'
    ) THEN
        ALTER TABLE plan_line_groups
            ADD CONSTRAINT plan_line_groups_billing_multiplier_positive
            CHECK (billing_multiplier > 0);
    END IF;
END $$;
