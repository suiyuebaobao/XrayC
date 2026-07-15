#!/usr/bin/env bash

select_multi_user_access_line() {
  LINE_SELECTION_TSV="$TMP_DIR/line-selection.tsv"
  INVENTORY_ACCESS_HOSTS="$(inventory_target_host_csv)"
  [[ -n "$INVENTORY_ACCESS_HOSTS" ]] || die "real inventory has no usable access hosts"

  run_psql -XAtq -F $'\t' \
    -v requested_access_line_id="${XRAYC_REAL_MULTI_USER_ACCESS_LINE_ID:-}" \
    -v preferred_transit_host="${TRANSIT_SSH_HOST:-}" \
    -v preferred_transit_public_host="${TRANSIT_PUBLIC_HOST:-${TRANSIT_SSH_HOST:-}}" \
    -v client_host="${CLIENT_SSH_HOST:-}" \
    -v inventory_access_hosts="$INVENTORY_ACCESS_HOSTS" <<'SQL' >"$LINE_SELECTION_TSV"
WITH line_candidates AS (
  SELECT l.id,
         l.access_node_id,
         n.public_host AS access_node_public_host,
         l.line_group_id,
         l.exit_endpoint_id,
         l.exit_pool_id,
         l.listen_port,
         lower(l.protocol) AS protocol,
         (
           SELECT COUNT(*)
           FROM exit_pool_members m
           JOIN exit_endpoints bound_endpoint ON bound_endpoint.id = m.exit_endpoint_id
           JOIN exit_resources bound_endpoint_resource ON bound_endpoint_resource.id = bound_endpoint.exit_resource_id
           LEFT JOIN access_exit_probe_states bound_probe
             ON bound_probe.access_node_id = l.access_node_id
            AND bound_probe.exit_endpoint_id = bound_endpoint.id
           WHERE m.exit_pool_id = l.exit_pool_id
             AND m.exit_endpoint_id = l.exit_endpoint_id
             AND m.status = 'healthy'
             AND m.allow_new_assignments = TRUE
             AND bound_endpoint.enabled = TRUE
             AND bound_endpoint_resource.enabled = TRUE
             AND lower(COALESCE(NULLIF(trim(bound_endpoint_resource.status), ''), 'healthy')) <> 'offline'
             AND COALESCE(bound_probe.effective_status, '') = 'healthy'
         ) AS active_exit_endpoint_count
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_endpoints bound_endpoint ON bound_endpoint.id = l.exit_endpoint_id
  JOIN exit_resources bound_endpoint_resource ON bound_endpoint_resource.id = bound_endpoint.exit_resource_id
  LEFT JOIN access_exit_probe_states bound_probe
    ON bound_probe.access_node_id = l.access_node_id
   AND bound_probe.exit_endpoint_id = bound_endpoint.id
  WHERE l.enabled = TRUE
    AND lower(l.protocol) IN ('vless', 'trojan', 'shadowsocks', 'ss')
    AND lower(COALESCE(NULLIF(trim(l.transport), ''), 'tcp')) = 'tcp'
    AND bound_endpoint.enabled = TRUE
    AND bound_endpoint_resource.enabled = TRUE
    AND lower(COALESCE(NULLIF(trim(bound_endpoint_resource.status), ''), 'healthy')) <> 'offline'
    AND COALESCE(bound_probe.effective_status, '') = 'healthy'
    AND lower(COALESCE(NULLIF(trim(n.status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
    AND n.public_host = ANY(string_to_array(:'inventory_access_hosts', ','))
    AND n.config_dirty = FALSE
    AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
    AND NULLIF(trim(COALESCE(n.desired_config_hash, '')), '') IS NOT NULL
    AND n.applied_config_hash = n.desired_config_hash
    AND n.last_heartbeat_at >= now() - interval '60 seconds'
    AND EXISTS (
      SELECT 1
      FROM access_entry_exit_bindings b
      WHERE b.id = l.id
        AND b.enabled = TRUE
    )
    AND (
      NULLIF(:'requested_access_line_id', '')::uuid IS NULL
      OR l.id = NULLIF(:'requested_access_line_id', '')::uuid
    )
),
line AS (
  SELECT *
  FROM line_candidates
  ORDER BY
    CASE
      WHEN access_node_public_host = :'preferred_transit_host' THEN 0
      WHEN access_node_public_host = :'preferred_transit_public_host' THEN 0
      WHEN access_node_public_host <> :'client_host' THEN 1
      ELSE 2
    END,
    CASE WHEN listen_port >= 1024 THEN 0 ELSE 1 END,
    active_exit_endpoint_count DESC,
    listen_port DESC,
    id
  LIMIT 1
),
plan AS (
  SELECT p.id
  FROM plans p
  WHERE p.enabled = TRUE AND p.is_deleted = FALSE
  ORDER BY p.is_default DESC, p.created_at ASC
  LIMIT 1
)
SELECT line.id::text, line.access_node_id::text,
       COALESCE(line.line_group_id, '00000000-0000-0000-0000-000000000000'::uuid)::text,
       line.exit_pool_id::text, line.listen_port::text, line.protocol,
       plan.id::text, line.access_node_public_host
FROM line, plan;
SQL

  IFS=$'\t' read -r ACCESS_LINE_ID ACCESS_NODE_ID LINE_GROUP_ID EXIT_POOL_ID LISTEN_PORT PROTOCOL PLAN_ID ACCESS_NODE_PUBLIC_HOST <"$LINE_SELECTION_TSV" || true
  [[ -n "${ACCESS_LINE_ID:-}" && -n "${PLAN_ID:-}" ]] \
    || die "active access line or plan is missing"
}
