#!/usr/bin/env bash
# 用途：执行运行时 HTTP/API 轻量压测主入口，并加载拆分出的帮助文本。
set -euo pipefail
IFS=$'\n\t'

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/runtime-http-loadtest/usage.sh
source "$script_dir/lib/runtime-http-loadtest/usage.sh"
# shellcheck source=scripts/lib/runtime-http-loadtest/http.sh
source "$script_dir/lib/runtime-http-loadtest/http.sh"

die() {
  printf '%s\n' "$*" >&2
  exit 2
}

bool_is_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

require_positive_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

require_non_negative_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[0-9]+$ ]] || die "${name} must be a non-negative integer"
}

validate_uuid_if_set() {
  local name="$1"
  local value="${!name:-}"
  [[ -z "$value" || "$value" =~ ^[0-9A-Fa-f-]{32,36}$ ]] || die "${name} must look like a UUID"
}

split_csv() {
  local value="$1"
  local target_name="$2"
  local -n target="$target_name"
  local item
  local IFS=','

  target=()
  read -ra raw_items <<< "$value"
  for item in "${raw_items[@]}"; do
    item="${item#"${item%%[![:space:]]*}"}"
    item="${item%"${item##*[![:space:]]}"}"
    if [[ -n "$item" ]]; then
      target+=("$item")
    fi
  done
}

validate_uuid_items() {
  local name="$1"
  shift
  local value
  for value in "$@"; do
    [[ "$value" =~ ^[0-9A-Fa-f-]{32,36}$ ]] || die "${name} must contain UUID values"
  done
}

profile="${RUNTIME_HTTP_LOADTEST_PROFILE:-${PROFILE:-standard}}"
case "$profile" in
  smoke)
    default_concurrency=2
    default_requests=6
    default_agent_concurrency=2
    default_agent_requests=6
    ;;
  standard)
    default_concurrency=8
    default_requests=60
    default_agent_concurrency=4
    default_agent_requests=20
    ;;
  large)
    default_concurrency=64
    default_requests=1200
    default_agent_concurrency=8
    default_agent_requests=120
    ;;
  stress)
    default_concurrency=96
    default_requests=2400
    default_agent_concurrency=16
    default_agent_requests=240
    ;;
  *)
    die "PROFILE must be smoke, standard, large, or stress"
    ;;
esac

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  runtime_http_loadtest_usage
  exit 0
fi
if [[ "${1:-}" == "--validate-only" ]]; then
  RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY=1
  shift
fi
[[ $# -eq 0 ]] || die "unexpected arguments; use --help for usage"

BASE_URL="${BASE_URL:-}"
[[ -n "$BASE_URL" ]] || die "BASE_URL is required"
case "$BASE_URL" in
  http://*|https://*) ;;
  *) die "BASE_URL must start with http:// or https://" ;;
esac

concurrency="${RUNTIME_HTTP_LOADTEST_CONCURRENCY:-$default_concurrency}"
requests="${RUNTIME_HTTP_LOADTEST_REQUESTS:-$default_requests}"
agent_concurrency="${RUNTIME_HTTP_LOADTEST_AGENT_CONCURRENCY:-$default_agent_concurrency}"
agent_requests="${RUNTIME_HTTP_LOADTEST_AGENT_REQUESTS:-$default_agent_requests}"
case "$profile" in
  smoke) default_agent_batch_size=1 ;;
  standard) default_agent_batch_size=10 ;;
  large) default_agent_batch_size=200 ;;
  stress) default_agent_batch_size=500 ;;
esac
agent_batch_size="${RUNTIME_HTTP_LOADTEST_AGENT_BATCH_SIZE:-$default_agent_batch_size}"
connect_timeout="${RUNTIME_HTTP_LOADTEST_CONNECT_TIMEOUT_SECONDS:-5}"
request_timeout="${RUNTIME_HTTP_LOADTEST_REQUEST_TIMEOUT_SECONDS:-20}"
require_full="${RUNTIME_HTTP_LOADTEST_REQUIRE_FULL:-0}"
max_avg_ms="${RUNTIME_HTTP_LOADTEST_MAX_AVG_MS:-2000}"
max_max_ms="${RUNTIME_HTTP_LOADTEST_MAX_MAX_MS:-10000}"
agent_traffic_max_avg_ms="${RUNTIME_HTTP_LOADTEST_AGENT_TRAFFIC_MAX_AVG_MS:-3000}"

