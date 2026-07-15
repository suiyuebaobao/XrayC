#!/usr/bin/env bash
# 用途：恢复长稳真实运行时所需的订阅、分组和出口绑定。
# 由 scripts/restore-real-runtime-access-deploy.sh source 使用，不直接执行。

restore_runtime_sync_subscription_assignment() {
  local restore_token_hash restore_assignment_sql
if [[ -n "${SUB_TOKEN:-}" ]]; then
  restore_token_hash="$(sha256_text "$SUB_TOKEN")"
  restore_assignment_sql="$TMP_DIR/restore-assignment.sql"
  cat >"$restore_assignment_sql" <<'SQL'
BEGIN;

CREATE TEMP TABLE xrayc_restore_user AS
SELECT t.user_id
FROM subscription_tokens t
JOIN users u ON u.id = t.user_id
JOIN user_subscriptions s ON s.user_id = t.user_id
WHERE t.token_hash = :'token_hash'
  AND t.revoked_at IS NULL
  AND t.expires_at > now()
  AND u.disabled = FALSE
  AND s.active = TRUE
  AND s.expires_at > now()
LIMIT 1;

CREATE TEMP TABLE xrayc_restore_line AS
SELECT l.id AS access_line_id,
       l.access_node_id,
       l.exit_pool_id,
       l.exit_endpoint_id,
       COALESCE(
         (
           SELECT plg.line_group_id
           FROM xrayc_restore_user u
           JOIN user_subscriptions s ON s.user_id = u.user_id
           JOIN plan_line_groups plg ON plg.plan_id = s.plan_id
           JOIN line_groups lg ON lg.id = plg.line_group_id
           JOIN line_group_exit_endpoints lgee
             ON lgee.line_group_id = plg.line_group_id
            AND lgee.exit_endpoint_id = l.exit_endpoint_id
           WHERE l.exit_endpoint_id IS NOT NULL
             AND lg.enabled = TRUE
           ORDER BY lg.sort_weight, lg.name, plg.line_group_id
           LIMIT 1
         ),
         (
           SELECT plg.line_group_id
           FROM xrayc_restore_user u
           JOIN user_subscriptions s ON s.user_id = u.user_id
           JOIN plan_line_groups plg
             ON plg.plan_id = s.plan_id
            AND plg.line_group_id = l.line_group_id
           JOIN line_groups lg ON lg.id = plg.line_group_id
           WHERE lg.enabled = TRUE
           LIMIT 1
         ),
         (
           SELECT plg.line_group_id
           FROM xrayc_restore_user u
           JOIN user_subscriptions s ON s.user_id = u.user_id
           JOIN plan_line_groups plg ON plg.plan_id = s.plan_id
           JOIN line_groups lg ON lg.id = plg.line_group_id
           WHERE lg.enabled = TRUE
           ORDER BY lg.sort_weight, lg.name, plg.line_group_id
           LIMIT 1
         )
       ) AS line_group_id
FROM access_lines l
WHERE l.id = :'access_line_id'::uuid
  AND l.enabled = TRUE
LIMIT 1;

CREATE TEMP TABLE xrayc_restore_endpoint AS
SELECT e.id AS exit_endpoint_id
FROM exit_endpoints e
JOIN exit_resources r ON r.id = e.exit_resource_id
JOIN xrayc_restore_line l
  ON l.exit_endpoint_id IS NULL
  OR l.exit_endpoint_id = e.id
  OR NOT EXISTS (
    SELECT 1
    FROM exit_endpoints existing
    JOIN exit_resources existing_resource
      ON existing_resource.id = existing.exit_resource_id
    WHERE existing.id = l.exit_endpoint_id
      AND existing.enabled = TRUE
      AND existing_resource.enabled = TRUE
      AND trim(COALESCE(existing.host, '')) <> ''
      AND existing.port IS NOT NULL
      AND existing.port > 0
      AND lower(existing.host) NOT LIKE '%example%'
      AND lower(existing.host) NOT LIKE '%test%'
      AND lower(existing.host) NOT LIKE '%placeholder%'
      AND existing.host !~ '^(192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)'
      AND existing.outbound_type IN ('socks', 'http', 'vless', 'trojan', 'shadowsocks', 'hysteria')
  )
WHERE e.id = :'exit_endpoint_id'::uuid
  AND e.enabled = TRUE
  AND r.enabled = TRUE
LIMIT 1;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM xrayc_restore_user) THEN
    RAISE EXCEPTION 'restore subscription user not found';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM xrayc_restore_line) THEN
    RAISE EXCEPTION 'restore access line not found';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM xrayc_restore_endpoint) THEN
    RAISE EXCEPTION 'restore exit endpoint not found';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM xrayc_restore_line WHERE line_group_id IS NOT NULL) THEN
    RAISE EXCEPTION 'restore authorized line group not found';
  END IF;
END $$;

INSERT INTO exit_pool_members (
  exit_pool_id, exit_endpoint_id, weight, status, priority, allow_new_assignments
)
SELECT l.exit_pool_id, e.exit_endpoint_id, 100, 'healthy', 1000, TRUE
FROM xrayc_restore_line l
CROSS JOIN xrayc_restore_endpoint e
ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
  weight = EXCLUDED.weight,
  status = EXCLUDED.status,
  priority = EXCLUDED.priority,
  allow_new_assignments = EXCLUDED.allow_new_assignments;

INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
SELECT l.line_group_id, e.exit_endpoint_id
FROM xrayc_restore_line l
CROSS JOIN xrayc_restore_endpoint e
WHERE l.line_group_id IS NOT NULL
  AND l.exit_endpoint_id IS NULL
ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING;

DELETE FROM user_access_line_assignments ula
USING xrayc_restore_user u, xrayc_restore_line l
WHERE ula.user_id = u.user_id
  AND ula.line_group_id = l.line_group_id;

INSERT INTO user_access_line_assignments (
  user_id, line_group_id, access_line_id
)
SELECT u.user_id, l.line_group_id, l.access_line_id
FROM xrayc_restore_user u
CROSS JOIN xrayc_restore_line l
WHERE l.line_group_id IS NOT NULL
ON CONFLICT (user_id, line_group_id, access_line_id) DO NOTHING;

DELETE FROM user_exit_assignments uea
USING xrayc_restore_user u, xrayc_restore_line l
WHERE uea.user_id = u.user_id
  AND uea.access_line_id = l.access_line_id;

INSERT INTO user_exit_assignments (
  user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
SELECT u.user_id, l.access_line_id, l.exit_pool_id, e.exit_endpoint_id, 'restore_real_runtime_access_deploy'
FROM xrayc_restore_user u
CROSS JOIN xrayc_restore_line l
CROSS JOIN xrayc_restore_endpoint e;

UPDATE access_nodes n
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'restore_real_runtime_access_deploy'
FROM xrayc_restore_line l
WHERE n.id = l.access_node_id;

COMMIT;
SQL
  run_psql "$restore_assignment_sql" \
    -v "token_hash=$restore_token_hash" \
    -v "access_line_id=$ACCESS_LINE_ID" \
    -v "exit_endpoint_id=$restore_exit_endpoint_id"
fi
}
