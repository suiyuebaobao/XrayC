#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
set +x

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
# shellcheck source=real-e2e-lib.sh
. "$BASE_DIR/scripts/real-e2e-lib.sh"
# shellcheck source=scripts/lib/real-multi-user-traffic-uat/runtime-port.sh
. "$BASE_DIR/scripts/lib/real-multi-user-traffic-uat/runtime-port.sh"
# shellcheck source=scripts/lib/real-multi-user-traffic-uat/control.sh
. "$BASE_DIR/scripts/lib/real-multi-user-traffic-uat/control.sh"
# shellcheck source=scripts/lib/real-multi-user-traffic-uat/selection.sh
. "$BASE_DIR/scripts/lib/real-multi-user-traffic-uat/selection.sh"
# shellcheck source=scripts/lib/real-multi-user-traffic-uat/plan.sh
. "$BASE_DIR/scripts/lib/real-multi-user-traffic-uat/plan.sh"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
CALLER_ACCESS_LINE_ID_SET=0
CALLER_EXPECTED_EXIT_IP_SET=0
if [[ "${ACCESS_LINE_ID+x}" == "x" ]]; then
  CALLER_ACCESS_LINE_ID_SET=1
fi
if [[ "${EXPECTED_EXIT_IP+x}" == "x" ]]; then
  CALLER_EXPECTED_EXIT_IP_SET=1
fi
CALLER_ACCESS_LINE_ID="${ACCESS_LINE_ID:-}"
CALLER_EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi
if [[ "$CALLER_ACCESS_LINE_ID_SET" == "1" ]]; then
  # preserve caller-provided runtime line when .env.real-release still has an older kept E2E line.
  ACCESS_LINE_ID="$CALLER_ACCESS_LINE_ID"
fi
if [[ "$CALLER_EXPECTED_EXIT_IP_SET" == "1" ]]; then
  EXPECTED_EXIT_IP="$CALLER_EXPECTED_EXIT_IP"
fi

TMP_DIR="$(mktemp -d)"
RUN_ID="$(date +%s)-$$"
REMOTE_TMP="/tmp/xrayc-multi-user-traffic-${RUN_ID}"
REMOTE_CLIENT_DIR="/opt/xrayc-multi-user-traffic"

cleanup_remote_done=0
db_users_prepared=0
db_users_cleaned=0
TEMP_PLAN_ID=""
TEMP_LINE_GROUP_ID=""
temp_plan_cleaned=0
ORIGINAL_EXIT_POOL_STRATEGY=""
exit_pool_strategy_restored=0

die() {
  echo "$*" >&2
  exit 1
}

