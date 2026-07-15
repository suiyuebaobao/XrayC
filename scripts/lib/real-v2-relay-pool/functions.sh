# 真实 V2 中转池验收辅助函数。
# 由 scripts/real-v2-relay-pool-e2e.sh source 使用。
# 封装清理、远端执行、API 请求、JSON 写入和状态等待逻辑。
# 函数依赖主脚本初始化的 BASE_URL、TMP_DIR、DB_URL 等全局变量。
# 本文件不直接执行，避免在独立 shell 中缺少主脚本上下文。
# 修改时保持输出脱敏，不要在日志中打印令牌、密码或私有主机信息。
# shellcheck shell=bash

cleanup() {
  set +e
  if [[ "$keep_success_artifacts" == "1" && "$completed_ok" == "1" ]]; then
    rm -rf "$TMP_DIR"
    echo "real_v2_relay_pool_e2e: keeping_success_artifacts=1"
    return 0
  fi
  if [[ -n "${ssh_tunnel_pid:-}" ]]; then
    kill "$ssh_tunnel_pid" >/dev/null 2>&1 || true
  fi
  if [[ "$original_subscription_snapshot_saved" == "1" && -n "${user_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v user_id="$user_id" \
      -v active="$original_sub_active" \
      -v expires_at="$original_sub_expires_at" \
      -v used_bytes="$original_sub_used_bytes" \
      -v limit_bytes="$original_sub_limit_bytes" <<'SQL' >/dev/null 2>&1 || true
UPDATE user_subscriptions
SET active = :'active'::boolean,
    expires_at = :'expires_at'::timestamptz,
    used_bytes = :'used_bytes'::bigint,
    limit_bytes = :'limit_bytes'::bigint
WHERE user_id = :'user_id'::uuid;
SQL
  fi
  if [[ "$original_user_snapshot_saved" == "1" && -n "${user_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v user_id="$user_id" \
      -v disabled="$original_user_disabled" <<'SQL' >/dev/null 2>&1 || true
UPDATE users
SET disabled = :'disabled'::boolean
WHERE id = :'user_id'::uuid;
SQL
  fi
  if [[ "${rate_limit_snapshot_saved:-0}" == "1" && -n "${user_id:-}" && -n "${user_plan_id:-}" ]]; then
    if [[ "${original_user_rate_limit_bps_is_null:-0}" == "1" ]]; then
      psql_db -Xq -v ON_ERROR_STOP=0 \
        -v user_id="$user_id" \
        -v plan_id="$user_plan_id" \
        -v plan_rate="$original_plan_rate_limit_bps" <<'SQL' >/dev/null 2>&1 || true
BEGIN;
UPDATE plans SET rate_limit_bps = :'plan_rate'::bigint WHERE id = :'plan_id'::uuid;
UPDATE users SET rate_limit_bps = NULL WHERE id = :'user_id'::uuid;
COMMIT;
SQL
    else
      psql_db -Xq -v ON_ERROR_STOP=0 \
        -v user_id="$user_id" \
        -v plan_id="$user_plan_id" \
        -v plan_rate="$original_plan_rate_limit_bps" \
        -v user_rate="$original_user_rate_limit_bps" <<'SQL' >/dev/null 2>&1 || true
BEGIN;
UPDATE plans SET rate_limit_bps = :'plan_rate'::bigint WHERE id = :'plan_id'::uuid;
UPDATE users SET rate_limit_bps = :'user_rate'::bigint WHERE id = :'user_id'::uuid;
COMMIT;
SQL
    fi
  fi
  if [[ -n "${access_line_id:-}" || -n "${pool_id:-}" || -n "${access_node_id:-}" || -n "${group_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v access_line_id="${access_line_id:-00000000-0000-0000-0000-000000000000}" \
      -v access_line_id_a="${access_line_id_a:-00000000-0000-0000-0000-000000000000}" \
      -v access_line_id_b="${access_line_id_b:-00000000-0000-0000-0000-000000000000}" \
      -v pool_id="${pool_id:-00000000-0000-0000-0000-000000000000}" \
      -v pool_id_a="${pool_id_a:-00000000-0000-0000-0000-000000000000}" \
      -v pool_id_b="${pool_id_b:-00000000-0000-0000-0000-000000000000}" \
      -v access_entry_id="${access_entry_id:-00000000-0000-0000-0000-000000000000}" \
      -v access_node_id="${access_node_id:-00000000-0000-0000-0000-000000000000}" \
      -v group_id="${group_id:-00000000-0000-0000-0000-000000000000}" \
      -v exit_endpoint_a="${exit_endpoint_a:-00000000-0000-0000-0000-000000000000}" \
      -v exit_endpoint_b="${exit_endpoint_b:-00000000-0000-0000-0000-000000000000}" \
      -v run_pattern="real-exit-%-${RUN_ID}" <<'SQL' >/dev/null 2>&1 || true
DELETE FROM user_access_line_assignments
WHERE access_line_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM user_exit_assignments
WHERE access_line_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid)
   OR exit_pool_id IN (:'pool_id'::uuid, :'pool_id_a'::uuid, :'pool_id_b'::uuid);
DELETE FROM usage_ledgers
WHERE access_line_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM plan_line_groups WHERE line_group_id = :'group_id'::uuid;
DELETE FROM line_group_binding_nodes
WHERE line_group_id = :'group_id'::uuid
   OR entry_exit_binding_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM line_group_exit_endpoints WHERE line_group_id = :'group_id'::uuid
   OR exit_endpoint_id IN (:'exit_endpoint_a'::uuid, :'exit_endpoint_b'::uuid);
DELETE FROM line_groups WHERE id = :'group_id'::uuid;
DELETE FROM access_metric_snapshots
WHERE access_line_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM access_user_sessions
WHERE access_line_id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM access_lines
WHERE id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM access_entry_exit_bindings
WHERE id IN (:'access_line_id'::uuid, :'access_line_id_a'::uuid, :'access_line_id_b'::uuid)
   OR access_entry_id = :'access_entry_id'::uuid;
DELETE FROM access_entries WHERE id = :'access_entry_id'::uuid;
DELETE FROM exit_pool_members
WHERE exit_pool_id IN (:'pool_id'::uuid, :'pool_id_a'::uuid, :'pool_id_b'::uuid);
DELETE FROM access_exit_probes WHERE access_node_id = :'access_node_id'::uuid;
DELETE FROM access_exit_probe_states WHERE access_node_id = :'access_node_id'::uuid;
DELETE FROM exit_pools WHERE id IN (:'pool_id'::uuid, :'pool_id_a'::uuid, :'pool_id_b'::uuid);
DELETE FROM exit_endpoints WHERE id IN (:'exit_endpoint_a'::uuid, :'exit_endpoint_b'::uuid);
DELETE FROM exit_resources WHERE access_node_id = :'access_node_id'::uuid;
DELETE FROM exit_resources WHERE name LIKE :'run_pattern';
DELETE FROM access_nodes WHERE id = :'access_node_id'::uuid;
SQL
  fi
  if [[ "${remote_cleanup_enabled:-0}" == "1" ]] && declare -F remote_exec >/dev/null 2>&1; then
    remote_exec 1 "docker rm -f xrayc-real-client >/dev/null 2>&1 || true; rm -rf /opt/xrayc-real-client" >/dev/null 2>&1 || true
    cleanup_transit_limiter >/dev/null 2>&1 || true
    remote_exec 2 "docker compose -p xrayc-real-relay -f /opt/xrayc-real-relay/docker-compose.yml down --remove-orphans >/dev/null 2>&1 || true; docker rm -f \$(docker ps -aq --filter label=com.docker.compose.project=xrayc-real-relay) >/dev/null 2>&1 || true; rm -rf /opt/xrayc-real-relay" >/dev/null 2>&1 || true
    remote_exec "${EXIT_A_REMOTE_INDEX:-3}" "docker rm -f xrayc-real-exit-a >/dev/null 2>&1 || true; while iptables -D INPUT -p tcp --dport '$EXIT_A_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_A_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p tcp --dport '$EXIT_A_SS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_A_SS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p tcp --dport '$EXIT_A_VLESS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_A_VLESS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; if command -v ufw >/dev/null 2>&1; then ufw delete allow '${EXIT_A_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_A_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_A_SS_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_A_SS_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_A_VLESS_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_A_VLESS_PORT}/udp' >/dev/null 2>&1 || true; fi; rm -rf /opt/xrayc-real-exit-a" >/dev/null 2>&1 || true
    remote_exec "${EXIT_B_REMOTE_INDEX:-${EXIT_A_REMOTE_INDEX:-3}}" "docker rm -f xrayc-real-exit-b >/dev/null 2>&1 || true; if [ -s /opt/xrayc-real-udp-target/pid ]; then kill \$(cat /opt/xrayc-real-udp-target/pid) >/dev/null 2>&1 || true; fi; if [ -s /opt/xrayc-real-tcp-target/pid ]; then kill \$(cat /opt/xrayc-real-tcp-target/pid) >/dev/null 2>&1 || true; fi; while iptables -D INPUT -p tcp --dport '$EXIT_B_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_B_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p tcp --dport '$EXIT_B_SS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_B_SS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p tcp --dport '$EXIT_B_VLESS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$EXIT_B_VLESS_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p udp --dport '$UDP_TARGET_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; while iptables -D INPUT -p tcp --dport '$TCP_TARGET_PORT' -j ACCEPT >/dev/null 2>&1; do :; done; if command -v ufw >/dev/null 2>&1; then ufw delete allow '${EXIT_B_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_B_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_B_SS_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_B_SS_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_B_VLESS_PORT}/tcp' >/dev/null 2>&1 || true; ufw delete allow '${EXIT_B_VLESS_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${UDP_TARGET_PORT}/udp' >/dev/null 2>&1 || true; ufw delete allow '${TCP_TARGET_PORT}/tcp' >/dev/null 2>&1 || true; fi; rm -rf /opt/xrayc-real-exit-b /opt/xrayc-real-udp-target /opt/xrayc-real-tcp-target" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
}

die() {
  echo "real_v2_relay_pool_e2e: $*" >&2
  exit 1
}

require_file() {
  local path="$1"
  [[ -f "$path" ]] || die "required private file is missing"
}

psql_db() {
  xrayc_real_e2e_psql_database_url "$DB_URL" "$@"
}

remote_exec() {
  local index="$1"
  shift
  local host user port passfile
  eval "host=\$H${index}" "user=\$U${index}" "port=\$P${index}" "passfile=\$PF${index}"
  local attempt
  for attempt in 1 2 3; do
    if sshpass -f "$passfile" ssh -p "$port" \
      -o ConnectTimeout=15 -o ServerAliveInterval=15 \
      -o ServerAliveCountMax=4 \
      -o StrictHostKeyChecking=accept-new -o BatchMode=no \
      "$user@$host" "$@"; then
      return 0
    fi
    sleep $((attempt * 3))
  done
  return 255
}

write_shell_env() {
  local output="$1"
  shift
  python3 - "$output" "$@" <<'PY'
import os
import shlex
import sys

output = sys.argv[1]
names = sys.argv[2:]
with open(output, "w", encoding="utf-8") as fh:
    for name in names:
        fh.write(f"export {name}={shlex.quote(os.environ[name])}\n")
PY
  chmod 600 "$output"
}

remote_bash_env() {
  local index="$1"
  local env_file="$2"
  shift 2
  local host user port passfile script_file combined_file attempt
  eval "host=\$H${index}" "user=\$U${index}" "port=\$P${index}" "passfile=\$PF${index}"
  script_file="$TMP_DIR/remote-${index}-$$-${RANDOM}.sh"
  combined_file="$TMP_DIR/remote-${index}-combined-$$-${RANDOM}.sh"
  cat > "$script_file"
  {
    printf 'set -euo pipefail\n'
    cat "$env_file"
    cat "$script_file"
  } > "$combined_file"
  for attempt in 1 2 3; do
    if sshpass -f "$passfile" ssh -p "$port" \
      -o ConnectTimeout=15 -o ServerAliveInterval=15 \
      -o ServerAliveCountMax=4 \
      -o StrictHostKeyChecking=accept-new -o BatchMode=no \
      "$user@$host" "$@" bash -s < "$combined_file"; then
      return 0
    fi
    sleep $((attempt * 3))
  done
  return 255
}

api_json() {
  local method="$1"
  local path="$2"
  local payload="$3"
  local output="$4"
  local token="$5"
  local status config_file
  config_file="$TMP_DIR/curl-api-${RANDOM}.conf"
  API_URL="${BASE_URL}${path}" API_TOKEN="$token" API_METHOD="$method" API_PAYLOAD="$payload" \
    python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'request = "{quote(os.environ["API_METHOD"])}"\n')
    fh.write('header = "Content-Type: application/json"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
    fh.write(f'data-binary = "@{quote(os.environ["API_PAYLOAD"])}"\n')
PY
  chmod 600 "$config_file"
  status="$(curl -sS --max-time 180 -o "$output" -w '%{http_code}' --config "$config_file" || true)"
  case "$status" in
    2*) ;;
    *)
      {
        printf 'real_v2_relay_pool_e2e: api request failed method=%s path=%s status=%s body=' "$method" "$path" "$status"
        python3 - "$output" <<'PY'
import json
import re
import sys

path = sys.argv[1]
try:
    raw = open(path, "r", encoding="utf-8").read().strip()
except OSError:
    raw = ""
raw = re.sub(r"Bearer\s+[A-Za-z0-9._~+/-]+", "Bearer <redacted>", raw)
raw = re.sub(r"eyJ[A-Za-z0-9._-]+", "<jwt>", raw)
raw = re.sub(r"[A-Za-z0-9_-]{24,}", "<redacted>", raw)
try:
    parsed = json.loads(raw)
except Exception:
    print(raw[:1000])
else:
    print(json.dumps(parsed, ensure_ascii=False)[:1000])
PY
      } >&2
      die "api request failed with status ${status}"
      ;;
  esac
}

api_auth_get() {
  local path="$1"
  local output="$2"
  local token="$3"
  local status config_file
  config_file="$TMP_DIR/curl-get-${RANDOM}.conf"
  API_URL="${BASE_URL}${path}" API_TOKEN="$token" python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
PY
  chmod 600 "$config_file"
  status="$(curl -sS --max-time 30 -o "$output" -w '%{http_code}' --config "$config_file" || true)"
  case "$status" in
    2*) ;;
    *) die "api get failed with status ${status}" ;;
  esac
}

json_value() {
  local file="$1"
  local expression="$2"
  python3 - "$file" "$expression" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload) if isinstance(payload, dict) else payload
cursor = data
for part in sys.argv[2].split("."):
    if not isinstance(cursor, dict):
        raise SystemExit(1)
    cursor = cursor.get(part)
    if cursor is None:
        raise SystemExit(1)
print(cursor)
PY
}

