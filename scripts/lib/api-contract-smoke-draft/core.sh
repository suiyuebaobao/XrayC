#!/usr/bin/env bash
# 这个 helper 提供 api-contract-smoke-draft.sh 的请求封装。
# 这个 helper 提供认证 token 派生和 HTTP 状态断言函数。
# 这个 helper 提供响应字段、敏感信息和订阅 YAML 合约校验。
# 该文件仅供主脚本 source 使用，不应作为独立入口执行。

record_failure() {
  local message="$1"
  echo "api-contract-smoke: ${message}" >&2
  failures=$((failures + 1))
}

write_curl_bearer_config() {
  local token="$1"
  local config_file="$2"
  case "$token" in
    *$'\n'*|*$'\r'*)
      record_failure "authorization token must not contain newlines"
      return 1
      ;;
  esac
  local escaped="${token//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$config_file"
  chmod 600 "$config_file"
}

request_get() {
  local label="$1"
  local path="$2"
  local token="${3:-}"
  local body_file="$4"
  local status_file="$5"
  local url="${BASE_URL%/}${path}"
  local err_file="${tmp_dir}/curl-${label//[^A-Za-z0-9_.-]/_}.err"

  echo "Checking ${label}"
  if [[ -n "$token" ]]; then
    local auth_config="${tmp_dir}/curl-auth-${label//[^A-Za-z0-9_.-]/_}.conf"
    write_curl_bearer_config "$token" "$auth_config" || {
      printf '000' > "$status_file"
      return
    }
    if ! curl \
      --silent \
      --show-error \
      --location \
      --max-time "$CURL_TIMEOUT" \
      --config "$auth_config" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" > "$status_file" 2> "$err_file"; then
      record_failure "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens"
      printf '000' > "$status_file"
    fi
  else
    if ! curl \
      --silent \
      --show-error \
      --location \
      --max-time "$CURL_TIMEOUT" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" > "$status_file" 2> "$err_file"; then
      record_failure "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens"
      printf '000' > "$status_file"
    fi
  fi
}

request_json_method() {
  local label="$1"
  local method="$2"
  local path="$3"
  local token="${4:-}"
  local json_body="$5"
  local body_file="$6"
  local status_file="$7"
  local url="${BASE_URL%/}${path}"
  local err_file="${tmp_dir}/curl-${label//[^A-Za-z0-9_.-]/_}.err"
  local request_file="${tmp_dir}/request-${label//[^A-Za-z0-9_.-]/_}.json"

  echo "Checking ${label}"
  printf '%s' "$json_body" > "$request_file"
  if [[ -n "$token" ]]; then
    local auth_config="${tmp_dir}/curl-auth-${label//[^A-Za-z0-9_.-]/_}.conf"
    write_curl_bearer_config "$token" "$auth_config" || {
      printf '000' > "$status_file"
      return
    }
    if ! curl \
      --silent \
      --show-error \
      --location \
      --max-time "$CURL_TIMEOUT" \
      --request "$method" \
      --header "Content-Type: application/json" \
      --config "$auth_config" \
      --data-binary "@$request_file" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" > "$status_file" 2> "$err_file"; then
      record_failure "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens"
      printf '000' > "$status_file"
    fi
  else
    if ! curl \
      --silent \
      --show-error \
      --location \
      --max-time "$CURL_TIMEOUT" \
      --request "$method" \
      --header "Content-Type: application/json" \
      --data-binary "@$request_file" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" > "$status_file" 2> "$err_file"; then
      record_failure "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens"
      printf '000' > "$status_file"
    fi
  fi
}

request_post_json() {
  request_json_method "$1" "POST" "$2" "${3:-}" "$4" "$5" "$6"
}

request_put_json() {
  request_json_method "$1" "PUT" "$2" "${3:-}" "$4" "$5" "$6"
}

json_login_body() {
  local account="$1"
  local password="$2"
  ACCOUNT="$account" PASSWORD="$password" python3 - <<'PY'
import json
import os

print(json.dumps({
    "account": os.environ["ACCOUNT"],
    "password": os.environ["PASSWORD"],
}))
PY
}

extract_access_token() {
  local body_file="$1"
  python3 - "$body_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
token = payload.get("data", {}).get("access_token", "")
if token:
    print(token)
PY
}

