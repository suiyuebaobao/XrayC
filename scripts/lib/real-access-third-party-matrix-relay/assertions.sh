#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的数据库断言、订阅轮询和协议路由校验函数。
# 负责强制出口分配、账本归因检查、订阅暴露/回收等待和禁用/配额驱逐闭环。
# 调试函数只输出脱敏后的状态摘要，避免泄露第三方 endpoint、凭据、IP 或 UUID。
# 本文件依赖 common.sh、remote.sh 和 client.sh 已经按顺序加载。

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

force_user_exit_assignment() {
  local endpoint_id="$1"
  local reason="$2"
  psql_db -Xq -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" \
    -v access_line_id="$access_line_id" \
    -v pool_id="$pool_id" \
    -v endpoint_id="$endpoint_id" \
    -v reason="$reason" <<'SQL'
	BEGIN;
	UPDATE access_lines
	SET exit_endpoint_id = :'endpoint_id'::uuid
	WHERE id = :'access_line_id'::uuid;
	UPDATE exit_resources r
SET enabled = TRUE,
    status = 'healthy',
    last_probe_status = 'healthy',
    last_probe_at = now()
FROM exit_endpoints e
WHERE e.id = :'endpoint_id'::uuid
  AND e.exit_resource_id = r.id;
UPDATE exit_endpoints
SET enabled = TRUE,
    last_probe_status = 'healthy',
    last_probe_at = now()
WHERE id = :'endpoint_id'::uuid;
	UPDATE exit_pool_members
	SET status = CASE WHEN exit_endpoint_id = :'endpoint_id'::uuid THEN 'healthy' ELSE 'offline' END,
    allow_new_assignments = CASE WHEN exit_endpoint_id = :'endpoint_id'::uuid THEN TRUE ELSE FALSE END,
    weight = CASE WHEN exit_endpoint_id = :'endpoint_id'::uuid THEN 1000 ELSE 1 END,
    priority = CASE WHEN exit_endpoint_id = :'endpoint_id'::uuid THEN 1000 ELSE 1 END,
    updated_at = now()
WHERE exit_pool_id = :'pool_id'::uuid;
INSERT INTO access_exit_probe_states (
    access_node_id, exit_endpoint_id, effective_status,
    consecutive_failures, consecutive_successes, last_probe_status,
    last_error_summary, last_probe_at, status_changed_at, updated_at
)
VALUES (
    (SELECT access_node_id FROM access_lines WHERE id = :'access_line_id'::uuid),
    :'endpoint_id'::uuid,
    'healthy',
    0,
    1,
    'success',
    '',
    now(),
    now(),
    now()
)
ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
    effective_status = 'healthy',
    consecutive_failures = 0,
    consecutive_successes = 1,
    last_probe_status = 'success',
    last_error_summary = '',
    last_probe_at = now(),
    status_changed_at = now(),
    updated_at = now();
INSERT INTO user_exit_assignments (
    user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
VALUES (
    :'user_id'::uuid,
    :'access_line_id'::uuid,
    :'pool_id'::uuid,
    :'endpoint_id'::uuid,
    :'reason'
)
ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
    failover_reason = EXCLUDED.failover_reason,
    assigned_at = now();
COMMIT;
SQL
  mark_access_node_dirty "$reason"
}

ledger_sum_for_endpoint() {
  local endpoint_id="$1"
  psql_db -XAtq -v ON_ERROR_STOP=1 \
    -v access_line_id="$access_line_id" \
    -v user_id="$user_id" \
    -v endpoint_id="$endpoint_id" <<'SQL'
SELECT COALESCE(SUM(billed_bytes), 0)::bigint
FROM usage_ledgers
WHERE traffic_source = 'access_line'
  AND access_line_id = :'access_line_id'::uuid
  AND user_id = :'user_id'::uuid
  AND exit_endpoint_id = :'endpoint_id'::uuid;
SQL
}

subscription_used_bytes_for_user() {
  psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT COALESCE(used_bytes, 0)::bigint
FROM user_subscriptions
WHERE user_id = :'user_id'::uuid
LIMIT 1;
SQL
}

access_node_dirty_state() {
  psql_db -XAtq -v ON_ERROR_STOP=1 -v access_node_id="$access_node_id" <<'SQL'
SELECT config_dirty::text FROM access_nodes WHERE id = :'access_node_id'::uuid LIMIT 1;
SQL
}

assignment_matches_endpoint() {
  local endpoint_id="$1"
  local assigned=""
  assigned="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" \
    -v access_line_id="$access_line_id" \
    -v pool_id="$pool_id" <<'SQL'
SELECT exit_endpoint_id::text
FROM user_exit_assignments
WHERE user_id = :'user_id'::uuid
  AND access_line_id = :'access_line_id'::uuid
  AND exit_pool_id = :'pool_id'::uuid
LIMIT 1;
SQL
)"
  [[ "$assigned" == "$endpoint_id" ]] && printf 'true' || printf 'false'
}

