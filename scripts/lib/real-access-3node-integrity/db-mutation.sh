#!/usr/bin/env bash
# 此 helper 提供真实三节点完整性 E2E 的数据库上下文发现与临时状态变更。
# 它由主脚本 source，不直接执行，并负责在异常退出时配合 cleanup 恢复账户状态。

db_query() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq "$@"
}

db_query_single() {
  local output
  output="$(db_query "$@")"
  printf '%s\n' "$output" | sed -n '1p'
}

discover_access_line_id_if_needed() {
  if [[ -n "$ACCESS_LINE_ID" ]]; then
    return
  fi
  local server
  if ! server="$(first_expected_access_server)"; then
    die_usage "ACCESS_LINE_ID is required when EXPECTED_ACCESS_SERVERS contains more than one relay"
  fi
  ACCESS_LINE_ID="$(db_query_single -v "expected_access_server=$server" <<'SQL'
SELECT id::text
FROM access_lines
WHERE concat(listen_host, ':', listen_port::text) = :'expected_access_server'
ORDER BY id::text
LIMIT 1;
SQL
)"
  [[ -n "$ACCESS_LINE_ID" ]] || fail "could not discover ACCESS_LINE_ID from EXPECTED_ACCESS_SERVERS"
}

discover_db_context() {
  local context_file="$tmp_dir/context.env"
  local sub_token_hash="$1"

  db_query \
    -v "sub_token_hash=$sub_token_hash" \
    -v "access_line_id=$ACCESS_LINE_ID" >"$context_file" <<'SQL'
WITH target_user AS (
    SELECT u.id::text AS user_id,
           u.xray_user_key,
           u.disabled::text AS user_disabled,
           s.active::text AS subscription_active
    FROM subscription_tokens t
    JOIN users u ON u.id = t.user_id
    JOIN user_subscriptions s ON s.user_id = u.id
    WHERE t.token_hash = :'sub_token_hash'
      AND t.revoked_at IS NULL
      AND t.expires_at > now()
    LIMIT 1
), target_line AS (
    SELECT l.id::text AS access_line_id,
           l.access_node_id::text AS access_node_id,
           l.exit_pool_id::text AS exit_pool_id,
           concat(l.listen_host, ':', l.listen_port::text) AS access_server,
           l.enabled::text AS line_enabled
    FROM access_lines l
    WHERE l.id = :'access_line_id'::uuid
    LIMIT 1
), assignment AS (
    SELECT uea.exit_endpoint_id::text AS assigned_exit_endpoint_id,
           uea.failover_reason,
           e.outbound_type::text AS assigned_outbound_type
    FROM target_user tu
    CROSS JOIN target_line tl
    JOIN user_exit_assignments uea
      ON uea.user_id = tu.user_id::uuid
     AND uea.access_line_id = tl.access_line_id::uuid
     AND uea.exit_pool_id = tl.exit_pool_id::uuid
    JOIN exit_endpoints e ON e.id = uea.exit_endpoint_id
), available_members AS (
    SELECT count(DISTINCT m.exit_endpoint_id)::text AS count_value
    FROM target_line tl
    JOIN exit_pool_members m ON m.exit_pool_id = tl.exit_pool_id::uuid
    JOIN exit_pools p ON p.id = m.exit_pool_id
    JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
    JOIN exit_resources r ON r.id = e.exit_resource_id
    LEFT JOIN access_exit_probe_states s
      ON s.access_node_id = tl.access_node_id::uuid
     AND s.exit_endpoint_id = e.id
    WHERE p.enabled = TRUE
      AND m.status = 'healthy'
      AND m.allow_new_assignments = TRUE
      AND COALESCE(s.effective_status, 'healthy') <> 'offline'
      AND e.enabled = TRUE
      AND r.enabled = TRUE
      AND e.outbound_type <> 'direct'
      AND trim(e.host) <> ''
      AND e.port > 0
)
SELECT 'TARGET_USER_ID=' || user_id FROM target_user
UNION ALL SELECT 'TARGET_XRAY_USER_KEY=' || xray_user_key FROM target_user
UNION ALL SELECT 'TARGET_USER_DISABLED=' || user_disabled FROM target_user
UNION ALL SELECT 'TARGET_SUBSCRIPTION_ACTIVE=' || subscription_active FROM target_user
UNION ALL SELECT 'TARGET_ACCESS_LINE_ID=' || access_line_id FROM target_line
UNION ALL SELECT 'TARGET_ACCESS_NODE_ID=' || access_node_id FROM target_line
UNION ALL SELECT 'TARGET_EXIT_POOL_ID=' || exit_pool_id FROM target_line
UNION ALL SELECT 'TARGET_ACCESS_SERVER=' || access_server FROM target_line
UNION ALL SELECT 'TARGET_LINE_ENABLED=' || line_enabled FROM target_line
UNION ALL SELECT 'ASSIGNED_EXIT_ENDPOINT_ID=' || assigned_exit_endpoint_id FROM assignment
UNION ALL SELECT 'ASSIGNED_EXIT_FAILOVER_REASON=' || failover_reason FROM assignment
UNION ALL SELECT 'ASSIGNED_OUTBOUND_TYPE=' || assigned_outbound_type FROM assignment
UNION ALL SELECT 'AVAILABLE_EXIT_MEMBER_COUNT=' || count_value FROM available_members;
SQL

  # shellcheck disable=SC1090
  . "$context_file"
  TARGET_USER_ID="${TARGET_USER_ID:-}"
  TARGET_XRAY_USER_KEY="${TARGET_XRAY_USER_KEY:-}"
  TARGET_ACCESS_LINE_ID="${TARGET_ACCESS_LINE_ID:-}"
  TARGET_ACCESS_NODE_ID="${TARGET_ACCESS_NODE_ID:-}"
  TARGET_EXIT_POOL_ID="${TARGET_EXIT_POOL_ID:-}"
  TARGET_ACCESS_SERVER="${TARGET_ACCESS_SERVER:-}"
  TARGET_LINE_ENABLED="${TARGET_LINE_ENABLED:-}"
  ASSIGNED_EXIT_ENDPOINT_ID="${ASSIGNED_EXIT_ENDPOINT_ID:-}"
  ASSIGNED_EXIT_FAILOVER_REASON="${ASSIGNED_EXIT_FAILOVER_REASON:-}"
  ASSIGNED_OUTBOUND_TYPE="${ASSIGNED_OUTBOUND_TYPE:-}"
  AVAILABLE_EXIT_MEMBER_COUNT="${AVAILABLE_EXIT_MEMBER_COUNT:-0}"

  [[ -n "$TARGET_USER_ID" ]] || fail "subscription token did not map to an active user in the database"
  [[ "$TARGET_USER_DISABLED" == "false" ]] || fail "target user is already disabled"
  [[ "$TARGET_SUBSCRIPTION_ACTIVE" == "true" ]] || fail "target subscription is not active"
  [[ -n "$TARGET_ACCESS_LINE_ID" && "$TARGET_LINE_ENABLED" == "true" ]] || fail "target access line is missing or disabled"
  [[ -n "$ASSIGNED_EXIT_ENDPOINT_ID" ]] || fail "user has no exit assignment for the target access line"
  [[ "$ASSIGNED_OUTBOUND_TYPE" != "direct" ]] || fail "target exit assignment uses legacy direct endpoint"
  [[ "$AVAILABLE_EXIT_MEMBER_COUNT" =~ ^[0-9]+$ && "$AVAILABLE_EXIT_MEMBER_COUNT" -ge "$MIN_EXIT_MEMBERS" ]] || fail "target exit pool has fewer assignable members than required"
  if [[ -n "$EXIT_ENDPOINT_ID" && "$EXIT_ENDPOINT_ID" != "$ASSIGNED_EXIT_ENDPOINT_ID" ]]; then
    fail "database exit assignment differs from EXIT_ENDPOINT_ID"
  fi
  ACCESS_LINE_ID="$TARGET_ACCESS_LINE_ID"
  ACCESS_NODE_ID="${ACCESS_NODE_ID:-$TARGET_ACCESS_NODE_ID}"
  EXIT_ENDPOINT_ID="$ASSIGNED_EXIT_ENDPOINT_ID"
  XRAY_USER_KEY="${XRAY_USER_KEY:-$TARGET_XRAY_USER_KEY}"
  TARGET_STATS_EMAIL="xrayc-line-${TARGET_ACCESS_LINE_ID}--${XRAY_USER_KEY}"

  EXIT_POOL_HOSTS_FILE="$tmp_dir/exit-pool-hosts.txt"
  db_query -v "exit_pool_id=$TARGET_EXIT_POOL_ID" >"$EXIT_POOL_HOSTS_FILE" <<'SQL'
SELECT DISTINCT e.host
FROM exit_pool_members m
JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
WHERE m.exit_pool_id = :'exit_pool_id'::uuid
  AND trim(e.host) <> '';
SQL
}

