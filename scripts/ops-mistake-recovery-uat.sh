#!/usr/bin/env bash
# 该脚本执行运维误操作恢复 UAT 主流程。
# 辅助函数拆分到 scripts/lib/ops-mistake-recovery-uat/ 以控制行数。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=lib/ops-mistake-recovery-uat/common.sh
source "${SCRIPT_DIR}/lib/ops-mistake-recovery-uat/common.sh"

usage() {
  cat <<'USAGE'
usage: RUN_OPS_MISTAKE_RECOVERY_UAT=1 bash scripts/ops-mistake-recovery-uat.sh

Runs a real operator mistake recovery UAT against temporary control-plane data.
Loads .env.real-release or XRAYC_REAL_RELEASE_ENV_FILE.

Required:
  BASE_URL, DATABASE_URL, RUN_OPS_MISTAKE_RECOVERY_UAT=1
  ADMIN_ACCESS_TOKEN or ADMIN_LOGIN_ACCOUNT + ADMIN_LOGIN_PASSWORD.
  E2E_ADMIN_ACCOUNT/E2E_ADMIN_PASSWORD are used as fallback login credentials.
Optional: OPS_MISTAKE_RECOVERY_UAT_KEEP_DATA=1, OPS_MISTAKE_RECOVERY_UAT_VALIDATE_ONLY=1
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

BASE_URL="${BASE_URL:-}"
DATABASE_URL="${DATABASE_URL:-}"
ADMIN_ACCESS_TOKEN="${ADMIN_ACCESS_TOKEN:-}"
ADMIN_LOGIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}"
ADMIN_LOGIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}"
xrayc_real_e2e_require_url_scheme BASE_URL "ops mistake recovery UAT"
xrayc_real_e2e_require_url_scheme DATABASE_URL "ops mistake recovery UAT"
if [[ -z "$ADMIN_ACCESS_TOKEN" && ( -z "$ADMIN_LOGIN_ACCOUNT" || -z "$ADMIN_LOGIN_PASSWORD" ) ]]; then
  echo "ADMIN_ACCESS_TOKEN or ADMIN_LOGIN_ACCOUNT + ADMIN_LOGIN_PASSWORD is required for ops mistake recovery UAT" >&2
  exit 2
fi

if xrayc_real_e2e_bool_is_true "${OPS_MISTAKE_RECOVERY_UAT_VALIDATE_ONLY:-0}"; then
  echo "ops_mistake_recovery_uat: validation_passed"
  exit 0
fi
if ! xrayc_real_e2e_bool_is_true "${RUN_OPS_MISTAKE_RECOVERY_UAT:-0}"; then
  echo "RUN_OPS_MISTAKE_RECOVERY_UAT=1 is required for ops mistake recovery UAT" >&2
  exit 2
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for ops mistake recovery UAT" >&2
  exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for ops mistake recovery UAT" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
run_id="$(python3 - <<'PY'
import secrets
print(secrets.token_hex(6))
PY
)"
sub_token="$(python3 - <<'PY'
import secrets
print("uat-" + secrets.token_urlsafe(32))
PY
)"

main_user_id="$(uuid)"
delete_user_id="$(uuid)"
plan_id="$(uuid)"
delete_plan_id="$(uuid)"
node_id="$(uuid)"
pool_id="$(uuid)"
resource_id="$(uuid)"
primary_endpoint_id="$(uuid)"
backup_endpoint_id="$(uuid)"
line_id="$(uuid)"
group_id="$(uuid)"
line_name="ops-recovery-line-${run_id}"
pool_name="ops-recovery-pool-${run_id}"
line_host="ops-recovery-${run_id}.invalid"
primary_host="198.51.100.21"
backup_host="198.51.100.22"
listen_port="$(RUN_ID="$run_id" python3 - <<'PY'
import os
print(43000 + (int(os.environ["RUN_ID"][:2], 16) % 1000))
PY
)"

trap cleanup EXIT