require_positive_int RUNTIME_HTTP_LOADTEST_CONCURRENCY "$concurrency"
require_positive_int RUNTIME_HTTP_LOADTEST_REQUESTS "$requests"
require_positive_int RUNTIME_HTTP_LOADTEST_AGENT_CONCURRENCY "$agent_concurrency"
require_positive_int RUNTIME_HTTP_LOADTEST_AGENT_REQUESTS "$agent_requests"
require_positive_int RUNTIME_HTTP_LOADTEST_AGENT_BATCH_SIZE "$agent_batch_size"
require_positive_int RUNTIME_HTTP_LOADTEST_CONNECT_TIMEOUT_SECONDS "$connect_timeout"
require_positive_int RUNTIME_HTTP_LOADTEST_REQUEST_TIMEOUT_SECONDS "$request_timeout"
require_non_negative_int RUNTIME_HTTP_LOADTEST_MAX_AVG_MS "$max_avg_ms"
require_non_negative_int RUNTIME_HTTP_LOADTEST_MAX_MAX_MS "$max_max_ms"
require_non_negative_int RUNTIME_HTTP_LOADTEST_AGENT_TRAFFIC_MAX_AVG_MS "$agent_traffic_max_avg_ms"
validate_uuid_if_set ACCESS_NODE_ID
validate_uuid_if_set ACCESS_LINE_ID
validate_uuid_if_set EXIT_ENDPOINT_ID

split_csv "${USER_ACCESS_TOKENS:-${USER_ACCESS_TOKEN:-}}" user_access_tokens
split_csv "${ACCESS_NODE_IDS:-${ACCESS_NODE_ID:-}}" access_node_ids
split_csv "${ACCESS_LINE_IDS:-${ACCESS_LINE_ID:-}}" access_line_ids
split_csv "${EXIT_ENDPOINT_IDS:-${EXIT_ENDPOINT_ID:-}}" exit_endpoint_ids
validate_uuid_items ACCESS_NODE_IDS "${access_node_ids[@]}"
validate_uuid_items ACCESS_LINE_IDS "${access_line_ids[@]}"
validate_uuid_items EXIT_ENDPOINT_IDS "${exit_endpoint_ids[@]}"

if [[ -n "${AGENT_TOKEN:-}" || -n "${ACCESS_NODE_ID:-}" || -n "${ACCESS_LINE_ID:-}" || -n "${EXIT_ENDPOINT_ID:-}" ]]; then
  [[ -n "${AGENT_TOKEN:-}" && -n "${ACCESS_NODE_ID:-}" && -n "${ACCESS_LINE_ID:-}" ]] || \
    die "AGENT_TOKEN, ACCESS_NODE_ID, and ACCESS_LINE_ID are required together for agent stages"
fi

if bool_is_true "$require_full" && bool_is_true "${RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY:-0}"; then
  die "RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY must be disabled when RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1"
fi

if bool_is_true "${RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY:-0}"; then
  printf 'runtime http loadtest validation passed: profile=%s concurrency=%s requests=%s agent_concurrency=%s agent_requests=%s agent_batch_size=%s require_full=%s max_avg_ms=%s max_max_ms=%s agent_traffic_max_avg_ms=%s\n' \
    "$profile" "$concurrency" "$requests" "$agent_concurrency" "$agent_requests" "$agent_batch_size" "$require_full" "$max_avg_ms" "$max_max_ms" "$agent_traffic_max_avg_ms"
  exit 0
fi

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v awk >/dev/null 2>&1 || die "awk is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

base_url="${BASE_URL%/}"
now_unix="$(date +%s)"
agent_metrics_payload="$tmp_dir/agent-metrics.json"
agent_sessions_payload="$tmp_dir/agent-sessions.json"
agent_probes_payload="$tmp_dir/agent-probes.json"
agent_heartbeat_payload="$tmp_dir/agent-heartbeat.json"
agent_config_result_payload="$tmp_dir/agent-config-result.json"
agent_traffic_payload="$tmp_dir/agent-traffic.json"

