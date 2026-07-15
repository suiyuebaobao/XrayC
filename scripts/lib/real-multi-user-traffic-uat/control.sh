#!/usr/bin/env bash
# 真实多用户流量 UAT 的控制流 helper。
# shellcheck shell=bash

run_multi_user_rounds_if_requested() {
  require_positive_int XRAYC_REAL_MULTI_USER_ROUNDS "$ROUNDS"
  if [[ "${XRAYC_REAL_MULTI_USER_ROUND_ACTIVE:-0}" == "1" || "$ROUNDS" == "1" ]]; then
    return
  fi
  local round
  for round in $(seq 1 "$ROUNDS"); do
    echo "real_multi_user_traffic_uat: round=${round}/${ROUNDS} starting"
    XRAYC_REAL_MULTI_USER_ROUND_ACTIVE=1 \
      XRAYC_REAL_MULTI_USER_ROUND_INDEX="$round" \
      XRAYC_REAL_MULTI_USER_ROUNDS=1 \
      bash "$0"
    echo "real_multi_user_traffic_uat: round=${round}/${ROUNDS} passed"
    if [[ "$round" -lt "$ROUNDS" ]]; then
      local settle_seconds="${XRAYC_REAL_MULTI_USER_ROUND_SETTLE_SECONDS:-90}"
      require_positive_int XRAYC_REAL_MULTI_USER_ROUND_SETTLE_SECONDS "$settle_seconds"
      echo "real_multi_user_traffic_uat: round_settle_seconds=${settle_seconds}"
      sleep "$settle_seconds"
    fi
  done
  exit 0
}