dump_protocol_debug() {
  local protocol="$1"
  local endpoint_id="$2"
  local expected_ip="$3"
  local before="$4"
  local actual_ip="${5:-}"
  local subscription_before="${6:-}"
  local current=""
  local subscription_current=""
  current="$(ledger_sum_for_endpoint "$endpoint_id" || printf '0')"
  subscription_current="$(subscription_used_bytes_for_user || printf '0')"
  status "protocol=${protocol} debug actual_empty=$([[ -z "$actual_ip" ]] && printf true || printf false) egress_matches_expected=$([[ "$actual_ip" == "$expected_ip" ]] && printf true || printf false) ledger_before=${before:-0} ledger_current=${current:-0} ledger_increased=$([[ "$current" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ && "$current" -gt "$before" ]] && printf true || printf false) subscription_before=${subscription_before:-0} subscription_current=${subscription_current:-0} subscription_billed=$([[ "$subscription_current" =~ ^[0-9]+$ && "$subscription_before" =~ ^[0-9]+$ && "$subscription_current" -gt "$subscription_before" ]] && printf true || printf false) assignment_matches=$(assignment_matches_endpoint "$endpoint_id") access_node_dirty=$(access_node_dirty_state || printf unknown)"
  dump_remote_relay_debug
  if declare -F remote_exec >/dev/null 2>&1; then
    remote_exec 1 '
      echo "client_debug: containers"
      docker ps -a --filter name=xrayc-real-matrix-relay-client --format "{{.Names}} {{.Status}}" || true
      echo "client_debug: listeners"
      ss -ltnp | grep -E ":(32080) " || true
      echo "client_debug: logs"
      docker logs --tail 60 xrayc-real-matrix-relay-client 2>&1 || true
    ' 2>/dev/null \
      | sed -E 's#(https?|socks5?|vless|trojan|ss|hysteria2?)://[^[:space:]]+#<proxy-url>#Ig; s#//[^/@[:space:]]+:[^/@[:space:]]+@#//<userinfo>@#g; s/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g; s/[0-9a-fA-F-]{36}/<uuid>/g; s/(token|password|secret|key|uuid)[=:][^ ]+/\1=<redacted>/Ig' \
      || true
  fi
}