derive_access_token_if_needed() {
  local kind="$1"
  local account="$2"
  local password="$3"
  local output_var="$4"
  local current_value="${!output_var:-}"

  if [[ -n "$current_value" || -z "$account" || -z "$password" ]]; then
    return
  fi

  local body="$tmp_dir/derive-${kind}-token.body"
  local status_file="$tmp_dir/derive-${kind}-token.status"
  request_post_json "derive ${kind} access token" "/api/auth/login" "" "$(json_login_body "$account" "$password")" "$body" "$status_file"
  if status_is_2xx "$(cat "$status_file")"; then
    local token
    token="$(extract_access_token "$body" || true)"
    if [[ -n "$token" ]]; then
      printf -v "$output_var" '%s' "$token"
      export "$output_var"
      echo "derive ${kind} access token: token captured"
      return
    fi
  fi
  record_failure "derive ${kind} access token: login failed or token missing; response redacted"
}

status_is_2xx() {
  [[ "$1" == 2* ]]
}

status_is_auth_reject() {
  [[ "$1" == "401" || "$1" == "403" ]]
}

status_is_missing() {
  [[ "$1" == "404" || "$1" == "405" ]]
}

assert_2xx() {
  local label="$1"
  local status="$2"
  if status_is_2xx "$status"; then
    echo "${label}: HTTP ${status}"
  else
    record_failure "${label}: expected 2xx, got HTTP ${status}"
  fi
}

assert_auth_boundary_or_missing() {
  local label="$1"
  local status="$2"
  if status_is_auth_reject "$status"; then
    echo "${label}: auth boundary present, HTTP ${status}"
  elif status_is_missing "$status" && [[ "$STRICT_CONTRACT" != "1" ]]; then
    echo "${label}: checked route missing, HTTP ${status}"
  elif status_is_missing "$status"; then
    record_failure "${label}: checked route missing in strict mode, HTTP ${status}"
  else
    record_failure "${label}: expected 401/403 or known missing route, got HTTP ${status}"
  fi
}

assert_optional_token_read() {
  local label="$1"
  local status="$2"
  if status_is_2xx "$status"; then
    echo "${label}: authenticated read available, HTTP ${status}"
  elif status_is_missing "$status" && [[ "$STRICT_CONTRACT" != "1" ]]; then
    echo "${label}: checked authenticated read missing, HTTP ${status}"
  else
    record_failure "${label}: expected 2xx read or known missing route, got HTTP ${status}"
  fi
}

assert_optional_contract_response() {
  local label="$1"
  local status="$2"
  if status_is_2xx "$status"; then
    echo "${label}: contract response available, HTTP ${status}"
    return 0
  elif status_is_auth_reject "$status"; then
    echo "${label}: auth boundary present, HTTP ${status}"
    return 1
  elif status_is_missing "$status" && [[ "$STRICT_CONTRACT" != "1" ]]; then
    echo "${label}: checked route missing, HTTP ${status}"
    return 1
  elif status_is_missing "$status"; then
    record_failure "${label}: checked route missing in strict mode, HTTP ${status}"
    return 1
  else
    record_failure "${label}: expected 2xx, 401/403 or known missing route, got HTTP ${status}"
    return 1
  fi
}

assert_no_sensitive_fields() {
  local label="$1"
  local file="$2"
  if grep -Eiq '"?(smtp_password|private[ _-]?key|secret_key|agent_token|refresh_token|access_token|payment_callback_secret|outbound_config|stream_config|probe_config|proxy_url|subscription_token)"?[[:space:]]*[:=][[:space:]]*[^[:space:]}",]+' "$file"; then
    record_failure "${label}: response appears to expose sensitive fields"
  else
    echo "${label}: sensitive field check passed"
  fi
}

assert_no_sensitive_text() {
  local label="$1"
  local file="$2"
  if grep -Eiq '(vless|trojan|ss|ssr|hysteria2|hy2|socks|socks4|socks4a|socks5|socks5h)://[^[:space:]]+' "$file"; then
    record_failure "${label}: response contains a full proxy URL"
  elif grep -Eiq 'https?://[^[:space:]'\''"]+@[^[:space:]'\''"]+' "$file"; then
    record_failure "${label}: response contains a credentialed HTTP URL"
  elif grep -Eiq '"?(outbound_proxy_url|agent_token|private[ _-]?key|secret_key|exit_endpoint|outbound_config|stream_config|probe_config|proxy_url)"?[[:space:]]*[:=][[:space:]]*[^[:space:]#]+' "$file"; then
    record_failure "${label}: response text appears to expose sensitive material"
  else
    echo "${label}: sensitive text check passed"
  fi
}

assert_public_plans_contract() {
  local label="$1"
  local file="$2"
  if python3 - "$file" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)

data = payload.get("data", payload)
if isinstance(data, dict):
    data = data.get("items", [])