verify_inventory_contains_default_aliases() {
  python3 - "$XRAYC_REAL_E2E_INVENTORY" "$CLIENT_ALIAS" "$TRANSIT_ALIAS" <<'PY'
import json
import sys

inventory, client_alias, transit_alias = sys.argv[1:4]
with open(inventory, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
if len(targets) < 3:
    raise SystemExit("inventory must include at least three server_N aliases")
aliases = {str(item.get("alias", "")) for item in targets}
missing = [alias for alias in (client_alias, transit_alias, "server_3") if alias not in aliases]
if missing:
    raise SystemExit("inventory is missing required server_N aliases")
PY
  echo "real_multi_user_traffic_uat: inventory_aliases=ok"
}

load_inventory_target() {
  local alias="$1"
  local prefix="$2"
  python3 - "$XRAYC_REAL_E2E_INVENTORY" "$alias" "$prefix" <<'PY'
import json
import shlex
import sys

path, alias, prefix = sys.argv[1:4]
with open(path, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
target = None
for item in targets:
    if str(item.get("alias", "")) == alias:
        target = item
        break
if target is None:
    raise SystemExit("inventory target alias not found")
values = {
    f"{prefix}_SSH_HOST": target.get("ssh_host", ""),
    f"{prefix}_SSH_USER": target.get("ssh_user", "root"),
    f"{prefix}_SSH_PORT": str(target.get("ssh_port", 22)),
    f"{prefix}_SSH_PASSWORD_FILE": target.get("ssh_password_file", ""),
    f"{prefix}_PUBLIC_HOST": target.get("public_host") or target.get("public_domain") or target.get("ssh_host", ""),
}
for key, value in values.items():
    if not str(value).strip():
        raise SystemExit(f"inventory target missing {key}")
    print(f"{key}={shlex.quote(str(value))}")
PY
}

load_inventory_target_by_ssh_host() {
  local ssh_host="$1"
  local prefix="$2"
  python3 - "$XRAYC_REAL_E2E_INVENTORY" "$ssh_host" "$prefix" <<'PY'
import json
import shlex
import sys

path, ssh_host, prefix = sys.argv[1:4]
with open(path, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
target = None
for item in targets:
    candidate_hosts = {
        str(item.get("ssh_host", "")).strip(),
        str(item.get("public_host", "")).strip(),
        str(item.get("public_domain", "")).strip(),
    }
    if ssh_host in candidate_hosts:
        target = item
        break
if target is None:
    raise SystemExit("inventory target ssh_host not found")
values = {
    f"{prefix}_SSH_HOST": target.get("ssh_host", ""),
    f"{prefix}_SSH_USER": target.get("ssh_user", "root"),
    f"{prefix}_SSH_PORT": str(target.get("ssh_port", 22)),
    f"{prefix}_SSH_PASSWORD_FILE": target.get("ssh_password_file", ""),
    f"{prefix}_PUBLIC_HOST": target.get("public_host") or target.get("public_domain") or target.get("ssh_host", ""),
}
for key, value in values.items():
    if not str(value).strip():
        raise SystemExit(f"inventory target missing {key}")
    print(f"{key}={shlex.quote(str(value))}")
PY
}

inventory_target_host_csv() {
  python3 - "$XRAYC_REAL_E2E_INVENTORY" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
hosts = []
seen = set()
for item in targets:
    for key in ("ssh_host", "public_host", "public_domain"):
        value = str(item.get(key, "")).strip()
        if value and value not in seen:
            hosts.append(value)
            seen.add(value)
print(",".join(hosts))
PY
}

assert_remote_traffic_results() {
  local results_file="$1"
  local expected_count="$2"
  local allowed_failures="$3"
  python3 - "$results_file" "$expected_count" "$allowed_failures" <<'PY'
import re
import sys

path, expected_count, allowed_failures = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
pattern = re.compile(
    r"^user_(?P<idx>\d+) "
    r"download_ok=(?P<download_ok>\d+) "
    r"download_fail=(?P<download_fail>\d+) "
    r"upload_ok=(?P<upload_ok>\d+) "
    r"upload_fail=(?P<upload_fail>\d+) "
    r"seconds=(?P<seconds>\d+)$"
)
seen = {}
for raw in open(path, encoding="utf-8"):
    match = pattern.match(raw.strip())
    if match:
        seen[int(match.group("idx"))] = {key: int(value) for key, value in match.groupdict().items() if key != "idx"}
if len(seen) != expected_count:
    raise SystemExit("remote traffic result count mismatch")
for idx, item in sorted(seen.items()):
    if item["download_ok"] <= 0 or item["upload_ok"] <= 0:
        raise SystemExit(f"remote traffic user {idx} has no successful download/upload sample")
    if item["download_fail"] > allowed_failures or item["upload_fail"] > allowed_failures:
        raise SystemExit(f"remote traffic user {idx} exceeded allowed failures")
print("remote_traffic_results=ok")
PY
  }

assert_multi_user_cleanup_closed() {
  local cleanup_tsv="$TMP_DIR/cleanup-assert.tsv"
  run_psql -XAtq -F $'\t' <<SQL >"$cleanup_tsv"
CREATE TEMP TABLE xrayc_multi_users (
  idx integer,
  user_id uuid,
  email text,
  xray_user_key text,
  access_credential text,
  token text,
  token_hash text
);
\\copy xrayc_multi_users FROM '${USERS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');
SELECT
  (SELECT COUNT(*) FROM users u JOIN xrayc_multi_users x ON x.user_id = u.id WHERE u.disabled = FALSE)::text AS active_user_count,
  (SELECT COUNT(*) FROM subscription_tokens st JOIN xrayc_multi_users x ON x.user_id = st.user_id WHERE st.revoked_at IS NULL AND st.expires_at > now())::text AS subscription_token_active_count,
  (SELECT COUNT(*) FROM user_access_line_assignments a JOIN xrayc_multi_users x ON x.user_id = a.user_id)::text AS user_access_assignment_count,
  (SELECT COUNT(*) FROM user_exit_assignments e JOIN xrayc_multi_users x ON x.user_id = e.user_id)::text AS user_exit_assignment_count;
SQL
  local active_user_count subscription_token_active_count user_access_assignment_count user_exit_assignment_count
  IFS=$'\t' read -r active_user_count subscription_token_active_count user_access_assignment_count user_exit_assignment_count <"$cleanup_tsv"
  echo "real_multi_user_traffic_uat: cleanup active_user_count=${active_user_count} subscription_token_active_count=${subscription_token_active_count} user_access_assignment_count=${user_access_assignment_count} user_exit_assignment_count=${user_exit_assignment_count}"
  if [[ "$active_user_count" != "0" || "$subscription_token_active_count" != "0" || "$user_access_assignment_count" != "0" || "$user_exit_assignment_count" != "0" ]]; then
    die "multi-user cleanup db rows were not cleared"
  fi

  local remote_client_container_count
  remote_client_container_count="$(ssh_client "docker ps -aq --filter 'name=xrayc-multi-user-client-${RUN_ID}-' 2>/dev/null | wc -l | tr -d '[:space:]'")"
  echo "real_multi_user_traffic_uat: cleanup remote_client_container_count=${remote_client_container_count}"
  [[ "$remote_client_container_count" == "0" ]] || die "multi-user cleanup remote clients were not cleared"
}

cleanup_db_users() {
  if [[ "$db_users_prepared" != "1" || "$db_users_cleaned" == "1" || ! -s "${USERS_TSV:-}" ]]; then
    return
  fi
  run_psql -Xq -v access_node_id="${ACCESS_NODE_ID:-00000000-0000-0000-0000-000000000000}" <<SQL
CREATE TEMP TABLE xrayc_multi_users (
  idx integer,
  user_id uuid,
  email text,
  xray_user_key text,
  access_credential text,
  token text,
  token_hash text
);
\\copy xrayc_multi_users FROM '${USERS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');
UPDATE subscription_tokens st
    SET revoked_at = now()
    FROM xrayc_multi_users u
    WHERE st.user_id = u.user_id;
    DELETE FROM user_subscriptions
    WHERE user_id IN (SELECT user_id FROM xrayc_multi_users);
    UPDATE users
    SET disabled = TRUE,
        updated_at = now()
WHERE id IN (SELECT user_id FROM xrayc_multi_users);
DELETE FROM user_access_line_assignments
WHERE user_id IN (SELECT user_id FROM xrayc_multi_users);
DELETE FROM user_exit_assignments
WHERE user_id IN (SELECT user_id FROM xrayc_multi_users);
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    applied_config_hash = NULL,
    config_dirty_reason = 'real_multi_user_traffic_uat_cleanup'
WHERE id = :'access_node_id'::uuid;
SQL
  db_users_cleaned=1
}

cleanup_temp_plan() {
  if [[ "$temp_plan_cleaned" == "1" ]]; then
    return
  fi
  if [[ -z "${TEMP_PLAN_ID:-}" && -z "${TEMP_LINE_GROUP_ID:-}" ]]; then
    return
  fi
  if [[ -n "${TEMP_PLAN_ID:-}" ]]; then
    run_psql -Xq -v plan_id="$TEMP_PLAN_ID" <<'SQL'
DELETE FROM plan_line_groups WHERE plan_id = :'plan_id'::uuid;
UPDATE plans
SET enabled = FALSE,
    is_deleted = TRUE,
    name = name || '-deleted'
WHERE id = :'plan_id'::uuid
  AND name LIKE 'real-multi-user-traffic-%';
SQL
  fi
  if [[ -n "${TEMP_LINE_GROUP_ID:-}" ]]; then
    run_psql -Xq -v line_group_id="$TEMP_LINE_GROUP_ID" <<'SQL'
DELETE FROM plan_line_groups WHERE line_group_id = :'line_group_id'::uuid;
DELETE FROM line_group_binding_nodes WHERE line_group_id = :'line_group_id'::uuid;
DELETE FROM line_group_exit_endpoints WHERE line_group_id = :'line_group_id'::uuid;
DELETE FROM line_group_rule_set_bindings WHERE line_group_id = :'line_group_id'::uuid;
DELETE FROM line_groups
WHERE id = :'line_group_id'::uuid
  AND name LIKE 'real-multi-user-traffic-%';
SQL
  fi
  temp_plan_cleaned=1
  echo "real_multi_user_traffic_uat_cleanup_plan: ok"
}

restore_exit_pool_strategy() {
  if [[ -z "${ORIGINAL_EXIT_POOL_STRATEGY:-}" || -z "${EXIT_POOL_ID:-}" || "$exit_pool_strategy_restored" == "1" ]]; then
    return
  fi
  run_psql -Xq \
    -v exit_pool_id="$EXIT_POOL_ID" \
    -v strategy="$ORIGINAL_EXIT_POOL_STRATEGY" \
    -v access_node_id="${ACCESS_NODE_ID:-00000000-0000-0000-0000-000000000000}" <<'SQL'
UPDATE exit_pools
SET strategy = :'strategy',
    updated_at = now()
WHERE id = :'exit_pool_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    applied_config_hash = NULL,
    config_dirty_reason = 'real_multi_user_traffic_uat_restore_strategy'
WHERE id = :'access_node_id'::uuid;
SQL
  exit_pool_strategy_restored=1
}

reapply_explicit_exit_assignments() {
  run_psql -Xq \
    -v users_file="$USERS_TSV" \
    -v endpoints_file="$ENDPOINTS_TSV" \
    -v access_line_id="$ACCESS_LINE_ID" \
    -v exit_pool_id="$EXIT_POOL_ID" \
    -v access_node_id="$ACCESS_NODE_ID" <<SQL
CREATE TEMP TABLE xrayc_multi_users (
  idx integer,
  user_id uuid,
  email text,
  xray_user_key text,
  access_credential text,
  token text,
  token_hash text
);
\\copy xrayc_multi_users FROM '${USERS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');
CREATE TEMP TABLE xrayc_multi_exit_endpoints (
  idx integer,
  exit_endpoint_id uuid
);
\\copy xrayc_multi_exit_endpoints FROM '${ENDPOINTS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');