if [[ -n "${AGENT_TOKEN:-}" ]]; then
  cat > "$agent_heartbeat_payload" <<JSON
{"access_node_id":"${ACCESS_NODE_ID}"}
JSON
  cat > "$agent_config_result_payload" <<JSON
{"access_node_id":"${ACCESS_NODE_ID}","success":true,"config_version":"runtime-http-loadtest"}
JSON
  ACCESS_NODE_ID_VALUE="$ACCESS_NODE_ID" \
  ACCESS_LINE_ID_VALUE="$ACCESS_LINE_ID" \
  EXIT_ENDPOINT_ID_VALUE="${EXIT_ENDPOINT_ID:-}" \
  XRAY_USER_KEY_VALUE="${XRAY_USER_KEY:-}" \
  NOW_UNIX_VALUE="$now_unix" \
  AGENT_BATCH_SIZE_VALUE="$agent_batch_size" \
  AGENT_METRICS_PAYLOAD="$agent_metrics_payload" \
  AGENT_SESSIONS_PAYLOAD="$agent_sessions_payload" \
  AGENT_PROBES_PAYLOAD="$agent_probes_payload" \
  AGENT_TRAFFIC_PAYLOAD="$agent_traffic_payload" \
  python3 - <<'PY'
import hashlib
import json
import os

access_node_id = os.environ["ACCESS_NODE_ID_VALUE"]
access_line_id = os.environ["ACCESS_LINE_ID_VALUE"]
exit_endpoint_id = os.environ.get("EXIT_ENDPOINT_ID_VALUE", "")
xray_user_key = os.environ.get("XRAY_USER_KEY_VALUE", "")
now_unix = int(os.environ["NOW_UNIX_VALUE"])
batch_size = int(os.environ["AGENT_BATCH_SIZE_VALUE"])

metrics = []
sessions = []
line_probes = []
exit_probes = []
snapshots = []
for index in range(batch_size):
    ts = now_unix + index
    metrics.append({
        "access_line_id": access_line_id,
        "online_users": min(batch_size, index + 1),
        "active_connections": index + 1,
        "unique_client_ips": min(batch_size, index + 1),
        "uplink_rate_bps": 1024 * (index + 1),
        "downlink_rate_bps": 2048 * (index + 1),
        "collected_at_unix": ts,
    })
    digest = hashlib.sha256(f"runtime-http-loadtest-{index}".encode()).hexdigest()
    sessions.append({
        "access_line_id": access_line_id,
        "xray_user_key": f"runtime-http-loadtest-{index}",
        "client_ip_hash": f"sha256:{digest}",
        "started_at_unix": now_unix,
        "last_seen_at_unix": ts,
    })
    line_probes.append({
        "access_line_id": access_line_id,
        "status": "healthy",
        "latency_ms": 1 + (index % 50),
        "probed_at_unix": ts,
    })
    if exit_endpoint_id:
        exit_probes.append({
            "exit_endpoint_id": exit_endpoint_id,
            "status": "healthy",
            "latency_ms": 1 + (index % 50),
            "probed_at_unix": ts,
        })
    if xray_user_key:
        snapshots.append({
            "access_line_id": access_line_id,
            "xray_user_key": xray_user_key,
            "uplink_bytes": 1024 * 1024 * (index + 1),
            "downlink_bytes": 2 * 1024 * 1024 * (index + 1),
            "captured_at_unix": ts,
        })

payloads = {
    os.environ["AGENT_METRICS_PAYLOAD"]: {"access_node_id": access_node_id, "metrics": metrics},
    os.environ["AGENT_SESSIONS_PAYLOAD"]: {"access_node_id": access_node_id, "sessions": sessions},
    os.environ["AGENT_PROBES_PAYLOAD"]: {
        "access_node_id": access_node_id,
        "line_probes": line_probes,
        "exit_probes": exit_probes,
    },
}
if xray_user_key:
    payloads[os.environ["AGENT_TRAFFIC_PAYLOAD"]] = {
        "access_node_id": access_node_id,
        "snapshots": snapshots,
    }
for path, payload in payloads.items():
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, separators=(",", ":"))
PY
fi

