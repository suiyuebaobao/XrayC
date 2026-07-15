ALTER TABLE line_groups
    ADD COLUMN IF NOT EXISTS parent_group_id UUID NULL REFERENCES line_groups(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS group_level TEXT NOT NULL DEFAULT 'secondary',
    ADD COLUMN IF NOT EXISTS sort_weight INTEGER NOT NULL DEFAULT 100,
    ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE line_groups
   SET group_level = 'secondary'
 WHERE group_level NOT IN ('primary', 'secondary');

UPDATE line_groups
   SET parent_group_id = NULL
 WHERE group_level = 'primary';

CREATE INDEX IF NOT EXISTS idx_line_groups_parent_group_id ON line_groups(parent_group_id);
CREATE INDEX IF NOT EXISTS idx_line_groups_level_sort ON line_groups(group_level, sort_weight, name);
CREATE INDEX IF NOT EXISTS idx_line_groups_enabled_level_sort
    ON line_groups(enabled, group_level, sort_weight, name);

DO $$
DECLARE
    default_primary_id UUID;
BEGIN
    SELECT id
      INTO default_primary_id
      FROM line_groups
     WHERE group_level = 'primary'
     ORDER BY sort_weight, name, id
     LIMIT 1;

    IF default_primary_id IS NULL THEN
        INSERT INTO line_groups (name, country_code, icon, group_level, sort_weight, enabled)
        VALUES ('默认一级分组', 'GLOBAL', '🌐', 'primary', 1, TRUE)
        RETURNING id INTO default_primary_id;
    END IF;

    UPDATE line_groups
       SET parent_group_id = default_primary_id,
           group_level = 'secondary'
     WHERE id <> default_primary_id
       AND group_level <> 'primary'
       AND parent_group_id IS NULL;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'line_groups_group_level_check'
    ) THEN
        ALTER TABLE line_groups
            ADD CONSTRAINT line_groups_group_level_check
            CHECK (group_level IN ('primary', 'secondary'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'line_groups_primary_parent_check'
    ) THEN
        ALTER TABLE line_groups
            ADD CONSTRAINT line_groups_primary_parent_check
            CHECK (group_level <> 'primary' OR parent_group_id IS NULL);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'line_groups_no_self_parent_check'
    ) THEN
        ALTER TABLE line_groups
            ADD CONSTRAINT line_groups_no_self_parent_check
            CHECK (parent_group_id IS NULL OR parent_group_id <> id);
    END IF;
END $$;