DELETE FROM user_exit_assignments
WHERE user_id IN (SELECT user_id FROM xrayc_multi_users)
  AND access_line_id = :'access_line_id'::uuid
  AND exit_pool_id = :'exit_pool_id'::uuid;

INSERT INTO user_exit_assignments (
  user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
SELECT users.user_id, :'access_line_id'::uuid, :'exit_pool_id'::uuid,
       endpoints.exit_endpoint_id, 'real_multi_user_traffic_uat'
FROM xrayc_multi_users users
JOIN xrayc_multi_exit_endpoints endpoints
  ON endpoints.idx = ((users.idx - 1) % (SELECT COUNT(*) FROM xrayc_multi_exit_endpoints)) + 1;

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    applied_config_hash = NULL,
    config_dirty_reason = 'real_multi_user_traffic_uat'
WHERE id = :'access_node_id'::uuid;
SQL
  echo "real_multi_user_traffic_uat: explicit_exit_assignments=ok"
}

load_multi_user_exit_endpoints() {
	  ENDPOINTS_TSV="$TMP_DIR/endpoints.tsv"
	  run_psql -XAtq -F $'\t' \
	    -v exit_pool_id="$EXIT_POOL_ID" \
	    -v access_line_id="$ACCESS_LINE_ID" \
	    -v access_node_id="$ACCESS_NODE_ID" <<SQL >"$ENDPOINTS_TSV"
	SELECT row_number() OVER (ORDER BY epm.priority, epm.exit_endpoint_id)::text AS idx,
	       epm.exit_endpoint_id::text
	FROM exit_pool_members epm
	JOIN access_lines l ON l.exit_pool_id = epm.exit_pool_id
	JOIN exit_endpoints e ON e.id = epm.exit_endpoint_id
	JOIN exit_resources r ON r.id = e.exit_resource_id
	LEFT JOIN access_exit_probe_states s
	  ON s.access_node_id = :'access_node_id'::uuid
	 AND s.exit_endpoint_id = e.id
	WHERE epm.exit_pool_id = :'exit_pool_id'::uuid
	  AND l.id = :'access_line_id'::uuid
	  AND l.exit_endpoint_id = epm.exit_endpoint_id
	  AND epm.status = 'healthy'
  AND epm.allow_new_assignments = TRUE
  AND e.enabled = TRUE
  AND r.enabled = TRUE
  AND lower(COALESCE(NULLIF(trim(r.status), ''), 'healthy')) <> 'offline'
  AND COALESCE(s.effective_status, '') = 'healthy'
ORDER BY epm.priority, epm.exit_endpoint_id;
SQL
  ENDPOINT_COUNT="$(wc -l <"$ENDPOINTS_TSV" | tr -d '[:space:]')"
  [[ "$ENDPOINT_COUNT" =~ ^[1-9][0-9]*$ ]] || die "active exit endpoints are missing"
  if [[ "${XRAYC_REAL_MULTI_USER_REQUIRE_MULTI_EXIT:-0}" == "1" && "$ENDPOINT_COUNT" -lt 2 ]]; then
    die "multi-user UAT requires at least two active exit endpoints"
  fi
  echo "real_multi_user_traffic_uat: exit_endpoint_count=${ENDPOINT_COUNT}"
}