assert_private_matrix_material_redacted() {
  local file="$1"
  local proxy_name="${2:-}"
  local allowed_ingress_host="${PH2:-}"
  local allowed_ingress_port="${LISTEN_PORT:-}"
  local name value
  local literal_file=""
  local sensitive_names=(
    THIRD_PARTY_SOCKS_HOST THIRD_PARTY_SOCKS_RAW_URL THIRD_PARTY_SOCKS_PASSWORD THIRD_PARTY_SOCKS_KEY
    THIRD_PARTY_HTTP_HOST THIRD_PARTY_HTTP_RAW_URL THIRD_PARTY_HTTP_PASSWORD THIRD_PARTY_HTTP_KEY
    THIRD_PARTY_HOST THIRD_PARTY_VLESS_HOST THIRD_PARTY_VLESS_RAW_URL THIRD_PARTY_UUID THIRD_PARTY_VLESS_UUID
    THIRD_PARTY_VLESS_SECURITY THIRD_PARTY_VLESS_SERVER_NAME THIRD_PARTY_VLESS_SHORT_ID THIRD_PARTY_VLESS_FINGERPRINT
    THIRD_PARTY_PUBLIC_KEY THIRD_PARTY_VLESS_PUBLIC_KEY THIRD_PARTY_VLESS_KEY
    THIRD_PARTY_TROJAN_HOST THIRD_PARTY_TROJAN_RAW_URL THIRD_PARTY_TROJAN_PASSWORD THIRD_PARTY_TROJAN_SNI THIRD_PARTY_TROJAN_KEY
    THIRD_PARTY_SHADOWSOCKS_HOST THIRD_PARTY_SHADOWSOCKS_RAW_URL THIRD_PARTY_SHADOWSOCKS_PASSWORD THIRD_PARTY_SHADOWSOCKS_KEY
    THIRD_PARTY_HY2_HOST THIRD_PARTY_HY2_RAW_URL THIRD_PARTY_HY2_PASSWORD THIRD_PARTY_HY2_AUTH THIRD_PARTY_HY2_OBFS_PASSWORD THIRD_PARTY_HY2_KEY
    THIRD_PARTY_HY2_SERVER_NAME THIRD_PARTY_HY2_SNI
  )
  if [[ -n "$proxy_name" ]]; then
    literal_file="$TMP_DIR/private-matrix-literals-${RANDOM}.txt"
    : > "$literal_file"
    for name in "${sensitive_names[@]}"; do
      value="${!name:-}"
      if [[ -n "$value" ]] && ! value_is_placeholder "$value" && ! matrix_literal_is_low_signal "$value"; then
        printf '%s\n' "$value" >> "$literal_file"
      fi
    done
    assert_named_proxy_missing_literals "$file" "$literal_file" "$proxy_name" "subscription leaked private matrix material" "$allowed_ingress_host" "$allowed_ingress_port"
    return 0
  fi
  for name in "${sensitive_names[@]}"; do
    value="${!name:-}"
    if [[ -n "$value" ]] && ! value_is_placeholder "$value" && ! matrix_literal_is_low_signal "$value"; then
      xrayc_real_e2e_assert_subscription_missing_literal "$file" "$value" "subscription leaked private matrix material"
    fi
  done
}

matrix_literal_is_low_signal() {
  local value="${1:-}"
  value="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  case "$value" in
    real|tcp|udp|tls|reality|chrome|auto|stream-one|true|false|none)
      return 0
      ;;
  esac
  return 1
}

