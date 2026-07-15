-- Access node entries now bind a concrete exit endpoint from the line pool.
-- line_group_id stays nullable for historical rows; exit_endpoint_id is the
-- authoritative field for new access line entry creation.

ALTER TABLE access_lines
    ADD COLUMN IF NOT EXISTS exit_endpoint_id UUID NULL REFERENCES exit_endpoints(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_access_lines_exit_endpoint_id
    ON access_lines(exit_endpoint_id);

WITH ranked_endpoint AS (
    SELECT al.id AS access_line_id,
           lgee.exit_endpoint_id,
           ROW_NUMBER() OVER (
               PARTITION BY al.id
               ORDER BY lg.sort_weight, lg.name, lg.id, lgee.created_at, lgee.exit_endpoint_id
           ) AS endpoint_rank
    FROM access_lines al
    JOIN line_group_exit_endpoints lgee ON lgee.line_group_id = al.line_group_id
    JOIN line_groups lg ON lg.id = lgee.line_group_id
    JOIN exit_endpoints e ON e.id = lgee.exit_endpoint_id
    JOIN exit_resources r ON r.id = e.exit_resource_id
    WHERE al.exit_endpoint_id IS NULL
      AND e.enabled = TRUE
      AND r.enabled = TRUE
)
UPDATE access_lines al
SET exit_endpoint_id = ranked_endpoint.exit_endpoint_id
FROM ranked_endpoint
WHERE ranked_endpoint.access_line_id = al.id
  AND ranked_endpoint.endpoint_rank = 1;

DELETE FROM user_access_line_assignments;
DELETE FROM user_exit_assignments;

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'access_lines_exit_endpoint'
WHERE config_dirty = FALSE
   OR config_dirty_reason IS DISTINCT FROM 'access_lines_exit_endpoint';