cleanup() {
  local status=$?
  if declare -F cleanup_db_users >/dev/null 2>&1; then
    cleanup_db_users >/dev/null 2>&1 || true
  fi
  if declare -F cleanup_temp_plan >/dev/null 2>&1; then
    cleanup_temp_plan >/dev/null 2>&1 || true
  fi
  if declare -F restore_exit_pool_strategy >/dev/null 2>&1; then
    restore_exit_pool_strategy >/dev/null 2>&1 || true
  fi
  if [[ "$status" != "0" && "${CLEANUP_ON_FAILURE:-1}" == "0" && -n "${CLIENT_SSH_HOST:-}" ]]; then
    echo "real_multi_user_traffic_uat: preserving_remote_tmp=${REMOTE_TMP}" >&2
  elif [[ "$cleanup_remote_done" != "1" && -n "${CLIENT_SSH_HOST:-}" ]]; then
    remote_client_cleanup >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

require_env() {
  local name="$1"
  [[ -n "${!name:-}" ]] || die "${name} is required"
}

require_env BASE_URL
require_env DATABASE_URL
require_env XRAYC_REAL_E2E_INVENTORY
require_env DEPLOY_ARTIFACT_TOKEN

USER_COUNT="${XRAYC_REAL_MULTI_USER_COUNT:-4}"
DURATION_SECONDS="${XRAYC_REAL_MULTI_USER_DURATION_SECONDS:-1200}"
POLL_SETTLE_SECONDS="${XRAYC_REAL_MULTI_USER_SETTLE_SECONDS:-20}"
SOCKS_PORT_BASE="${XRAYC_REAL_MULTI_USER_SOCKS_PORT_BASE:-32100}"
TRAFFIC_RATE="${XRAYC_REAL_MULTI_USER_TRAFFIC_RATE:-96k}"
TRAFFIC_MAX_TIME="${XRAYC_REAL_MULTI_USER_TRAFFIC_MAX_TIME_SECONDS:-60}"
TRAFFIC_URLS="${XRAYC_REAL_MULTI_USER_TRAFFIC_URLS:-}"
# Current private role convention: server_1 is direct/non-CF and server_4 is
# reserved for Cloudflare/orange-cloud entry validation.
CLIENT_ALIAS="${XRAYC_REAL_MULTI_USER_CLIENT_ALIAS:-server_1}"
TRANSIT_ALIAS="${XRAYC_REAL_MULTI_USER_TRANSIT_ALIAS:-server_2}"
USER_RATE_LIMIT_BPS="${XRAYC_REAL_MULTI_USER_RATE_LIMIT_BPS:-1000000}"
ROUNDS="${XRAYC_REAL_MULTI_USER_ROUNDS:-2}"
UPLOAD_PORT="${XRAYC_REAL_MULTI_USER_UPLOAD_PORT:-32980}"
UPLOAD_BYTES="${XRAYC_REAL_MULTI_USER_UPLOAD_BYTES:-2097152}"
UPLOAD_MAX_TIME="${XRAYC_REAL_MULTI_USER_UPLOAD_MAX_TIME_SECONDS:-180}"
ALLOWED_FAILURES="${XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES:-3}"
CLEANUP_ON_FAILURE="${XRAYC_REAL_MULTI_USER_CLEANUP_ON_FAILURE:-1}"

for item in \
  XRAYC_REAL_MULTI_USER_COUNT="$USER_COUNT" \
  XRAYC_REAL_MULTI_USER_DURATION_SECONDS="$DURATION_SECONDS" \
  XRAYC_REAL_MULTI_USER_SETTLE_SECONDS="$POLL_SETTLE_SECONDS" \
  XRAYC_REAL_MULTI_USER_SOCKS_PORT_BASE="$SOCKS_PORT_BASE" \
  XRAYC_REAL_MULTI_USER_TRAFFIC_MAX_TIME_SECONDS="$TRAFFIC_MAX_TIME" \
  XRAYC_REAL_MULTI_USER_RATE_LIMIT_BPS="$USER_RATE_LIMIT_BPS" \
  XRAYC_REAL_MULTI_USER_UPLOAD_PORT="$UPLOAD_PORT" \
  XRAYC_REAL_MULTI_USER_UPLOAD_BYTES="$UPLOAD_BYTES" \
  XRAYC_REAL_MULTI_USER_UPLOAD_MAX_TIME_SECONDS="$UPLOAD_MAX_TIME"; do
  require_positive_int "${item%%=*}" "${item#*=}"
done

if (( USER_COUNT < 2 )); then
  die "XRAYC_REAL_MULTI_USER_COUNT must be at least 2"
fi
if (( DURATION_SECONDS < 1200 )); then
  die "XRAYC_REAL_MULTI_USER_DURATION_SECONDS must be at least 1200 for the required 20-minute UAT"
fi
case "$CLEANUP_ON_FAILURE" in
  0|1) ;;
  *) die "XRAYC_REAL_MULTI_USER_CLEANUP_ON_FAILURE must be 0 or 1" ;;
esac

run_multi_user_rounds_if_requested
verify_inventory_contains_default_aliases

load_inventory_target "$CLIENT_ALIAS" CLIENT >"$TMP_DIR/client-target.env"
load_inventory_target "$TRANSIT_ALIAS" TRANSIT >"$TMP_DIR/transit-target.env"
# shellcheck disable=SC1090
. "$TMP_DIR/client-target.env"
# shellcheck disable=SC1090
. "$TMP_DIR/transit-target.env"

if [[ -z "$TRAFFIC_URLS" ]]; then
  TRAFFIC_URLS="http://${CLIENT_SSH_HOST}:${UPLOAD_PORT}/download?bytes=1048576"
fi

[[ -f "$CLIENT_SSH_PASSWORD_FILE" ]] || die "client SSH password file is missing"
[[ -f "$TRANSIT_SSH_PASSWORD_FILE" ]] || die "transit SSH password file is missing"