assert_named_proxy_missing_literals() {
  local file="$1"
  local literal_file="$2"
  local proxy_name="$3"
  local failure_message="$4"
  local allowed_ingress_host="${5:-}"
  local allowed_ingress_port="${6:-}"
  python3 - "$file" "$literal_file" "$proxy_name" "$allowed_ingress_host" "$allowed_ingress_port" <<'PY' || die "$failure_message"
import json
import sys

import yaml

content_file, literal_file, proxy_name, allowed_ingress_host, allowed_ingress_port = sys.argv[1:6]
with open(content_file, "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)
with open(literal_file, "r", encoding="utf-8") as fh:
    literals = [line.rstrip("\n") for line in fh if len(line.rstrip("\n")) >= 4]

def walk(value):
    if isinstance(value, dict):
        name = str(value.get("name", "")).strip()
        line_group_name = str(value.get("line_group_name", "")).strip()
        if proxy_name in name or proxy_name in line_group_name:
            yield value
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)

matches = list(walk(payload))
if not matches:
    raise SystemExit(1)
literals = set(literals)

def scalars(value, path=""):
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = f"{path}.{key}" if path else str(key)
            yield from scalars(child, child_path)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from scalars(child, f"{path}[{index}]")
    elif value is not None:
        yield path, str(value)

def is_allowed_ingress_server(item, path, value):
    if not allowed_ingress_host or not allowed_ingress_port:
        return False
    if path == "server":
        server = str(item.get("server", "")).strip() if isinstance(item, dict) else ""
        port = str(item.get("port", "")).strip() if isinstance(item, dict) else ""
        return (
            value == allowed_ingress_host
            and server == allowed_ingress_host
            and port == allowed_ingress_port
        )
    if path == "listen_host":
        server = str(item.get("listen_host", "")).strip() if isinstance(item, dict) else ""
        port = str(item.get("listen_port", "")).strip() if isinstance(item, dict) else ""
        return (
            value == allowed_ingress_host
            and server == allowed_ingress_host
            and port == allowed_ingress_port
        )
    return False

for item in matches:
    for path, value in scalars(item):
        if value in literals and not is_allowed_ingress_server(item, path, value):
            raise SystemExit(1)
PY
}

subscription_line_present() {
  local config_file="$TMP_DIR/subscription-poll-${RANDOM}.conf"
  local body_file="$TMP_DIR/subscription-poll-${RANDOM}.yaml"
  local status=""
  xrayc_real_e2e_write_curl_url_config "${BASE_URL}/sub/${sub_token}" "$config_file"
  status="$(curl --silent --location --max-time 20 --output "$body_file" --write-out '%{http_code}' --config "$config_file" 2>/dev/null || true)"
  [[ "$status" == 2* ]] && grep -Fq "$subscription_proxy_name" "$body_file"
}

wait_subscription_line_absent() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if ! subscription_line_present; then
      status "subscription_eviction_ok reason=${reason}"
      return 0
    fi
    sleep 5
  done
  die "subscription still exposes relay line after ${reason}"
}

wait_subscription_line_present() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if subscription_line_present; then
      status "subscription_restore_ok reason=${reason}"
      return 0
    fi
    sleep 5
  done
  die "subscription did not restore relay line after ${reason}"
}

wait_access_node_applied_config() {
  local reason="$1"
  local state=""
  for _ in $(seq 1 90); do
    state="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v access_node_id="$access_node_id" <<'SQL'
SELECT CASE
  WHEN config_dirty = FALSE
   AND last_heartbeat_at IS NOT NULL
   AND last_heartbeat_at >= now() - interval '120 seconds'
   AND COALESCE(desired_config_hash, '') <> ''
   AND COALESCE(applied_config_hash, '') = COALESCE(desired_config_hash, '')
  THEN 'ok'
  ELSE 'wait'
END
FROM access_nodes
WHERE id = :'access_node_id'::uuid
LIMIT 1;
SQL
)"
    if [[ "$state" == "ok" ]]; then
      status "access_node_config_applied reason=${reason}"
      return 0
    fi
    sleep 2
  done
  dump_remote_relay_debug
  die "access node config was not applied after ${reason}"
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
    config_dirty_reason = 'real_matrix_relay_restore_runtime_state'
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
    config_dirty_reason = 'real_matrix_relay_disabled_user_eviction'
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
    config_dirty_reason = 'real_matrix_relay_quota_exhausted_eviction'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

revoke_test_subscription_token_for_eviction() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v sub_token="$sub_token" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE subscription_tokens
SET revoked_at = now(),
    last_used_at = NULL
WHERE token = :'sub_token';
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_matrix_relay_subscription_token_revoked'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

