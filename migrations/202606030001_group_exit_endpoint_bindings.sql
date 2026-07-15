-- Line groups now contain exit endpoints only. Access lines are runtime inbounds
-- generated when a group is bound to an access node.

ALTER TABLE line_groups
    ADD COLUMN IF NOT EXISTS exit_pool_id UUID NULL REFERENCES exit_pools(id) ON DELETE SET NULL;

ALTER TABLE access_lines
    ADD COLUMN IF NOT EXISTS line_group_id UUID NULL REFERENCES line_groups(id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS line_group_exit_endpoints (
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (line_group_id, exit_endpoint_id)
);

CREATE INDEX IF NOT EXISTS idx_access_lines_line_group_id
    ON access_lines(line_group_id);

CREATE INDEX IF NOT EXISTS idx_line_group_exit_endpoints_endpoint_id
    ON line_group_exit_endpoints(exit_endpoint_id);

INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
SELECT DISTINCT lgl.line_group_id, m.exit_endpoint_id
FROM line_group_lines lgl
JOIN access_lines al ON al.id = lgl.access_line_id
JOIN exit_pool_members m ON m.exit_pool_id = al.exit_pool_id
JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
JOIN exit_resources r ON r.id = e.exit_resource_id
WHERE e.enabled = TRUE
  AND r.enabled = TRUE
ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING;

WITH ranked_bindings AS (
    SELECT lgl.access_line_id,
           lgl.line_group_id,
           ROW_NUMBER() OVER (
               PARTITION BY lgl.access_line_id
               ORDER BY lg.sort_weight, lg.name, lg.id
           ) AS binding_rank
    FROM line_group_lines lgl
    JOIN line_groups lg ON lg.id = lgl.line_group_id
)
UPDATE access_lines al
SET line_group_id = ranked_bindings.line_group_id
FROM ranked_bindings
WHERE ranked_bindings.access_line_id = al.id
  AND ranked_bindings.binding_rank = 1
  AND al.line_group_id IS NULL;

DO $$
DECLARE
    group_row RECORD;
    group_exit_pool_id UUID;
BEGIN
    FOR group_row IN
        SELECT lg.id,
               lg.name,
               lg.exit_pool_id,
               COALESCE(NULLIF(lg.country_code, ''), 'GLOBAL') AS region_code
        FROM line_groups lg
        WHERE EXISTS (
            SELECT 1
            FROM line_group_exit_endpoints lgee
            WHERE lgee.line_group_id = lg.id
        )
        ORDER BY lg.sort_weight, lg.name, lg.id
    LOOP
        group_exit_pool_id := group_row.exit_pool_id;

        IF group_exit_pool_id IS NULL
           OR NOT EXISTS (SELECT 1 FROM exit_pools WHERE id = group_exit_pool_id) THEN
            INSERT INTO exit_pools (name, region_code, strategy, enabled, updated_at)
            VALUES (
                LEFT('分组出口：' || group_row.name, 128),
                group_row.region_code,
                'priority',
                TRUE,
                now()
            )
            RETURNING id INTO group_exit_pool_id;

            UPDATE line_groups
            SET exit_pool_id = group_exit_pool_id
            WHERE id = group_row.id;
        END IF;

        DELETE FROM exit_pool_members m
        WHERE m.exit_pool_id = group_exit_pool_id
          AND NOT EXISTS (
              SELECT 1
              FROM line_group_exit_endpoints lgee
              WHERE lgee.line_group_id = group_row.id
                AND lgee.exit_endpoint_id = m.exit_endpoint_id
          );

        INSERT INTO exit_pool_members (
            exit_pool_id, exit_endpoint_id, weight, status,
            priority, allow_new_assignments, updated_at
        )
        SELECT group_exit_pool_id, lgee.exit_endpoint_id, 100, 'healthy',
               100, TRUE, now()
        FROM line_group_exit_endpoints lgee
        WHERE lgee.line_group_id = group_row.id
        ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
            weight = EXCLUDED.weight,
            status = EXCLUDED.status,
            priority = EXCLUDED.priority,
            allow_new_assignments = EXCLUDED.allow_new_assignments,
            updated_at = now();

        UPDATE access_lines
        SET exit_pool_id = group_exit_pool_id
        WHERE line_group_id = group_row.id;
    END LOOP;
END $$;

DELETE FROM user_access_line_assignments;
DELETE FROM user_exit_assignments;

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'group_exit_endpoint_bindings'
WHERE config_dirty = FALSE
   OR config_dirty_reason IS DISTINCT FROM 'group_exit_endpoint_bindings';