ssh_client() {
  local attempt
  for attempt in 1 2 3; do
    if sshpass -f "$CLIENT_SSH_PASSWORD_FILE" ssh -p "$CLIENT_SSH_PORT" \
      -o ConnectTimeout=15 -o ServerAliveInterval=15 \
      -o StrictHostKeyChecking=accept-new -o BatchMode=no \
      "$CLIENT_SSH_USER@$CLIENT_SSH_HOST" "$@" 2>"$TMP_DIR/ssh-client.err"; then
      return 0
    fi
    sleep $((attempt * 2))
  done
  return 255
}

scp_client_to() {
  local source="$1"
  local target="$2"
  local attempt
  for attempt in 1 2 3; do
    if sshpass -f "$CLIENT_SSH_PASSWORD_FILE" scp -P "$CLIENT_SSH_PORT" \
      -o ConnectTimeout=15 -o StrictHostKeyChecking=accept-new -o BatchMode=no \
      "$source" "$CLIENT_SSH_USER@$CLIENT_SSH_HOST:$target" >/dev/null 2>"$TMP_DIR/scp-client.err"; then
      return 0
    fi
    sleep $((attempt * 2))
  done
  return 255
}

ssh_transit() {
  local attempt
  for attempt in 1 2 3; do
    if sshpass -f "$TRANSIT_SSH_PASSWORD_FILE" ssh -p "$TRANSIT_SSH_PORT" \
      -o ConnectTimeout=15 -o ServerAliveInterval=15 \
      -o StrictHostKeyChecking=accept-new -o BatchMode=no \
      "$TRANSIT_SSH_USER@$TRANSIT_SSH_HOST" "$@" 2>"$TMP_DIR/ssh-transit.err"; then
      return 0
    fi
    sleep $((attempt * 2))
  done
  return 255
}

remote_client_cleanup() {
  ssh_client "if [ -s $(shell_quote "$REMOTE_TMP/upload-sink.pid") ]; then kill \$(cat $(shell_quote "$REMOTE_TMP/upload-sink.pid")) >/dev/null 2>&1 || true; fi; while iptables -D INPUT -p tcp --dport $(shell_quote "$UPLOAD_PORT") -j ACCEPT >/dev/null 2>&1; do :; done; for name in \$(docker ps -aq --filter 'name=xrayc-multi-user-client-${RUN_ID}-' 2>/dev/null); do docker rm -f \"\$name\" >/dev/null 2>&1 || true; done; rm -rf $(shell_quote "$REMOTE_TMP")" || true
}

run_psql() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -v ON_ERROR_STOP=1 "$@"
}

echo "real_multi_user_traffic_uat: preparing users=${USER_COUNT} duration_seconds=${DURATION_SECONDS}"
select_multi_user_access_line
case "$PROTOCOL" in
  vless|trojan|shadowsocks|ss) ;;
  *) die "multi-user client builder does not support current access protocol" ;;
esac
if [[ "$PROTOCOL" == "ss" ]]; then
  PROTOCOL="shadowsocks"
fi
if [[ -n "${ACCESS_NODE_PUBLIC_HOST:-}" \
  && "$ACCESS_NODE_PUBLIC_HOST" != "${TRANSIT_SSH_HOST:-}" \
  && "$ACCESS_NODE_PUBLIC_HOST" != "${TRANSIT_PUBLIC_HOST:-}" ]]; then
  load_inventory_target_by_ssh_host "$ACCESS_NODE_PUBLIC_HOST" TRANSIT >"$TMP_DIR/transit-target.env"
  # shellcheck disable=SC1090
  . "$TMP_DIR/transit-target.env"
fi
[[ -n "${TRANSIT_SSH_HOST:-}" \
  && ( "${TRANSIT_SSH_HOST:-}" == "${ACCESS_NODE_PUBLIC_HOST:-}" \
    || "${TRANSIT_PUBLIC_HOST:-}" == "${ACCESS_NODE_PUBLIC_HOST:-}" ) ]] \
  || die "selected access node is not present in the real inventory"

load_multi_user_exit_endpoints
ensure_priority_exit_pool_strategy
create_multi_user_temp_plan
prepare_multi_user_db_users

