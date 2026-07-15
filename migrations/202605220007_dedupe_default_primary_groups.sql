-- 去重迁移与演示种子叠加产生的默认一级分组。
-- 只合并名称为“默认一级分组”的系统默认组，不处理管理员自定义业务专区。
-- 子二级分组和套餐授权会迁移到保留组，避免用户订阅范围丢失。

DO $$
DECLARE
    keeper_id UUID;
BEGIN
    SELECT id
      INTO keeper_id
      FROM line_groups
     WHERE group_level = 'primary'
       AND name = '默认一级分组'
     ORDER BY CASE WHEN id = '00000000-0000-0000-0000-000000000600'::uuid THEN 0 ELSE 1 END,
              sort_weight, id
     LIMIT 1;

    IF keeper_id IS NULL THEN
        RETURN;
    END IF;

    UPDATE line_groups
       SET parent_group_id = keeper_id
     WHERE group_level = 'secondary'
       AND parent_group_id IN (
           SELECT id
             FROM line_groups
            WHERE group_level = 'primary'
              AND name = '默认一级分组'
              AND id <> keeper_id
       );

    INSERT INTO plan_line_groups (
        plan_id, line_group_id, visible_line_count, selection_strategy
    )
    SELECT DISTINCT plg.plan_id, keeper_id, NULL::integer, 'all'
      FROM plan_line_groups plg
      JOIN line_groups lg ON lg.id = plg.line_group_id
     WHERE lg.group_level = 'primary'
       AND lg.name = '默认一级分组'
       AND lg.id <> keeper_id
    ON CONFLICT (plan_id, line_group_id) DO NOTHING;

    DELETE FROM plan_line_groups plg
    USING line_groups lg
    WHERE plg.line_group_id = lg.id
      AND lg.group_level = 'primary'
      AND lg.name = '默认一级分组'
      AND lg.id <> keeper_id;

    DELETE FROM line_groups
     WHERE group_level = 'primary'
       AND name = '默认一级分组'
       AND id <> keeper_id;
END $$;
