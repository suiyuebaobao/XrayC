-- 将历史套餐授权统一迁移到一级业务分组，避免新模型继续写入二级分组。
-- 二级分组没有父级时统一挂到默认一级分组，后续运行时不再允许孤儿二级。
-- 当前迁移不删除二级分组和线路成员，只迁移套餐授权关系。

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
       SET parent_group_id = default_primary_id
     WHERE group_level = 'secondary'
       AND parent_group_id IS NULL;
END $$;

INSERT INTO plan_line_groups (
    plan_id, line_group_id, visible_line_count, selection_strategy
)
SELECT DISTINCT ON (plg.plan_id, lg.parent_group_id)
       plg.plan_id,
       lg.parent_group_id,
       NULL,
       'all'
  FROM plan_line_groups plg
  JOIN line_groups lg ON lg.id = plg.line_group_id
 WHERE lg.group_level = 'secondary'
   AND lg.parent_group_id IS NOT NULL
 ORDER BY plg.plan_id, lg.parent_group_id
ON CONFLICT (plan_id, line_group_id) DO NOTHING;

DELETE FROM plan_line_groups plg
USING line_groups lg
WHERE plg.line_group_id = lg.id
  AND lg.group_level <> 'primary';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'line_groups_secondary_parent_check'
    ) THEN
        ALTER TABLE line_groups
            ADD CONSTRAINT line_groups_secondary_parent_check
            CHECK (group_level <> 'secondary' OR parent_group_id IS NOT NULL);
    END IF;
END $$;