CONFIG_DIR="$TMP_DIR/client-configs"
mkdir -p "$CONFIG_DIR"
PORTS_TSV="$TMP_DIR/ports.tsv"
RUNTIME_PORTS_TSV="$TMP_DIR/runtime-ports.tsv"
: >"$PORTS_TSV"
: >"$RUNTIME_PORTS_TSV"
idx=0
while IFS=$'\t' read -r user_index user_id email xray_key credential token token_hash; do
  idx=$((idx + 1))
  sub_file="$TMP_DIR/subscription-${user_index}.yaml"
  curl --fail --silent --show-error --location \
    --connect-timeout 10 --max-time 30 \
    --output "$sub_file" \
    "${BASE_URL%/}/sub/${token}" >/dev/null
  runtime_port="$(extract_subscription_runtime_port "$sub_file" "$PROTOCOL" "$LISTEN_PORT")"
  user_config_dir="$CONFIG_DIR/user-${user_index}"
  port=$((SOCKS_PORT_BASE + idx - 1))
  python3 scripts/real-access-inbound-matrix-client-configs.py build \
    "$sub_file" "$user_config_dir" "$port" \
    --preferred-port "${PROTOCOL}=${runtime_port}" \
    "$PROTOCOL" >/dev/null
  config_file="$user_config_dir/${PROTOCOL}.json"
  [[ -s "$config_file" ]] || die "client config was not generated"
  printf '%s\t%s\t%s\t%s\n' "$user_index" "$user_id" "$xray_key" "$port" >>"$PORTS_TSV"
  printf '%s\t%s\t%s\t%s\n' "$user_index" "$user_id" "$xray_key" "$runtime_port" >>"$RUNTIME_PORTS_TSV"
done <"$USERS_TSV"

verify_runtime_ports_file "$RUNTIME_PORTS_TSV" "$USER_COUNT" "$LISTEN_PORT"
reapply_explicit_exit_assignments

echo "real_multi_user_traffic_uat: waiting_for_agent_config"
XRAY_KEYS_B64="$(cut -f4 "$USERS_TSV" | base64 -w0)"
for _ in $(seq 1 80); do
  dirty="$(run_psql -XAtq -v access_node_id="$ACCESS_NODE_ID" <<'SQL'
SELECT config_dirty::text
FROM access_nodes
WHERE id = :'access_node_id'::uuid
  AND last_heartbeat_at >= now() - interval '60 seconds';
SQL
)"
  if [[ "$dirty" == "false" ]]; then
    break
  fi
  sleep 3
done

dirty="$(run_psql -XAtq -v access_node_id="$ACCESS_NODE_ID" <<'SQL'
SELECT config_dirty::text FROM access_nodes WHERE id = :'access_node_id'::uuid;
SQL
)"
[[ "$dirty" == "false" ]] || die "agent did not apply multi-user config"

assert_remote_strict_runtime_ports "$RUNTIME_PORTS_TSV"
echo "real_multi_user_traffic_uat: strict_runtime_ports=ok"

tarball="$TMP_DIR/client-configs.tar.gz"
tar -C "$CONFIG_DIR" -czf "$tarball" .
scp_client_to "$tarball" "${REMOTE_TMP}.tar.gz"

REMOTE_ENV="$TMP_DIR/remote.env"
EXPECTED_EXIT_IP_FOR_REMOTE="${XRAYC_REAL_MULTI_USER_EXPECTED_EXIT_IP:-}"
if [[ "$CALLER_EXPECTED_EXIT_IP_SET" == "1" ]]; then
  EXPECTED_EXIT_IP_FOR_REMOTE="$CALLER_EXPECTED_EXIT_IP"
fi
if (( ENDPOINT_COUNT > 1 )); then
  EXPECTED_EXIT_IP_FOR_REMOTE=""