default_plan_id="$(psql_scalar <<'SQL'
SELECT COALESCE((SELECT id::text FROM plans WHERE is_default = TRUE AND enabled = TRUE AND is_deleted = FALSE ORDER BY created_at ASC LIMIT 1), '');
SQL
)"
[[ -n "$default_plan_id" ]] || { echo "enabled default plan is required" >&2; exit 2; }
derive_admin_token_if_needed

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v run_id="$run_id" -v sub_token="$sub_token" \
  -v main_user_id="$main_user_id" -v delete_user_id="$delete_user_id" \
  -v plan_id="$plan_id" -v delete_plan_id="$delete_plan_id" -v default_plan_id="$default_plan_id" \
  -v node_id="$node_id" -v pool_id="$pool_id" -v resource_id="$resource_id" \
  -v primary_endpoint_id="$primary_endpoint_id" -v backup_endpoint_id="$backup_endpoint_id" \
  -v line_id="$line_id" -v group_id="$group_id" \
  -v line_name="$line_name" -v pool_name="$pool_name" -v line_host="$line_host" \
  -v primary_host="$primary_host" -v backup_host="$backup_host" -v listen_port="$listen_port" <<'SQL'
BEGIN;
INSERT INTO users (id, email, password_hash, xray_user_key, disabled)
VALUES
  (:'main_user_id'::uuid, 'ops-recovery-' || :'run_id' || '@example.invalid', 'uat-login-disabled', 'uat-main-' || :'run_id', FALSE),
  (:'delete_user_id'::uuid, 'ops-recovery-delete-' || :'run_id' || '@example.invalid', 'uat-login-disabled', 'uat-delete-' || :'run_id', FALSE);
INSERT INTO plans (id, name, is_default, traffic_limit_bytes, billing_multiplier, enabled, price_cents, currency, duration_days, sort_weight, is_deleted)
VALUES
  (:'plan_id'::uuid, 'ops-recovery-plan-' || :'run_id', FALSE, 10737418240, 1, TRUE, 0, 'USDT', 30, 900001, FALSE),
  (:'delete_plan_id'::uuid, 'ops-recovery-delete-plan-' || :'run_id', FALSE, 10737418240, 1, TRUE, 0, 'USDT', 30, 900002, FALSE);
INSERT INTO user_subscriptions (user_id, plan_id, active, expires_at, used_bytes, limit_bytes)
VALUES
  (:'main_user_id'::uuid, :'plan_id'::uuid, TRUE, now() + interval '30 days', 0, 10737418240),
  (:'delete_user_id'::uuid, :'delete_plan_id'::uuid, TRUE, now() + interval '30 days', 0, 10737418240);
INSERT INTO subscription_tokens (token, user_id, token_hash, expires_at)
VALUES (:'sub_token', :'main_user_id'::uuid, encode(digest(:'sub_token', 'sha256'), 'hex'), now() + interval '30 days');
INSERT INTO access_nodes (id, name, public_host, agent_token_hash, config_dirty, config_dirty_at, config_dirty_reason, status)
VALUES (:'node_id'::uuid, 'ops-recovery-node-' || :'run_id', 'ops-recovery-node.invalid', encode(digest('ops-agent-' || :'run_id', 'sha256'), 'hex'), TRUE, now(), 'ops_mistake_recovery_uat_setup', 'online');
INSERT INTO exit_pools (id, name, region_code, strategy, enabled)
VALUES (:'pool_id'::uuid, :'pool_name', 'UAT', 'priority', TRUE);
INSERT INTO exit_resources (id, name, region_code, provider_name, ownership, status, enabled)
VALUES (:'resource_id'::uuid, 'ops-recovery-resource-' || :'run_id', 'UAT', 'ops-uat', 'third_party', 'healthy', TRUE);
INSERT INTO exit_endpoints (id, exit_resource_id, name, outbound_type, host, port, outbound_config, stream_config, probe_config, enabled)
VALUES
  (:'primary_endpoint_id'::uuid, :'resource_id'::uuid, 'ops-primary-' || :'run_id', 'socks'::endpoint_type, :'primary_host', 1080, '{}'::jsonb, '{}'::jsonb, '{}'::jsonb, TRUE),
  (:'backup_endpoint_id'::uuid, :'resource_id'::uuid, 'ops-backup-' || :'run_id', 'socks'::endpoint_type, :'backup_host', 1080, '{}'::jsonb, '{}'::jsonb, '{}'::jsonb, TRUE);