json_array_first_value() {
  local file="$1"
  local expression="$2"
  python3 - "$file" "$expression" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload) if isinstance(payload, dict) else payload
cursor = data
for part in sys.argv[2].split("."):
    if not isinstance(cursor, dict):
        raise SystemExit(1)
    cursor = cursor.get(part)
    if cursor is None:
        raise SystemExit(1)
if not isinstance(cursor, list) or not cursor:
    raise SystemExit(1)
print(cursor[0])
PY
}

write_json() {
  local output="$1"
  local code="$2"
  python3 - "$output" > "$output" <<PY
import json
import os
import secrets
import sys

output = sys.argv[1]
$code
PY
}

login_token() {
  local account="$1"
  local password="$2"
  local label="$3"
  local payload="$TMP_DIR/${label}-login-payload.json"
  local body="$TMP_DIR/${label}-login-body.json"
  ACCOUNT_VALUE="$account" PASSWORD_VALUE="$password" \
    write_json "$payload" 'print(json.dumps({"account": os.environ["ACCOUNT_VALUE"], "password": os.environ["PASSWORD_VALUE"]}, ensure_ascii=False))'
  local status
  status="$(curl -sS --max-time 30 -o "$body" -w '%{http_code}' \
    -H "Content-Type: application/json" \
    --data-binary "@$payload" "${BASE_URL}/api/auth/login" || true)"
  case "$status" in
    2*) ;;
    *) die "${label} login failed" ;;
  esac
  json_value "$body" access_token || json_value "$body" accessToken || json_value "$body" token
}