fi
{
  printf 'BASE_URL=%s\n' "$(shell_quote "$BASE_URL")"
  printf 'DEPLOY_ARTIFACT_TOKEN=%s\n' "$(shell_quote "$DEPLOY_ARTIFACT_TOKEN")"
  printf 'REMOTE_TMP=%s\n' "$(shell_quote "$REMOTE_TMP")"
  printf 'REMOTE_CLIENT_DIR=%s\n' "$(shell_quote "$REMOTE_CLIENT_DIR")"
  printf 'RUN_ID=%s\n' "$(shell_quote "$RUN_ID")"
  printf 'USER_COUNT=%s\n' "$(shell_quote "$USER_COUNT")"
  printf 'DURATION_SECONDS=%s\n' "$(shell_quote "$DURATION_SECONDS")"
  printf 'TRAFFIC_RATE=%s\n' "$(shell_quote "$TRAFFIC_RATE")"
  printf 'TRAFFIC_MAX_TIME=%s\n' "$(shell_quote "$TRAFFIC_MAX_TIME")"
  printf 'TRAFFIC_URLS=%s\n' "$(shell_quote "$TRAFFIC_URLS")"
  printf 'PUBLIC_IP_URL=%s\n' "$(shell_quote "${PUBLIC_IP_URL:-https://api.ipify.org}")"
  printf 'EXPECTED_EXIT_IP=%s\n' "$(shell_quote "$EXPECTED_EXIT_IP_FOR_REMOTE")"
  printf 'UPLOAD_LISTEN_PORT=%s\n' "$(shell_quote "$UPLOAD_PORT")"
  printf 'UPLOAD_URL=%s\n' "$(shell_quote "${XRAYC_REAL_MULTI_USER_UPLOAD_URL:-http://${CLIENT_SSH_HOST}:${UPLOAD_PORT}/upload}")"
  printf 'UPLOAD_BYTES=%s\n' "$(shell_quote "$UPLOAD_BYTES")"
  printf 'UPLOAD_MAX_TIME=%s\n' "$(shell_quote "$UPLOAD_MAX_TIME")"
  printf 'RATE_LIMIT_BPS=%s\n' "$(shell_quote "$USER_RATE_LIMIT_BPS")"
} >"$REMOTE_ENV"
scp_client_to "$REMOTE_ENV" "${REMOTE_TMP}.env"
scp_client_to "$PORTS_TSV" "${REMOTE_TMP}.ports.tsv"

REMOTE_RUNNER="scripts/lib/real-multi-user-traffic-uat/remote-runner.sh"
scp_client_to "$REMOTE_RUNNER" "${REMOTE_TMP}.runner.sh"
REMOTE_LAUNCHER="scripts/lib/real-multi-user-traffic-uat/remote-launcher.sh"
scp_client_to "$REMOTE_LAUNCHER" "${REMOTE_TMP}.launcher.sh"

ssh_client "rm -rf $(shell_quote "$REMOTE_TMP"); mkdir -p $(shell_quote "$REMOTE_TMP"); mv $(shell_quote "${REMOTE_TMP}.tar.gz") $(shell_quote "$REMOTE_TMP/configs.tar.gz"); mv $(shell_quote "${REMOTE_TMP}.env") $(shell_quote "$REMOTE_TMP/remote.env"); mv $(shell_quote "${REMOTE_TMP}.ports.tsv") $(shell_quote "$REMOTE_TMP/ports.tsv"); mv $(shell_quote "${REMOTE_TMP}.runner.sh") $(shell_quote "$REMOTE_TMP/runner.sh"); mv $(shell_quote "${REMOTE_TMP}.launcher.sh") $(shell_quote "$REMOTE_TMP/launcher.sh"); chmod 700 $(shell_quote "$REMOTE_TMP/runner.sh") $(shell_quote "$REMOTE_TMP/launcher.sh")"

echo "real_multi_user_traffic_uat: remote_clients_starting"
REMOTE_RESULTS="$TMP_DIR/remote-results.txt"
ssh_client "bash $(shell_quote "$REMOTE_TMP/launcher.sh") $(shell_quote "$REMOTE_TMP")"
remote_wait_deadline=$((SECONDS + DURATION_SECONDS + 900))
remote_status=""
while [[ "$SECONDS" -le "$remote_wait_deadline" ]]; do
  remote_status="$(ssh_client "if [ -s $(shell_quote "$REMOTE_TMP/remote-status") ]; then cat $(shell_quote "$REMOTE_TMP/remote-status"); elif [ -s $(shell_quote "$REMOTE_TMP/remote-runner.pid") ] && kill -0 \$(cat $(shell_quote "$REMOTE_TMP/remote-runner.pid")) >/dev/null 2>&1; then printf RUNNING; else printf MISSING; fi" | tr -d '[:space:]' || true)"
  case "$remote_status" in
    RUNNING|"")
      sleep 15
      ;;
    MISSING)
      die "remote multi-user traffic runner disappeared"
      ;;
    *)
      break
      ;;
  esac
