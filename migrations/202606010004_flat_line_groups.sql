-- 扁平化线路分组：分组直接包含线路，不再拆一级/二级，也不做粘性或负载策略。
-- 旧字段继续保留兼容，所有当前分组按 secondary 存储，parent_group_id 清空。
ALTER TABLE line_groups DROP CONSTRAINT IF EXISTS line_groups_secondary_parent_check;

UPDATE plans SET visible_line_count = NULL;

UPDATE plan_line_groups
   SET visible_line_count = NULL,
       selection_strategy = 'all';

UPDATE exit_pools
   SET strategy = 'priority'
 WHERE strategy = 'weighted_sticky';

INSERT INTO plan_line_groups (
    plan_id, line_group_id, visible_line_count, selection_strategy
)
SELECT DISTINCT plg.plan_id, child.id, NULL::INTEGER, 'all'
  FROM plan_line_groups plg
  JOIN line_groups parent ON parent.id = plg.line_group_id
  JOIN line_groups child ON child.parent_group_id = parent.id
 WHERE parent.group_level = 'primary'
   AND child.group_level = 'secondary'
   AND child.enabled = TRUE
ON CONFLICT (plan_id, line_group_id) DO UPDATE SET
    visible_line_count = NULL::INTEGER,
    selection_strategy = 'all';

DELETE FROM plan_line_groups plg
USING line_groups lg
WHERE plg.line_group_id = lg.id
  AND lg.group_level = 'primary';

DELETE FROM line_groups lg
WHERE lg.group_level = 'primary'
  AND NOT EXISTS (
      SELECT 1 FROM line_group_lines lgl WHERE lgl.line_group_id = lg.id
  );

UPDATE line_groups
   SET group_level = 'secondary',
       parent_group_id = NULL
 WHERE group_level <> 'secondary'
    OR parent_group_id IS NOT NULL;

DELETE FROM line_group_lines lgl
USING line_groups lg, access_lines al
WHERE lgl.line_group_id = lg.id
  AND lgl.access_line_id = al.id
  AND lg.name <> al.name
  AND (
      SELECT COUNT(*)
      FROM line_group_lines existing
      WHERE existing.line_group_id = lg.id
  ) > 1
  AND EXISTS (
      SELECT 1
      FROM line_groups peer
      JOIN line_group_lines peer_lgl ON peer_lgl.line_group_id = peer.id
      WHERE peer.id <> lg.id
        AND peer_lgl.access_line_id = al.id
  );

DO $$
DECLARE
    default_group_id UUID;
    direct_group_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO direct_group_count
    FROM line_groups lg
    WHERE lg.enabled = TRUE
      AND EXISTS (
          SELECT 1 FROM line_group_lines lgl WHERE lgl.line_group_id = lg.id
      );

    IF direct_group_count = 0 THEN
        SELECT id INTO default_group_id
        FROM line_groups
        WHERE group_level = 'secondary'
          AND parent_group_id IS NULL
          AND name = '默认分组'
        ORDER BY sort_weight, created_at, name
        LIMIT 1;

        IF default_group_id IS NULL THEN
            INSERT INTO line_groups (
                name, country_code, icon, group_level, parent_group_id, sort_weight, enabled
            )
            VALUES ('默认分组', 'GLOBAL', '🌐', 'secondary', NULL, 100, TRUE)
            RETURNING id INTO default_group_id;
        END IF;

        INSERT INTO line_group_lines (line_group_id, access_line_id)
        SELECT default_group_id, al.id
        FROM access_lines al
        WHERE al.enabled = TRUE
        ON CONFLICT (line_group_id, access_line_id) DO NOTHING;
    END IF;
END $$;

INSERT INTO plan_line_groups (
    plan_id, line_group_id, visible_line_count, selection_strategy
)
SELECT p.id, lg.id, NULL::INTEGER, 'all'
FROM plans p
CROSS JOIN line_groups lg
WHERE p.is_default = TRUE
  AND p.is_deleted = FALSE
  AND lg.enabled = TRUE
  AND EXISTS (
      SELECT 1 FROM line_group_lines lgl WHERE lgl.line_group_id = lg.id
  )
ON CONFLICT (plan_id, line_group_id) DO UPDATE SET
    visible_line_count = NULL::INTEGER,
    selection_strategy = 'all';

DELETE FROM user_access_line_assignments;
DELETE FROM user_exit_assignments;

UPDATE access_nodes
   SET config_dirty = TRUE,
       config_dirty_at = now(),
       config_dirty_reason = 'flat_line_groups'
 WHERE config_dirty = FALSE
    OR config_dirty_reason IS DISTINCT FROM 'flat_line_groups';