request_once() {
  local method="$1"
  local path="$2"
  local token="$3"
  local payload_file="$4"
  local output
  local args=(
    -sS
    -o /dev/null
    -w '%{http_code} %{time_total}\n'
    --connect-timeout "$connect_timeout"
    --max-time "$request_timeout"
    -X "$method"
  )

  if [[ -n "$token" ]]; then
    local config_file="$tmp_dir/curl-auth-${BASHPID}-${RANDOM}.conf"
    write_curl_bearer_config "$token" "$config_file"
    args+=(--config "$config_file")
  fi
  if [[ -n "$payload_file" ]]; then
    args+=(-H "Content-Type: application/json" --data-binary "@${payload_file}")
  fi

  output="$(curl "${args[@]}" "${base_url}${path}" 2>/dev/null || true)"
  if [[ "$output" =~ ^[0-9]{3}[[:space:]][0-9] ]]; then
    printf '%s\n' "$output"
  else
    printf '000 0\n'
  fi
}

print_summary() {
  local stage="$1"
  local result_file="$2"
  local stage_max_avg_ms="$max_avg_ms"
  if [[ "$stage" == "agent-traffic" ]]; then
    stage_max_avg_ms="$agent_traffic_max_avg_ms"
  fi
  awk -v stage="$stage" -v max_avg_ms="$stage_max_avg_ms" -v max_max_ms="$max_max_ms" '
    {
      count += 1
      if ($1 ~ /^2/) ok += 1
      else failed += 1
      ms = $2 * 1000
      if (count == 1 || ms < min) min = ms
      if (count == 1 || ms > max) max = ms
      sum += ms
    }
    END {
      if (count == 0) {
        printf "%s: requests=0 ok=0 failed=0 latency_ms min=0.0 avg=0.0 max=0.0\n", stage
        exit 1
      }
      printf "%s: requests=%d ok=%d failed=%d latency_ms min=%.1f avg=%.1f max=%.1f\n", stage, count, ok, failed, min, sum / count, max
      if (failed > 0) exit 1
      if (max_avg_ms > 0 && (sum / count) > max_avg_ms) exit 1
      if (max_max_ms > 0 && max > max_max_ms) exit 1
      exit 0
    }
  ' "$result_file"
}

skip_stage() {
  local stage="$1"
  printf '%s: skipped requests=0 ok=0 failed=0 latency_ms min=0.0 avg=0.0 max=0.0\n' "$stage"
  ! bool_is_true "$require_full"
}

run_stage() {
  local stage="$1"
  local method="$2"
  local path="$3"
  local token="${4:-}"
  local payload_file="${5:-}"
  local stage_requests="${6:-$requests}"
  local stage_concurrency="${7:-$concurrency}"
  local result_file="$tmp_dir/${stage}.results"
  local active=0
  local index

  : > "$result_file"
  for index in $(seq 1 "$stage_requests"); do
    request_once "$method" "$path" "$token" "$payload_file" >> "$result_file" &
    active=$((active + 1))
    if [[ "$active" -ge "$stage_concurrency" ]]; then
      wait -n || true
      active=$((active - 1))
    fi
  done
  while [[ "$active" -gt 0 ]]; do
    wait -n || true
    active=$((active - 1))
  done

  local completed
  completed="$(wc -l <"$result_file" | tr -d '[:space:]')"
  while [[ "$completed" -lt "$stage_requests" ]]; do
    request_once "$method" "$path" "$token" "$payload_file" >> "$result_file"
    completed=$((completed + 1))
  done

  print_summary "$stage" "$result_file"
}

run_stage_matrix() {
  local stage="$1"
  local method="$2"
  local token_array_name="$3"
  local path_array_name="$4"
  local result_file="$tmp_dir/${stage}.results"
  local active=0
  local index
  local token
  local path
  local -n tokens="$token_array_name"
  local -n paths="$path_array_name"

  if [[ "${#tokens[@]}" -eq 0 || "${#paths[@]}" -eq 0 ]]; then
    skip_stage "$stage"
    return
  fi

  : > "$result_file"
  for index in $(seq 1 "$requests"); do
    token="${tokens[$(((index - 1) % ${#tokens[@]}))]}"
    path="${paths[$(((index - 1) % ${#paths[@]}))]}"
    request_once "$method" "$path" "$token" "" >> "$result_file" &
    active=$((active + 1))
    if [[ "$active" -ge "$concurrency" ]]; then
      wait -n || true
      active=$((active - 1))
    fi
  done
  while [[ "$active" -gt 0 ]]; do
    wait -n || true
    active=$((active - 1))
  done

  local completed
  completed="$(wc -l <"$result_file" | tr -d '[:space:]')"
  while [[ "$completed" -lt "$requests" ]]; do
    index=$((completed + 1))
    token="${tokens[$(((index - 1) % ${#tokens[@]}))]}"
    path="${paths[$(((index - 1) % ${#paths[@]}))]}"
    request_once "$method" "$path" "$token" "" >> "$result_file"
    completed=$((completed + 1))
  done

  print_summary "$stage" "$result_file"
}