ensure_priority_exit_pool_strategy() {
  ORIGINAL_EXIT_POOL_STRATEGY="$(run_psql -XAtq -v exit_pool_id="$EXIT_POOL_ID" <<'SQL'
SELECT strategy FROM exit_pools WHERE id = :'exit_pool_id'::uuid LIMIT 1;
SQL
)"
  [[ -n "$ORIGINAL_EXIT_POOL_STRATEGY" ]] || die "exit pool strategy is missing"
  if [[ "$ORIGINAL_EXIT_POOL_STRATEGY" != "priority" ]]; then
    run_psql -Xq \
      -v exit_pool_id="$EXIT_POOL_ID" \
      -v access_node_id="$ACCESS_NODE_ID" <<'SQL'
UPDATE exit_pools
SET strategy = 'priority',
    updated_at = now()
WHERE id = :'exit_pool_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    applied_config_hash = NULL,
    config_dirty_reason = 'real_multi_user_traffic_uat_priority_strategy'
WHERE id = :'access_node_id'::uuid;
SQL
  fi
  echo "real_multi_user_traffic_uat: exit_pool_strategy=priority"
}

prepare_multi_user_db_users() {
  local password_hash
  USERS_TSV="$TMP_DIR/users.tsv"
  password_hash="$(run_psql -XAtq -F $'\t' <<'SQL'
SELECT password_hash FROM users WHERE email = 'demo@example.test' LIMIT 1;
SQL
)"
  [[ -n "$password_hash" ]] || die "demo password hash is missing"

  USER_COUNT="$USER_COUNT" RUN_ID_VALUE="$RUN_ID" python3 - "$USERS_TSV" <<'PY'