INSERT INTO exit_pool_members (exit_pool_id, exit_endpoint_id, weight, status, priority, allow_new_assignments)
VALUES
  (:'pool_id'::uuid, :'primary_endpoint_id'::uuid, 100, 'healthy', 200, TRUE),
  (:'pool_id'::uuid, :'backup_endpoint_id'::uuid, 100, 'healthy', 100, TRUE);
INSERT INTO line_groups (id, name)
VALUES (:'group_id'::uuid, :'line_name');
INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
VALUES (:'group_id'::uuid, :'primary_endpoint_id'::uuid), (:'group_id'::uuid, :'backup_endpoint_id'::uuid)
ON CONFLICT DO NOTHING;
INSERT INTO access_lines (id, name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port, protocol, transport, user_uuid, enabled, region_code, region_name, region_flag, identity_mode, user_key_source, inbound_config, visibility_weight)
VALUES (:'line_id'::uuid, :'line_name', :'node_id'::uuid, :'group_id'::uuid, :'primary_endpoint_id'::uuid, :'pool_id'::uuid, :'line_host', :'listen_port'::int, 'vless', 'tcp', gen_random_uuid()::text, TRUE, 'UAT', 'Ops Recovery UAT', 'UAT', 'credential', 'xray_email', '{}'::jsonb, 100);
INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
VALUES (:'plan_id'::uuid, :'group_id'::uuid, 1), (:'delete_plan_id'::uuid, :'group_id'::uuid, 1);
INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id)
VALUES (:'delete_user_id'::uuid, :'group_id'::uuid, :'line_id'::uuid);
INSERT INTO user_exit_assignments (user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason)
VALUES (:'delete_user_id'::uuid, :'line_id'::uuid, :'pool_id'::uuid, :'primary_endpoint_id'::uuid, 'pre_delete_seed');
INSERT INTO usage_ledgers (access_line_id, user_id, xray_user_key, traffic_source, delta_uplink, delta_downlink, billing_multiplier, billed_bytes, collected_at, delta_total, billed_uplink, billed_downlink, recorded_at, exit_endpoint_id)
VALUES (:'line_id'::uuid, :'main_user_id'::uuid, 'uat-main-' || :'run_id', 'access_line', 1024, 2048, 1, 3072, now(), 3072, 1024, 2048, now(), :'primary_endpoint_id'::uuid);
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.setup', 'uat_run', :'node_id'::uuid, jsonb_build_object('run_id', :'run_id', 'scope', 'temporary-data-only'), 'success');
COMMIT;
SQL
echo "ops_mistake_recovery_uat: setup_ok"

sub_file="$tmp_dir/sub-initial.yaml"
fetch_subscription "$sub_file"
assert_has_line "$sub_file"
require_eq "$(assignment_count)" "1" "initial assignment was not generated"
require_eq "$(assigned_endpoint)" "$primary_endpoint_id" "initial assignment did not choose primary endpoint"
require_count "$(psql_scalar -v user_id="$main_user_id" <<'SQL'
SELECT COUNT(*)::text FROM usage_ledgers WHERE user_id = :'user_id'::uuid AND billed_bytes > 0;
SQL
)" "temporary ledger verification failed"
echo "ops_mistake_recovery_uat: initial_subscription_ledger_ok"

psql_db -Xq -v ON_ERROR_STOP=1 -v run_id="$run_id" -v pool_id="$pool_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE exit_pool_members
SET status = 'offline',
    allow_new_assignments = FALSE,
    updated_at = now()
