ALTER TABLE line_groups DROP CONSTRAINT IF EXISTS line_groups_secondary_parent_check;
ALTER TABLE line_groups DROP CONSTRAINT IF EXISTS line_groups_primary_parent_check;
ALTER TABLE line_groups DROP CONSTRAINT IF EXISTS line_groups_group_level_check;

UPDATE line_groups
   SET group_level = 'group',
       parent_group_id = NULL
 WHERE group_level <> 'group'
    OR parent_group_id IS NOT NULL;

ALTER TABLE line_groups
    ALTER COLUMN group_level SET DEFAULT 'group';

ALTER TABLE line_groups
    ADD CONSTRAINT line_groups_group_level_check
    CHECK (group_level = 'group');