import hashlib
import os
import secrets
import sys
import uuid

count = int(os.environ["USER_COUNT"])
run_id = os.environ["RUN_ID_VALUE"].replace("-", "")
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    for idx in range(1, count + 1):
        user_id = str(uuid.uuid4())
        token = "multi-" + secrets.token_urlsafe(30)
        token_hash = hashlib.sha256(token.encode()).hexdigest()
        credential = str(uuid.uuid4())
        xray_key = f"multi-{run_id}-{idx}@xrayc.local"
        email = f"xrayc-multi-{run_id}-{idx}@example.test"
        fh.write("\t".join([str(idx), user_id, email, xray_key, credential, token, token_hash]) + "\n")
PY

  run_psql -Xq \
    -v users_file="$USERS_TSV" \
    -v endpoints_file="$ENDPOINTS_TSV" \
    -v password_hash="$password_hash" \
    -v plan_id="$PLAN_ID" \
    -v line_group_id="$LINE_GROUP_ID" \
    -v access_line_id="$ACCESS_LINE_ID" \
    -v exit_pool_id="$EXIT_POOL_ID" \
    -v access_node_id="$ACCESS_NODE_ID" \
    -v user_rate_limit_bps="$USER_RATE_LIMIT_BPS" <<SQL
CREATE TEMP TABLE xrayc_multi_users (
  idx integer,
  user_id uuid,
  email text,
  xray_user_key text,
  access_credential text,
  token text,
  token_hash text
);
\\copy xrayc_multi_users FROM '${USERS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');
CREATE TEMP TABLE xrayc_multi_exit_endpoints (
  idx integer,
  exit_endpoint_id uuid
);
\\copy xrayc_multi_exit_endpoints FROM '${ENDPOINTS_TSV}' WITH (FORMAT csv, DELIMITER E'\\t');