WHERE exit_pool_id = :'pool_id'::uuid;
DELETE FROM user_exit_assignments WHERE exit_pool_id = :'pool_id'::uuid;
DELETE FROM user_access_line_assignments WHERE access_line_id IN (
  SELECT id FROM access_lines WHERE exit_pool_id = :'pool_id'::uuid
);
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'ops_mistake_recovery_uat_exit_members_disabled'
WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.disable_exit_members', 'exit_pool', :'pool_id'::uuid, jsonb_build_object('run_id', :'run_id', 'status', 'offline'), 'success');
COMMIT;
SQL
sub_file="$tmp_dir/sub-pool-disabled.yaml"
if fetch_subscription_optional "$sub_file"; then
  assert_missing_line "$sub_file"
fi
require_eq "$(assignment_count)" "0" "disabled exit pool did not clear temporary assignments"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_exit_members_disabled" "dirty reason missing after exit pool member disable"
echo "ops_mistake_recovery_uat: disabled_exit_pool_verified"

psql_db -Xq -v ON_ERROR_STOP=1 -v run_id="$run_id" -v pool_id="$pool_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE exit_pool_members
SET status = 'healthy',
    allow_new_assignments = TRUE,
    updated_at = now()
WHERE exit_pool_id = :'pool_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'ops_mistake_recovery_uat_exit_members_restored'
WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.restore_exit_members', 'exit_pool', :'pool_id'::uuid, jsonb_build_object('run_id', :'run_id', 'status', 'healthy'), 'success');
COMMIT;
SQL
sub_file="$tmp_dir/sub-pool-restored.yaml"
fetch_subscription "$sub_file"
assert_has_line "$sub_file"
require_eq "$(assigned_endpoint)" "$primary_endpoint_id" "restored exit pool did not recover primary assignment"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_exit_members_restored" "dirty reason missing after exit pool member restore"
echo "ops_mistake_recovery_uat: restored_exit_pool_verified"

psql_db -Xq -v ON_ERROR_STOP=1 -v run_id="$run_id" -v line_id="$line_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE access_lines SET enabled = FALSE WHERE id = :'line_id'::uuid;
UPDATE access_nodes SET config_dirty = TRUE, config_dirty_at = now(), config_dirty_reason = 'ops_mistake_recovery_uat_access_line_disabled' WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.disable_access_line', 'access_line', :'line_id'::uuid, jsonb_build_object('run_id', :'run_id', 'enabled', false), 'success');
COMMIT;
SQL
sub_file="$tmp_dir/sub-line-disabled.yaml"
if fetch_subscription_optional "$sub_file"; then
  assert_missing_line "$sub_file"
fi
require_eq "$(assignment_count)" "0" "disabled access line did not clear temporary assignments"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_access_line_disabled" "dirty reason missing after access line disable"
echo "ops_mistake_recovery_uat: disabled_access_line_verified"

psql_db -Xq -v ON_ERROR_STOP=1 -v run_id="$run_id" -v line_id="$line_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE access_lines SET enabled = TRUE WHERE id = :'line_id'::uuid;
UPDATE access_nodes SET config_dirty = TRUE, config_dirty_at = now(), config_dirty_reason = 'ops_mistake_recovery_uat_access_line_restored' WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.restore_access_line', 'access_line', :'line_id'::uuid, jsonb_build_object('run_id', :'run_id', 'enabled', true), 'success');
COMMIT;
SQL
sub_file="$tmp_dir/sub-line-restored.yaml"
fetch_subscription "$sub_file"
assert_has_line "$sub_file"
echo "ops_mistake_recovery_uat: restored_access_line_verified"

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v run_id="$run_id" -v node_id="$node_id" -v endpoint_id="$primary_endpoint_id" <<'SQL'
BEGIN;
INSERT INTO access_exit_probes (
  access_node_id, exit_endpoint_id, status, latency_ms, error_summary, probed_at
)
VALUES (:'node_id'::uuid, :'endpoint_id'::uuid, 'queued', NULL, 'ops recovery queued probe', now());
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'ops_mistake_recovery_uat_probe_queued'
WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.queue_exit_probe', 'exit_endpoint', :'endpoint_id'::uuid, jsonb_build_object('run_id', :'run_id', 'queued', true), 'success');
COMMIT;
SQL
require_count "$(psql_scalar -v node_id="$node_id" -v endpoint_id="$primary_endpoint_id" <<'SQL'
SELECT COUNT(*)::text
FROM access_exit_probes queued
WHERE access_node_id = :'node_id'::uuid
  AND exit_endpoint_id = :'endpoint_id'::uuid
  AND status = 'queued'
  AND NOT EXISTS (
    SELECT 1
    FROM access_exit_probes result
    WHERE result.access_node_id = queued.access_node_id
      AND result.exit_endpoint_id = queued.exit_endpoint_id
      AND result.status <> 'queued'
      AND result.probed_at >= queued.probed_at
  );