subscription_line_present() {
  local config_file="$TMP_DIR/subscription-poll-${RANDOM}.conf"
  local body_file="$TMP_DIR/subscription-poll-${RANDOM}.yaml"
  local status
  xrayc_real_e2e_write_curl_url_config "${BASE_URL}/sub/${sub_token}" "$config_file"
  status="$(curl --silent --location --max-time 20 --output "$body_file" --write-out '%{http_code}' --config "$config_file" 2>/dev/null || true)"
  [[ "$status" == 2* ]] && grep -Fq "$subscription_proxy_name" "$body_file"
}

wait_subscription_line_absent() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if ! subscription_line_present; then
      echo "real_v2_relay_pool_e2e: subscription_eviction_ok reason=${reason}"
      return
    fi
    sleep 5
  done
  die "subscription still exposes relay line after ${reason}"
}

wait_subscription_line_present() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if subscription_line_present; then
      echo "real_v2_relay_pool_e2e: subscription_restore_ok reason=${reason}"
      return
    fi
    sleep 5
  done
  die "subscription did not restore relay line after ${reason}"
}

refresh_assigned_exit_endpoint() {
  local reason="$1"
  local safe_reason="${reason//[^A-Za-z0-9_.-]/_}"
  local refresh_file="$TMP_DIR/subscription-refresh-${safe_reason}.yaml"
  assigned_exit_endpoint_id=""
  expected_egress_ip=""
  for _ in $(seq 1 36); do
    xrayc_real_e2e_fetch_sensitive_url_to_file "${BASE_URL}/sub/${sub_token}" "$refresh_file" "subscription assignment refresh failed"
    assigned_exit_endpoint_id="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
      -v user_id="$user_id" -v line_id="$access_line_id" <<'SQL'
SELECT exit_endpoint_id
FROM user_exit_assignments
WHERE user_id = :'user_id'::uuid
  AND access_line_id = :'line_id'::uuid
LIMIT 1;
SQL
)"
    case "$assigned_exit_endpoint_id" in
      "$exit_endpoint_a")
        expected_egress_ip="$exit_a_ip"
        echo "real_v2_relay_pool_e2e: assignment_resync_ok reason=${reason}"
        return
        ;;
      "$exit_endpoint_b")
        expected_egress_ip="$exit_b_ip"
        echo "real_v2_relay_pool_e2e: assignment_resync_ok reason=${reason}"
        return
        ;;
    esac
    sleep 5
  done
  die "assigned exit endpoint did not refresh after ${reason}"
}

