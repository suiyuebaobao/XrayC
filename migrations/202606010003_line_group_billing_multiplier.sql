ALTER TABLE line_groups
    ADD COLUMN IF NOT EXISTS billing_multiplier NUMERIC(10, 3) NOT NULL DEFAULT 1.000;

UPDATE line_groups
SET billing_multiplier = 1.000
WHERE billing_multiplier IS NULL OR billing_multiplier <= 0;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'line_groups_billing_multiplier_positive'
    ) THEN
        ALTER TABLE line_groups
            ADD CONSTRAINT line_groups_billing_multiplier_positive
            CHECK (billing_multiplier > 0);
    END IF;
END $$;