SQL
)" "queued exit probe was not visible to access-agent scheduling"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_probe_queued" "dirty reason missing after probe queue"

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v run_id="$run_id" -v node_id="$node_id" -v endpoint_id="$primary_endpoint_id" <<'SQL'
BEGIN;
INSERT INTO access_exit_probes (
  access_node_id, exit_endpoint_id, status, latency_ms, error_summary, probed_at
)
VALUES (:'node_id'::uuid, :'endpoint_id'::uuid, 'healthy', 1, '', now() + interval '1 second');
INSERT INTO access_exit_probe_states (
  access_node_id, exit_endpoint_id, effective_status,
  consecutive_successes, last_probe_status, last_latency_ms,
  last_error_summary, last_probe_at, status_changed_at, updated_at
)
VALUES (:'node_id'::uuid, :'endpoint_id'::uuid, 'healthy', 1, 'healthy', 1, '', now(), now(), now())
ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
  effective_status = EXCLUDED.effective_status,
  consecutive_successes = EXCLUDED.consecutive_successes,
  last_probe_status = EXCLUDED.last_probe_status,
  last_latency_ms = EXCLUDED.last_latency_ms,
  last_error_summary = EXCLUDED.last_error_summary,
  last_probe_at = EXCLUDED.last_probe_at,
  status_changed_at = EXCLUDED.status_changed_at,
  updated_at = EXCLUDED.updated_at;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'ops_mistake_recovery_uat_probe_result'
WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.resolve_exit_probe', 'exit_endpoint', :'endpoint_id'::uuid, jsonb_build_object('run_id', :'run_id', 'status', 'healthy'), 'success');
COMMIT;
SQL
require_eq "$(psql_scalar -v node_id="$node_id" -v endpoint_id="$primary_endpoint_id" <<'SQL'
SELECT EXISTS (
  SELECT 1
  FROM access_exit_probe_states
  WHERE access_node_id = :'node_id'::uuid
    AND exit_endpoint_id = :'endpoint_id'::uuid
    AND effective_status = 'healthy'
    AND last_probe_at >= now() - interval '5 minutes'
)::text;
SQL
)" "true" "exit probe state did not recover to healthy"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_probe_result" "dirty reason missing after probe result"
echo "ops_mistake_recovery_uat: probe_queue_recovery_verified"