refresh_subscription_client_runtime() {
  local reason="$1"
  local safe_reason="${reason//[^A-Za-z0-9_.-]/_}"
  local refreshed_yaml="$TMP_DIR/subscription-client-${safe_reason}.yaml"
  local refreshed_env="$TMP_DIR/subscription-client-${safe_reason}.env"
  xrayc_real_e2e_fetch_sensitive_url_to_file "${BASE_URL}/sub/${sub_token}" "$refreshed_yaml" "subscription refresh failed after ${reason}"
  write_subscription_client_env "$refreshed_yaml" "$subscription_proxy_name" "$PH2" "$EXIT_A_PUBLIC_HOST" "$EXIT_B_PUBLIC_HOST" "$refreshed_env"
  # shellcheck disable=SC1090
  . "$refreshed_env"
  setup_client_runtime
  echo "real_v2_relay_pool_e2e: client_subscription_refresh_ok reason=${reason}"
}

mark_access_node_dirty() {
  local reason="$1"
  psql_db -Xq -v ON_ERROR_STOP=1 -v access_node_id="$access_node_id" -v reason="$reason" <<'SQL'
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = :'reason'
WHERE id = :'access_node_id'::uuid;
SQL
}

restore_test_user_runtime_state() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v user_id="$user_id" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE users SET disabled = FALSE WHERE id = :'user_id'::uuid;
UPDATE user_subscriptions
SET active = TRUE,
    expires_at = now() + interval '30 days',
    used_bytes = 0,
    limit_bytes = GREATEST(limit_bytes, 10737418240),
    updated_at = now()
WHERE user_id = :'user_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_restore_runtime_state'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

disable_test_user_for_eviction() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v user_id="$user_id" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE users SET disabled = TRUE WHERE id = :'user_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_disabled_user_eviction'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

exhaust_test_user_quota() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v user_id="$user_id" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE user_subscriptions
SET used_bytes = limit_bytes,
    updated_at = now()
WHERE user_id = :'user_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_quota_exhausted_eviction'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

disable_access_line_for_eviction() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v access_line_id="$access_line_id" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE access_lines
SET enabled = FALSE
WHERE id = :'access_line_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_access_line_disabled_eviction'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

restore_access_line_after_eviction() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v access_line_id="$access_line_id" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE access_lines
SET enabled = TRUE
WHERE id = :'access_line_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_access_line_restored'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/exit-hosts.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/pool-endpoints.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/cleanup-transit-limiter.sh"