if not isinstance(data, list):
    raise SystemExit("plans data is not a list")

for index, plan in enumerate(data):
    if not isinstance(plan, dict):
        continue
    if plan.get("enabled") is False:
        raise SystemExit(f"plan[{index}] is disabled")
    deleted = plan.get("is_deleted", plan.get("isDeleted"))
    if deleted is True:
        raise SystemExit(f"plan[{index}] is logically deleted")
PY
  then
    echo "${label}: public plan filter check passed"
  else
    record_failure "${label}: expected only enabled and non-deleted plans"
  fi
}

assert_admin_collection_contract() {
  local label="$1"
  local file="$2"
  local collection="$3"
  shift 3
  if python3 - "$file" "$collection" "$@" <<'PY'
import json
import sys

path, collection, *required = sys.argv[1:]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)

data = payload.get("data", payload)
items = data.get("items") if isinstance(data, dict) else None
if items is None and isinstance(data, dict):
    items = data.get(collection)
if items is None and isinstance(payload, dict):
    items = payload.get(collection)
if not isinstance(items, list):
    raise SystemExit(f"{collection} is not a list")

for index, item in enumerate(items):
    if not isinstance(item, dict):
        raise SystemExit(f"{collection}[{index}] is not an object")
    missing = [field for field in required if field not in item]
    if missing:
        raise SystemExit(f"{collection}[{index}] missing {', '.join(missing)}")
    if collection == "exit_endpoints":
        if "host_redacted" in item and not isinstance(item["host_redacted"], bool):
            raise SystemExit(f"{collection}[{index}] host_redacted is not boolean")
        for json_field in ("outbound_config", "stream_config", "probe_config"):
            if json_field in item and not isinstance(item[json_field], dict):
                raise SystemExit(f"{collection}[{index}] {json_field} is not an object")
    if collection == "exit_pools":
        for number_field in ("healthy_members", "total_members", "active_assignments"):
            if number_field in item and not isinstance(item[number_field], int):
                raise SystemExit(f"{collection}[{index}] {number_field} is not an integer")
PY
  then
    echo "${label}: ${collection} field contract check passed"
  else
    record_failure "${label}: ${collection} field contract check failed"
  fi
}

assert_operations_summary_contract() {
  local label="$1"
  local file="$2"
  if python3 - "$file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
required = [
    "active_users",
    "active_access_lines",
    "healthy_exit_pools",
    "monthly_billed_traffic_gb",
    "config_dirty_nodes",
    "access_node_count",
    "access_line_count",
    "exit_pool_count",
    "ledger_count",
    "generated_at",
    "traffic_health",
]
missing = [field for field in required if field not in data]
if missing:
    raise SystemExit(f"summary missing {', '.join(missing)}")
if "settings" in data and not isinstance(data["settings"], dict):
    raise SystemExit("summary settings is not an object")
traffic_health = data["traffic_health"]
if not isinstance(traffic_health, dict):
    raise SystemExit("summary traffic_health is not an object")
windows = traffic_health.get("windows")
if not isinstance(windows, dict):
    raise SystemExit("summary traffic_health.windows is not an object")
for window_name in ("today", "week", "month", "total"):
    window = windows.get(window_name)
    if not isinstance(window, dict):
        raise SystemExit(f"summary traffic_health.windows.{window_name} is not an object")
    for number_field in ("real_bytes", "billed_bytes"):
        if not isinstance(window.get(number_field), int):
            raise SystemExit(f"summary traffic_health.windows.{window_name}.{number_field} is not an integer")
for collection in ("line_items", "node_items", "exit_items", "group_items"):
    items = traffic_health.get(collection)
    if not isinstance(items, list):
        raise SystemExit(f"summary traffic_health.{collection} is not an array")
    for item in items:
        if not isinstance(item, dict):
            raise SystemExit(f"summary traffic_health.{collection} item is not an object")
        for number_field in ("today_real_bytes", "week_real_bytes", "month_real_bytes", "total_real_bytes", "peak_hour_real_bytes", "low_hour_real_bytes"):
            if not isinstance(item.get(number_field), int):
                raise SystemExit(f"summary traffic_health.{collection}.{number_field} is not an integer")
PY
  then
    echo "${label}: summary field contract check passed"
  else
    record_failure "${label}: summary field contract check failed"
  fi
}