api_expect_2xx "delete_plan_migrate" "DELETE" "/api/admin/plans/${delete_plan_id}"
require_eq "$(psql_scalar -v plan_id="$delete_plan_id" <<'SQL'
SELECT (is_deleted AND NOT enabled)::text FROM plans WHERE id = :'plan_id'::uuid;
SQL
)" "true" "soft-deleted plan state is invalid"
require_eq "$(psql_scalar -v user_id="$delete_user_id" -v default_plan_id="$default_plan_id" <<'SQL'
SELECT (plan_id = :'default_plan_id'::uuid)::text FROM user_subscriptions WHERE user_id = :'user_id'::uuid;
SQL
)" "true" "subscription was not migrated to default plan"
require_eq "$(psql_scalar -v user_id="$delete_user_id" -v group_id="$group_id" -v line_id="$line_id" -v pool_id="$pool_id" <<'SQL'
SELECT (COUNT(*) = 0)::text FROM (
  SELECT user_id
  FROM user_access_line_assignments
  WHERE user_id = :'user_id'::uuid
    AND line_group_id = :'group_id'::uuid
    AND access_line_id = :'line_id'::uuid
  UNION ALL
  SELECT user_id
  FROM user_exit_assignments
  WHERE user_id = :'user_id'::uuid
    AND access_line_id = :'line_id'::uuid
    AND exit_pool_id = :'pool_id'::uuid
) stale;
SQL
)" "true" "plan soft delete did not clear stale assignments"
require_eq "$(dirty_reason)" "admin_deleted_plan" "dirty reason missing after admin plan delete"
echo "ops_mistake_recovery_uat: plan_soft_delete_migration_verified"

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v run_id="$run_id" -v user_id="$main_user_id" -v line_id="$line_id" \
  -v pool_id="$pool_id" -v backup_endpoint_id="$backup_endpoint_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE user_exit_assignments
SET exit_endpoint_id = :'backup_endpoint_id'::uuid, assigned_at = now(), failover_reason = 'ops_mistake_recovery_uat_wrong_manual_assignment'
WHERE user_id = :'user_id'::uuid AND access_line_id = :'line_id'::uuid AND exit_pool_id = :'pool_id'::uuid;
UPDATE access_nodes SET config_dirty = TRUE, config_dirty_at = now(), config_dirty_reason = 'ops_mistake_recovery_uat_wrong_assignment' WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.wrong_assignment', 'user_exit_assignment', :'line_id'::uuid, jsonb_build_object('run_id', :'run_id', 'manual_assignment', 'backup'), 'success');
COMMIT;
SQL
require_eq "$(assigned_endpoint)" "$backup_endpoint_id" "wrong assignment was not applied"
echo "ops_mistake_recovery_uat: wrong_assignment_applied"

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v run_id="$run_id" -v user_id="$main_user_id" -v line_id="$line_id" \
  -v pool_id="$pool_id" -v primary_endpoint_id="$primary_endpoint_id" -v node_id="$node_id" <<'SQL'
BEGIN;
UPDATE user_exit_assignments
SET exit_endpoint_id = :'primary_endpoint_id'::uuid,
    assigned_at = now(),
    failover_reason = 'ops_mistake_recovery_uat_rule_recovery'
WHERE user_id = :'user_id'::uuid
  AND access_line_id = :'line_id'::uuid
  AND exit_pool_id = :'pool_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'ops_mistake_recovery_uat_assignment_restored'
WHERE id = :'node_id'::uuid;
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
VALUES ('ops_mistake_recovery_uat.restore_assignment', 'user_exit_assignment', :'line_id'::uuid, jsonb_build_object('run_id', :'run_id', 'recovered', true), 'success');
COMMIT;
SQL
sub_file="$tmp_dir/sub-assignment-restored.yaml"
fetch_subscription "$sub_file"
assert_has_line "$sub_file"
require_eq "$(assigned_endpoint)" "$primary_endpoint_id" "assignment recovery did not restore primary endpoint"
require_eq "$(dirty_reason)" "ops_mistake_recovery_uat_assignment_restored" "dirty reason missing after assignment recovery"
require_count "$(psql_scalar -v run_id="$run_id" <<'SQL'
SELECT COUNT(*)::text FROM audit_logs WHERE action LIKE 'ops_mistake_recovery_uat.%' AND request_summary->>'run_id' = :'run_id';
SQL
)" "audit markers are missing"
require_eq "$(psql_scalar -v token="$sub_token" <<'SQL'
SELECT EXISTS (SELECT 1 FROM audit_logs WHERE action LIKE 'ops_mistake_recovery_uat.%' AND request_summary::text LIKE '%' || :'token' || '%')::text;
SQL
)" "false" "audit summary leaked subscription token"
echo "ops_mistake_recovery_uat: assignment_recovery_audit_redaction_verified"
echo "ops_mistake_recovery_uat: passed"