capture_mutation_state() {
  local state_file="$tmp_dir/original-state.tsv"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -F $'\t' \
    -v "user_id=$TARGET_USER_ID" \
    -v "access_line_id=$TARGET_ACCESS_LINE_ID" \
    -v "exit_pool_id=$TARGET_EXIT_POOL_ID" >"$state_file" <<'SQL'
SELECT u.disabled::text,
       s.active::text,
       s.expires_at::text,
       s.used_bytes::text,
       s.limit_bytes::text,
       COALESCE(uea.exit_endpoint_id::text, ''),
       COALESCE(uea.failover_reason, '')
FROM users u
JOIN user_subscriptions s ON s.user_id = u.id
LEFT JOIN user_exit_assignments uea
  ON uea.user_id = u.id
 AND uea.access_line_id = :'access_line_id'::uuid
 AND uea.exit_pool_id = :'exit_pool_id'::uuid
WHERE u.id = :'user_id'::uuid
LIMIT 1;
SQL
  IFS=$'\t' read -r \
    ORIGINAL_USER_DISABLED \
    ORIGINAL_SUBSCRIPTION_ACTIVE \
    ORIGINAL_SUBSCRIPTION_EXPIRES_AT \
    ORIGINAL_USED_BYTES \
    ORIGINAL_LIMIT_BYTES \
    ORIGINAL_ASSIGNMENT_EXIT_ENDPOINT_ID \
    ORIGINAL_ASSIGNMENT_FAILOVER_REASON <"$state_file"
  [[ -n "$ORIGINAL_USER_DISABLED" ]] || fail "failed to capture original account state"
  MUTATION_STATE_CAPTURED=1
}