restore_test_subscription_token_for_eviction() {
  psql_db -Xq -v ON_ERROR_STOP=1 -v sub_token="$sub_token" -v access_node_id="$access_node_id" <<'SQL'
BEGIN;
UPDATE subscription_tokens
SET revoked_at = NULL,
    expires_at = now() + interval '30 days',
    last_used_at = NULL
WHERE token = :'sub_token';
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_matrix_relay_subscription_token_restored'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

wait_protocol_route_and_ledger() {
  local protocol="$1"
  local endpoint_id="$2"
  local expected_ip="$3"
  local before="$4"
  local interval="${LEDGER_POLL_INTERVAL_SECONDS:-5}"
  local timeout="${LEDGER_POLL_SECONDS:-150}"
  local deadline now
  local actual_ip=""
  local current=""
  local subscription_before=""
  local subscription_current=""
  subscription_before="$(subscription_used_bytes_for_user)"
  deadline="$(($(date +%s) + timeout))"
  while [[ "$(date +%s)" -le "$deadline" ]]; do
    actual_ip="$(client_public_ip || true)"
    if [[ "$actual_ip" == "$expected_ip" ]]; then
      current="$(ledger_sum_for_endpoint "$endpoint_id")"
      subscription_current="$(subscription_used_bytes_for_user)"
      if [[ "$current" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ && "$current" -gt "$before" && "$subscription_current" =~ ^[0-9]+$ && "$subscription_before" =~ ^[0-9]+$ && "$subscription_current" -gt "$subscription_before" ]]; then
        status "protocol=${protocol} egress_ledger_and_billing_ok"
        return 0
      fi
    fi
    now="$(date +%s)"
    [[ "$now" -ge "$deadline" ]] && break
    sleep "$interval"
  done
  dump_protocol_debug "$protocol" "$endpoint_id" "$expected_ip" "$before" "$actual_ip" "$subscription_before"
  die "protocol=${protocol} did not produce expected egress, ledger attribution, and subscription billing"
}

assert_protocol_eviction_cycle() {
  local protocol="$1"
  local endpoint_id="$2"
  local expected_ip="$3"
  local before_restore=""

  disable_test_user_for_eviction
  wait_access_node_applied_config "disabled-user-${protocol}"
  wait_subscription_line_absent "disabled-user-${protocol}"
  wait_client_blocked "disabled-user-${protocol}"
  restore_test_user_runtime_state
  wait_access_node_applied_config "restore-disabled-user-${protocol}"
  wait_subscription_line_present "disabled-user-${protocol}"
  before_restore="$(ledger_sum_for_endpoint "$endpoint_id")"
  wait_protocol_route_and_ledger "${protocol}-restore-disabled" "$endpoint_id" "$expected_ip" "$before_restore"

  exhaust_test_user_quota
  wait_access_node_applied_config "quota-exhausted-${protocol}"
  wait_subscription_line_absent "quota-exhausted-${protocol}"
  wait_client_blocked "quota-exhausted-${protocol}"
  restore_test_user_runtime_state
  wait_access_node_applied_config "restore-quota-exhausted-${protocol}"
  wait_subscription_line_present "quota-exhausted-${protocol}"
  before_restore="$(ledger_sum_for_endpoint "$endpoint_id")"
  wait_protocol_route_and_ledger "${protocol}-restore-quota" "$endpoint_id" "$expected_ip" "$before_restore"

  revoke_test_subscription_token_for_eviction
  wait_access_node_applied_config "subscription-token-revoked-${protocol}"
  wait_subscription_line_absent "subscription-token-revoked-${protocol}"
  wait_client_blocked "subscription-token-revoked-${protocol}"
  restore_test_subscription_token_for_eviction
  wait_access_node_applied_config "restore-subscription-token-${protocol}"
  wait_subscription_line_present "subscription-token-revoked-${protocol}"
  before_restore="$(ledger_sum_for_endpoint "$endpoint_id")"
  wait_protocol_route_and_ledger "${protocol}-restore-token" "$endpoint_id" "$expected_ip" "$before_restore"
  status "protocol=${protocol} eviction_cycle_ok"
}