done
[[ -n "$remote_status" && "$remote_status" != "RUNNING" ]] \
  || die "remote multi-user traffic runner timed out"
if [[ "$remote_status" != "0" ]]; then
  ssh_client "grep -E '^(upload_sink_readiness|client_[0-9]+_(readiness|egress|upload_preflight))=failed$' $(shell_quote "$REMOTE_TMP/remote-stderr.txt") 2>/dev/null || true" \
    >"$TMP_DIR/remote-error-markers.txt" || true
  remote_error_markers="$(tr '\n' ',' <"$TMP_DIR/remote-error-markers.txt" | sed 's/,$//')"
  die "remote multi-user traffic runner failed markers=${remote_error_markers:-none}"
fi
ssh_client "cat $(shell_quote "$REMOTE_TMP/remote-results.txt")" | tee "$REMOTE_RESULTS"
assert_remote_traffic_results "$REMOTE_RESULTS" "$USER_COUNT" "$ALLOWED_FAILURES"

echo "real_multi_user_traffic_uat: waiting_for_agent_reports"
sleep "$POLL_SETTLE_SECONDS"

STARTED_AT="$(date -u -d "@$(( $(date +%s) - DURATION_SECONDS - POLL_SETTLE_SECONDS - 120 ))" +%Y-%m-%dT%H:%M:%SZ)"
VERIFY_TSV="$TMP_DIR/verify.tsv"
run_psql -XAtq -F $'\t' \
  -v started_at="$STARTED_AT" \
  -v access_node_id="$ACCESS_NODE_ID" \
  -v access_line_id="$ACCESS_LINE_ID" \
  -v users_file="$USERS_TSV" <<SQL >"$VERIFY_TSV"
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
WITH per_user AS (
  SELECT u.idx,
         COALESCE(SUM(l.billed_bytes), 0)::bigint AS billed_bytes,
         COUNT(l.id)::bigint AS ledger_count,
         EXISTS (
           SELECT 1 FROM access_user_sessions s
           WHERE s.access_node_id = :'access_node_id'::uuid
             AND s.access_line_id = :'access_line_id'::uuid
             AND s.xray_user_key = u.xray_user_key
             AND s.last_seen_at >= :'started_at'::timestamptz
         ) AS has_session
  FROM xrayc_multi_users u
  LEFT JOIN usage_ledgers l
    ON l.user_id = u.user_id
   AND l.access_line_id = :'access_line_id'::uuid
   AND l.traffic_source = 'access_line'
   AND l.recorded_at >= :'started_at'::timestamptz
  GROUP BY u.idx, u.xray_user_key
)
SELECT idx::text, ledger_count::text, billed_bytes::text, has_session::text
FROM per_user
ORDER BY idx;
SQL

missing=0
while IFS=$'\t' read -r idx ledger_count billed_bytes has_session; do
  echo "real_multi_user_traffic_uat: verify user=${idx} ledger_count=${ledger_count} billed_bytes=${billed_bytes} session=${has_session}"
  if [[ ! "$ledger_count" =~ ^[0-9]+$ || "$ledger_count" -le 0 ]]; then
    missing=1
  fi
  if [[ ! "$billed_bytes" =~ ^[0-9]+$ || "$billed_bytes" -le 0 ]]; then
    missing=1
  fi
  if [[ "$has_session" != "t" && "$has_session" != "true" ]]; then
    missing=1
  fi
done <"$VERIFY_TSV"
[[ "$missing" == "0" ]] || die "multi-user traffic verification failed"

ENDPOINT_VERIFY_TSV="$TMP_DIR/verify-endpoints.tsv"
run_psql -XAtq -F $'\t' \
  -v started_at="$STARTED_AT" \
  -v access_line_id="$ACCESS_LINE_ID" \
  -v users_file="$USERS_TSV" \
  -v endpoints_file="$ENDPOINTS_TSV" <<SQL >"$ENDPOINT_VERIFY_TSV"
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
SELECT endpoints.idx::text,
       COUNT(ledgers.id)::text AS ledger_count,
       COALESCE(SUM(ledgers.billed_bytes), 0)::text AS billed_bytes,
       (
         endpoints.exit_endpoint_id = (
           SELECT exit_endpoint_id
           FROM access_lines
           WHERE id = :'access_line_id'::uuid
           LIMIT 1
         )
       )::text AS is_binding_endpoint
