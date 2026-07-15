#!/usr/bin/env bash

create_multi_user_temp_plan() {
  local source_plan_id="$PLAN_ID"
  local temp_ids
  temp_ids="$(run_psql -XAtq -F $'\t' \
    -v source_plan_id="$source_plan_id" \
    -v access_line_id="$ACCESS_LINE_ID" \
    -v run_id="$RUN_ID" <<'SQL'
WITH source_line AS (
  SELECT l.id, l.exit_endpoint_id
  FROM access_lines l
  JOIN access_entry_exit_bindings b ON b.id = l.id
  WHERE l.id = :'access_line_id'::uuid
    AND l.enabled = TRUE
    AND b.enabled = TRUE
  LIMIT 1
),
created_group AS (
  INSERT INTO line_groups (
    name, sort_weight, enabled, billing_multiplier
  )
  SELECT 'real-multi-user-traffic-' || :'run_id',
         -1000000,
         TRUE,
         1.000
  FROM source_line
  RETURNING id
),
bound_binding_node AS (
  INSERT INTO line_group_binding_nodes (
    line_group_id, entry_exit_binding_id, position
  )
  SELECT created_group.id, source_line.id, 100
  FROM created_group
  CROSS JOIN source_line
  RETURNING line_group_id
),
bound_exit_endpoint AS (
  INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
  SELECT created_group.id, source_line.exit_endpoint_id
  FROM created_group
  CROSS JOIN source_line
  WHERE source_line.exit_endpoint_id IS NOT NULL
  ON CONFLICT DO NOTHING
),
created_plan AS (
  INSERT INTO plans (
    name, is_default, traffic_limit_bytes, rate_limit_bps,
    billing_multiplier, enabled,
    price_cents, currency, duration_days, sort_weight, is_deleted
  )
  SELECT 'real-multi-user-traffic-' || :'run_id',
         FALSE,
         traffic_limit_bytes,
         rate_limit_bps,
         billing_multiplier,
         TRUE,
         price_cents,
         currency,
         duration_days,
         1000000,
         FALSE
  FROM plans
  CROSS JOIN bound_binding_node
  WHERE id = :'source_plan_id'::uuid
  RETURNING id
),
bound_group AS (
  INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
  SELECT created_plan.id, created_group.id, 1
  FROM created_plan
  CROSS JOIN created_group
)
SELECT created_plan.id::text, created_group.id::text
FROM created_plan
CROSS JOIN created_group;
SQL
)"
  IFS=$'\t' read -r TEMP_PLAN_ID TEMP_LINE_GROUP_ID <<<"$temp_ids"
  [[ -n "$TEMP_PLAN_ID" && -n "$TEMP_LINE_GROUP_ID" ]] \
    || die "temporary multi-user plan or line group was not created"
  PLAN_ID="$TEMP_PLAN_ID"
  LINE_GROUP_ID="$TEMP_LINE_GROUP_ID"
  echo "real_multi_user_traffic_uat: temp_plan_created"
}