INSERT INTO users (
  id, email, password_hash, xray_user_key, access_credential, disabled,
  is_admin, display_name, rate_limit_bps
)
SELECT user_id, email, :'password_hash', xray_user_key, access_credential,
       FALSE, FALSE, 'Multi user traffic UAT', :'user_rate_limit_bps'::bigint
FROM xrayc_multi_users
ON CONFLICT (email) DO UPDATE SET
  disabled = FALSE,
  xray_user_key = EXCLUDED.xray_user_key,
  access_credential = EXCLUDED.access_credential,
  rate_limit_bps = EXCLUDED.rate_limit_bps,
  updated_at = now();

INSERT INTO user_subscriptions (user_id, plan_id, active, expires_at, used_bytes, limit_bytes)
SELECT u.user_id, :'plan_id'::uuid, TRUE, now() + interval '30 days', 0,
       COALESCE((SELECT traffic_limit_bytes FROM plans WHERE id = :'plan_id'::uuid), 0)
FROM xrayc_multi_users u
ON CONFLICT (user_id) DO UPDATE SET
  plan_id = EXCLUDED.plan_id,
  active = TRUE,
  expires_at = EXCLUDED.expires_at,
  updated_at = now();

INSERT INTO subscription_tokens (token, token_hash, user_id, expires_at, revoked_at)
SELECT token, token_hash, user_id, now() + interval '30 days', NULL
FROM xrayc_multi_users
ON CONFLICT (user_id) DO UPDATE SET
  token = EXCLUDED.token,
  token_hash = EXCLUDED.token_hash,
  expires_at = EXCLUDED.expires_at,
  revoked_at = NULL,
  last_used_at = NULL;

INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id)
SELECT user_id, :'line_group_id'::uuid, :'access_line_id'::uuid
FROM xrayc_multi_users
ON CONFLICT (user_id, line_group_id, access_line_id) DO NOTHING;

INSERT INTO user_exit_assignments (
  user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
SELECT user_id, :'access_line_id'::uuid, :'exit_pool_id'::uuid,
       endpoints.exit_endpoint_id, 'real_multi_user_traffic_uat'
FROM xrayc_multi_users
JOIN xrayc_multi_exit_endpoints endpoints
  ON endpoints.idx = ((xrayc_multi_users.idx - 1) % (SELECT COUNT(*) FROM xrayc_multi_exit_endpoints)) + 1
ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
  exit_endpoint_id = EXCLUDED.exit_endpoint_id,
  failover_reason = EXCLUDED.failover_reason,
  assigned_at = now();

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    applied_config_hash = NULL,
    config_dirty_reason = 'real_multi_user_traffic_uat'
WHERE id = :'access_node_id'::uuid;
SQL
  db_users_prepared=1
}