FROM xrayc_multi_exit_endpoints endpoints
LEFT JOIN usage_ledgers ledgers
  ON ledgers.exit_endpoint_id = endpoints.exit_endpoint_id
 AND ledgers.access_line_id = :'access_line_id'::uuid
 AND ledgers.user_id IN (SELECT user_id FROM xrayc_multi_users)
 AND ledgers.traffic_source = 'access_line'
 AND ledgers.recorded_at >= :'started_at'::timestamptz
GROUP BY endpoints.idx, endpoints.exit_endpoint_id
ORDER BY endpoints.idx;
SQL

endpoint_missing=0
while IFS=$'\t' read -r idx ledger_count billed_bytes is_binding_endpoint; do
  echo "real_multi_user_traffic_uat: verify endpoint=${idx} ledger_count=${ledger_count} billed_bytes=${billed_bytes} binding_endpoint=${is_binding_endpoint}"
  if [[ "$is_binding_endpoint" != "t" && "$is_binding_endpoint" != "true" ]]; then
    continue
  fi
  if [[ ! "$ledger_count" =~ ^[0-9]+$ || "$ledger_count" -le 0 ]]; then
    endpoint_missing=1
  fi
  if [[ ! "$billed_bytes" =~ ^[0-9]+$ || "$billed_bytes" -le 0 ]]; then
    endpoint_missing=1
  fi
done <"$ENDPOINT_VERIFY_TSV"
[[ "$endpoint_missing" == "0" ]] || die "multi-user endpoint verification failed"

cleanup_db_users

echo "real_multi_user_traffic_uat: waiting_for_cleanup_config"
cleanup_applied=0
quoted_keys="$(shell_quote "$XRAY_KEYS_B64")"
xray_config_path="${XRAYC_REAL_MULTI_USER_REMOTE_XRAY_CONFIG_PATH:-/opt/xrayc/access-agent/xray/config.json}"
for _ in $(seq 1 80); do
  dirty="$(run_psql -XAtq -v access_node_id="$ACCESS_NODE_ID" <<'SQL'
SELECT config_dirty::text
FROM access_nodes
WHERE id = :'access_node_id'::uuid
  AND last_heartbeat_at >= now() - interval '60 seconds';
SQL
)"
  if [[ "$dirty" == "false" ]] && ssh_transit "XRAY_KEYS_B64=${quoted_keys} XRAYC_XRAY_CONFIG_PATH=$(shell_quote "$xray_config_path") python3 - <<'PY'
import base64
import json
import os
import subprocess
import sys
def agent_container_names():
    result = subprocess.run(
        ['docker', 'ps', '--format', '{{.Names}}', '--filter', 'name=xrayc-access-agent-'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
    )
    if result.returncode != 0:
        return []
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]
def read_xray_config(path):
    candidates = ['/etc/xray/config.json'] if path.endswith('/xray/config.json') or path == '/etc/xray/config.json' else [path]
    for name in agent_container_names():
        for candidate in candidates:
            result = subprocess.run(
                ['docker', 'exec', name, 'cat', candidate],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=20,
            )
            if result.returncode == 0 and result.stdout:
                return result.stdout
    with open(path, 'r', encoding='utf-8') as fh:
        return fh.read()
config = json.loads(read_xray_config(os.environ['XRAYC_XRAY_CONFIG_PATH']))
text = json.dumps(config, separators=(',', ':'))
keys = base64.b64decode(os.environ['XRAY_KEYS_B64']).decode().splitlines()
if any(key and key in text for key in keys):
    sys.exit(1)
PY" >/dev/null 2>&1; then
    cleanup_applied=1
    break
  fi
  sleep 3
done
[[ "$cleanup_applied" == "1" ]] || die "multi-user cleanup config was not applied"

remote_client_cleanup >/dev/null 2>&1 || true
cleanup_remote_done=1
assert_multi_user_cleanup_closed

echo "real_multi_user_traffic_uat: passed users=${USER_COUNT} duration_seconds=${DURATION_SECONDS}"