restore_account_state() {
  [[ "$MUTATION_STATE_CAPTURED" == "1" ]] || return 0
  db_query \
    -v "user_id=$TARGET_USER_ID" \
    -v "access_line_id=$TARGET_ACCESS_LINE_ID" \
    -v "access_node_id=$TARGET_ACCESS_NODE_ID" \
    -v "exit_pool_id=$TARGET_EXIT_POOL_ID" \
    -v "user_disabled=$ORIGINAL_USER_DISABLED" \
    -v "subscription_active=$ORIGINAL_SUBSCRIPTION_ACTIVE" \
    -v "expires_at=$ORIGINAL_SUBSCRIPTION_EXPIRES_AT" \
    -v "used_bytes=$ORIGINAL_USED_BYTES" \
    -v "limit_bytes=$ORIGINAL_LIMIT_BYTES" \
    -v "assignment_exit_endpoint_id=$ORIGINAL_ASSIGNMENT_EXIT_ENDPOINT_ID" \
    -v "assignment_failover_reason=$ORIGINAL_ASSIGNMENT_FAILOVER_REASON" <<'SQL'
BEGIN;
UPDATE users
SET disabled = :'user_disabled'::boolean
WHERE id = :'user_id'::uuid;

UPDATE user_subscriptions
SET active = :'subscription_active'::boolean,
    expires_at = :'expires_at'::timestamptz,
    used_bytes = :'used_bytes'::bigint,
    limit_bytes = :'limit_bytes'::bigint,
    updated_at = now()
WHERE user_id = :'user_id'::uuid;

INSERT INTO user_exit_assignments (
    user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
SELECT :'user_id'::uuid,
       :'access_line_id'::uuid,
       :'exit_pool_id'::uuid,
       :'assignment_exit_endpoint_id'::uuid,
       :'assignment_failover_reason'
WHERE :'assignment_exit_endpoint_id' <> ''
ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
    failover_reason = EXCLUDED.failover_reason,
    assigned_at = now();

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_3node_integrity_restore'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
  RESTORE_ON_EXIT=0
}

mark_restore_needed() {
  RESTORE_ON_EXIT=1
}

mutate_user_disabled() {
  mark_restore_needed
  db_query \
    -v "user_id=$TARGET_USER_ID" \
    -v "access_node_id=$TARGET_ACCESS_NODE_ID" <<'SQL'
BEGIN;
UPDATE users
SET disabled = TRUE
WHERE id = :'user_id'::uuid;

DELETE FROM user_access_line_assignments
WHERE user_id = :'user_id'::uuid;

DELETE FROM user_exit_assignments
WHERE user_id = :'user_id'::uuid;

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_3node_integrity_user_disabled'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

mutate_subscription_quota_exhausted() {
  mark_restore_needed
  local updated=""
  updated="$(db_query_single -v "user_id=$TARGET_USER_ID" -v "access_node_id=$TARGET_ACCESS_NODE_ID" <<'SQL'
WITH changed AS (
    UPDATE user_subscriptions
    SET used_bytes = limit_bytes,
        updated_at = now()
    WHERE user_id = :'user_id'::uuid
      AND limit_bytes > 0
    RETURNING limit_bytes
), removed_access AS (
    DELETE FROM user_access_line_assignments
    WHERE user_id = :'user_id'::uuid
      AND EXISTS (SELECT 1 FROM changed)
    RETURNING user_id
), removed_exit AS (
    DELETE FROM user_exit_assignments
    WHERE user_id = :'user_id'::uuid
      AND EXISTS (SELECT 1 FROM changed)
    RETURNING user_id
), dirty AS (
    UPDATE access_nodes
    SET config_dirty = TRUE,
        config_dirty_at = now(),
        desired_config_hash = NULL,
        config_dirty_reason = 'real_3node_integrity_quota_exhausted'
    WHERE id = :'access_node_id'::uuid
      AND EXISTS (SELECT 1 FROM changed)
    RETURNING id
)
SELECT count(*)::text FROM changed;
SQL
)"
  [[ "$updated" == "1" ]] || fail "target subscription has no positive traffic limit"
}