failure_count=0
run_stage "health" "GET" "/health" "" "" || failure_count=$((failure_count + 1))
run_stage "plans" "GET" "/api/plans" "" "" || failure_count=$((failure_count + 1))

if [[ "${#user_access_tokens[@]}" -gt 0 ]]; then
  user_subscription_paths=("/api/user/subscription")
  user_usage_paths=("/api/user/usage")
  run_stage_matrix "user-subscription" "GET" user_access_tokens user_subscription_paths || failure_count=$((failure_count + 1))
  run_stage_matrix "user-usage" "GET" user_access_tokens user_usage_paths || failure_count=$((failure_count + 1))
else
  skip_stage "user-authenticated" || failure_count=$((failure_count + 1))
fi

if [[ -n "${ADMIN_ACCESS_TOKEN:-}" ]]; then
  admin_tokens=("$ADMIN_ACCESS_TOKEN")
  admin_runtime_paths=(
    "/api/admin/access-operations/summary"
    "/api/admin/access-operations/ledger-ranking?limit=50"
    "/api/admin/audit-logs?page=1&page_size=50"
    "/api/admin/audit-logs?page=10&page_size=50"
    "/api/admin/access-routing"
    "/api/admin/access-nodes"
    "/api/admin/access-lines"
    "/api/admin/exit-endpoints"
  )
  for access_line_id in "${access_line_ids[@]}"; do
    admin_runtime_paths+=("/api/admin/access-lines/${access_line_id}/metrics")
    admin_runtime_paths+=("/api/admin/access-lines/${access_line_id}/sessions")
  done
  run_stage_matrix "admin-runtime-reads" "GET" admin_tokens admin_runtime_paths || failure_count=$((failure_count + 1))
else
  skip_stage "admin-runtime-reads" || failure_count=$((failure_count + 1))
fi

if [[ -n "${AGENT_TOKEN:-}" ]]; then
  run_stage "agent-heartbeat" "POST" "/api/agent/access/heartbeat" "$AGENT_TOKEN" "$agent_heartbeat_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  run_stage "agent-config-result" "POST" "/api/agent/access/config-result" "$AGENT_TOKEN" "$agent_config_result_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  run_stage "agent-metrics" "POST" "/api/agent/access/metrics" "$AGENT_TOKEN" "$agent_metrics_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  run_stage "agent-sessions" "POST" "/api/agent/access/sessions" "$AGENT_TOKEN" "$agent_sessions_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  run_stage "agent-probes" "POST" "/api/agent/access/probes" "$AGENT_TOKEN" "$agent_probes_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  if [[ -n "${XRAY_USER_KEY:-}" ]]; then
    run_stage "agent-traffic" "POST" "/api/agent/access/traffic" "$AGENT_TOKEN" "$agent_traffic_payload" "$agent_requests" "$agent_concurrency" || failure_count=$((failure_count + 1))
  else
    skip_stage "agent-traffic" || failure_count=$((failure_count + 1))
  fi
else
  skip_stage "agent-runtime" || failure_count=$((failure_count + 1))
fi

if [[ "$failure_count" -gt 0 ]]; then
  printf 'runtime http loadtest completed with failed stages=%s\n' "$failure_count" >&2
  exit 1
fi

printf 'runtime http loadtest completed: profile=%s concurrency=%s requests_per_stage=%s agent_concurrency=%s agent_requests_per_stage=%s agent_batch_size=%s\n' \
  "$profile" "$concurrency" "$requests" "$agent_concurrency" "$agent_requests" "$agent_batch_size"