assert_ledger_ranking_contract() {
  local label="$1"
  local file="$2"
  if python3 - "$file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
for field in ("source", "limit", "totals", "items"):
    if field not in data:
        raise SystemExit(f"ledger-ranking missing {field}")
if data["source"] != "usage_ledgers+usage_daily_rollups":
    raise SystemExit("ledger-ranking source is not usage_ledgers+usage_daily_rollups")
if not isinstance(data["totals"], dict):
    raise SystemExit("ledger-ranking totals is not an object")
if not isinstance(data["items"], list):
    raise SystemExit("ledger-ranking items is not a list")
for field in ("ledger_count", "delta_uplink", "delta_downlink", "real_bytes", "billed_bytes"):
    if field not in data["totals"]:
        raise SystemExit(f"ledger-ranking totals missing {field}")
for index, item in enumerate(data["items"]):
    if not isinstance(item, dict):
        raise SystemExit(f"ledger-ranking item[{index}] is not an object")
    for field in ("rank", "access_line_id", "ledger_count", "real_bytes", "billed_bytes"):
        if field not in item:
            raise SystemExit(f"ledger-ranking item[{index}] missing {field}")
PY
  then
    echo "${label}: ledger-ranking field contract check passed"
  else
    record_failure "${label}: ledger-ranking field contract check failed"
  fi
}

assert_access_operations_settings_contract() {
  local label="$1"
  local file="$2"
  if python3 - "$file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
policy = data.get("probe_policy") if isinstance(data, dict) else None
if not isinstance(policy, dict):
    raise SystemExit("probe_policy is not an object")
retention = data.get("traffic_log_retention") if isinstance(data, dict) else None
if not isinstance(retention, dict):
    raise SystemExit("traffic_log_retention is not an object")
backup = data.get("database_backup") if isinstance(data, dict) else None
if not isinstance(backup, dict):
    raise SystemExit("database_backup is not an object")
for field in (
    "exit_auto_failover_enabled",
    "exit_failure_threshold",
    "exit_recovery_threshold",
    "exit_window_minutes",
    "exit_probe_interval_seconds",
    "probe_queue_batch_size",
    "max_pending_probe_tasks",
):
    if field not in policy:
        raise SystemExit(f"probe_policy missing {field}")
ranges = {
    "exit_probe_interval_seconds": (30, 86400),
    "probe_queue_batch_size": (1, 1000),
    "max_pending_probe_tasks": (1, 1000),
}
for field, (minimum, maximum) in ranges.items():
    value = policy.get(field)
    if not isinstance(value, int) or isinstance(value, bool):
        raise SystemExit(f"probe_policy {field} is not an integer")
    if value < minimum or value > maximum:
        raise SystemExit(f"probe_policy {field} out of range")
for field in ("detail_retention_days", "prune_enabled", "delete_batch_size"):
    if field not in retention:
        raise SystemExit(f"traffic_log_retention missing {field}")
if not isinstance(retention.get("prune_enabled"), bool):
    raise SystemExit("traffic_log_retention prune_enabled is not a boolean")
retention_ranges = {
    "detail_retention_days": (1, 3650),
    "delete_batch_size": (100, 100000),
}
for field, (minimum, maximum) in retention_ranges.items():
    value = retention.get(field)
    if not isinstance(value, int) or isinstance(value, bool):
        raise SystemExit(f"traffic_log_retention {field} is not an integer")
    if value < minimum or value > maximum:
        raise SystemExit(f"traffic_log_retention {field} out of range")
for field in ("enabled", "interval_days", "retention_days"):
    if field not in backup:
        raise SystemExit(f"database_backup missing {field}")
if not isinstance(backup.get("enabled"), bool):
    raise SystemExit("database_backup enabled is not a boolean")
backup_ranges = {
    "interval_days": (1, 30),
    "retention_days": (1, 3650),
}
for field, (minimum, maximum) in backup_ranges.items():
    value = backup.get(field)
    if not isinstance(value, int) or isinstance(value, bool):
        raise SystemExit(f"database_backup {field} is not an integer")
    if value < minimum or value > maximum:
        raise SystemExit(f"database_backup {field} out of range")
PY
  then
    echo "${label}: settings field contract check passed"
  else
    record_failure "${label}: settings field contract check failed"
  fi
}

assert_subscription_yaml_contract() {
  local label="$1"
  local file="$2"
  if grep -Eq '^proxies:' "$file" \
    && grep -Eq '^proxy-groups:' "$file" \
    && grep -Eq '^rules:' "$file" \
    && grep -Eq '^[[:space:]]*-[[:space:]]+(name:|\{[[:space:]]*name:)' "$file"; then
    echo "${label}: subscription YAML shape check passed"
  else
    record_failure "${label}: expected complete Clash/mihomo YAML sections"
  fi
  assert_no_sensitive_text "$label" "$file"
}
