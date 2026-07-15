#!/usr/bin/env bash
# 用途：真实运行时稳定性 UAT 主入口，负责环境加载、校验和阶段编排。
# 说明：具体检查函数拆分到 scripts/lib/real-runtime-stability-uat/functions.sh。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

# shellcheck source=lib/real-runtime-stability-uat/functions.sh
source "${SCRIPT_DIR}/lib/real-runtime-stability-uat/functions.sh"

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

if [[ -n "${XRAYC_REAL_RUNTIME_FORCE_PROFILE:-}" ]]; then
  UAT_PROFILE="$XRAYC_REAL_RUNTIME_FORCE_PROFILE"
fi
if [[ -n "${XRAYC_REAL_RUNTIME_FORCE_DURATION_SECONDS:-}" ]]; then
  UAT_DURATION_SECONDS="$XRAYC_REAL_RUNTIME_FORCE_DURATION_SECONDS"
fi
if [[ -n "${XRAYC_REAL_RUNTIME_FORCE_VALIDATE_ONLY:-}" ]]; then
  UAT_VALIDATE_ONLY="$XRAYC_REAL_RUNTIME_FORCE_VALIDATE_ONLY"
fi

BASE_URL="${BASE_URL:-}"
DATABASE_URL="${DATABASE_URL:-}"
SUB_TOKEN="${SUB_TOKEN:-}"
SUBSCRIPTION_URL="${SUBSCRIPTION_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
ACCESS_NODE_ID="${ACCESS_NODE_ID:-}"
ACCESS_LINE_ID="${ACCESS_LINE_ID:-}"
EXIT_ENDPOINT_ID="${EXIT_ENDPOINT_ID:-}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"
AGENT_TOKEN="${AGENT_TOKEN:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
UAT_CLIENT_TRAFFIC_URL="${UAT_CLIENT_TRAFFIC_URL:-https://speed.cloudflare.com/__down?bytes=1048576}"
UAT_CLIENT_TRAFFIC_RATE="${UAT_CLIENT_TRAFFIC_RATE:-64k}"
UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS="${UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS:-25}"
UAT_POST_TRAFFIC_SETTLE_SECONDS="${UAT_POST_TRAFFIC_SETTLE_SECONDS:-5}"
UAT_PROFILE="${UAT_PROFILE:-10m}"
case "$UAT_PROFILE" in
  10m)
    default_duration_seconds=600
    default_min_users=1
    default_min_access_nodes=1
    default_min_replicas=1
    ;;
  6h)
    default_duration_seconds=21600
    default_min_users=1
    default_min_access_nodes=1
    default_min_replicas=1
    ;;
  24h)
    default_duration_seconds=86400
    default_min_users=2
    default_min_access_nodes=2
    default_min_replicas=2
    ;;
  *)
    echo "UAT_PROFILE must be 10m, 6h or 24h" >&2
    exit 2
    ;;
esac
UAT_DURATION_SECONDS="${UAT_DURATION_SECONDS:-$default_duration_seconds}"
UAT_POLL_INTERVAL_SECONDS="${UAT_POLL_INTERVAL_SECONDS:-60}"
UAT_RUNTIME_WARMUP_SECONDS="${UAT_RUNTIME_WARMUP_SECONDS:-300}"
UAT_RUNTIME_STALE_SECONDS="${UAT_RUNTIME_STALE_SECONDS:-600}"
UAT_MAX_QUEUED_PROBE_AGE_SECONDS="${UAT_MAX_QUEUED_PROBE_AGE_SECONDS:-600}"
UAT_MAX_PENDING_PROBES="${UAT_MAX_PENDING_PROBES:-50}"
UAT_REQUIRE_AGENT_API="${UAT_REQUIRE_AGENT_API:-0}"
UAT_AGENT_API_MODE="${UAT_AGENT_API_MODE:-observe}"
UAT_REQUIRE_CONFIG_APPLIED="${UAT_REQUIRE_CONFIG_APPLIED:-1}"
UAT_STAGE_RETRIES="${UAT_STAGE_RETRIES:-3}"
UAT_STAGE_RETRY_DELAY_SECONDS="${UAT_STAGE_RETRY_DELAY_SECONDS:-5}"
UAT_MIN_USERS="${UAT_MIN_USERS:-$default_min_users}"
UAT_MIN_ACCESS_NODES="${UAT_MIN_ACCESS_NODES:-$default_min_access_nodes}"
UAT_MIN_REPLICAS="${UAT_MIN_REPLICAS:-$default_min_replicas}"
UAT_ACCESS_NODE_IDS="${UAT_ACCESS_NODE_IDS:-$ACCESS_NODE_ID}"
UAT_ACCESS_LINE_IDS="${UAT_ACCESS_LINE_IDS:-$ACCESS_LINE_ID}"
UAT_EXIT_ENDPOINT_IDS="${UAT_EXIT_ENDPOINT_IDS:-$EXIT_ENDPOINT_ID}"
UAT_CLIENT_PROXY_URLS="${UAT_CLIENT_PROXY_URLS:-$CLIENT_PROXY_URL}"
UAT_EXPECTED_EXIT_IPS="${UAT_EXPECTED_EXIT_IPS:-$EXPECTED_EXIT_IP}"
UAT_USER_ACCESS_TOKENS="${UAT_USER_ACCESS_TOKENS:-$USER_ACCESS_TOKEN}"
UAT_AGENT_TOKENS="${UAT_AGENT_TOKENS:-$AGENT_TOKEN}"

xrayc_real_e2e_require_url_scheme BASE_URL "real runtime stability UAT"
xrayc_real_e2e_require_url_scheme DATABASE_URL "real runtime stability UAT"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "real runtime stability UAT" SUB_TOKEN SUBSCRIPTION_URL
xrayc_real_e2e_require_env CLIENT_PROXY_URL "real runtime stability UAT"
xrayc_real_e2e_require_env EXPECTED_EXIT_IP "real runtime stability UAT"
xrayc_real_e2e_require_env ACCESS_NODE_ID "real runtime stability UAT"
xrayc_real_e2e_require_env ACCESS_LINE_ID "real runtime stability UAT"
xrayc_real_e2e_require_env EXIT_ENDPOINT_ID "real runtime stability UAT"
xrayc_real_e2e_require_env USER_ACCESS_TOKEN "real runtime stability UAT"
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real runtime stability UAT"
xrayc_real_e2e_require_url_scheme UAT_CLIENT_TRAFFIC_URL "real runtime stability UAT"
for name in \
  UAT_DURATION_SECONDS \
  UAT_POLL_INTERVAL_SECONDS \
  UAT_RUNTIME_WARMUP_SECONDS \
  UAT_RUNTIME_STALE_SECONDS \
  UAT_MAX_QUEUED_PROBE_AGE_SECONDS \
  UAT_MAX_PENDING_PROBES \
  UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS \
  UAT_POST_TRAFFIC_SETTLE_SECONDS \
  UAT_STAGE_RETRIES \
  UAT_STAGE_RETRY_DELAY_SECONDS \
  UAT_MIN_USERS \
  UAT_MIN_ACCESS_NODES \
  UAT_MIN_REPLICAS; do
  if [[ ! "${!name}" =~ ^[0-9]+$ || "${!name}" -lt 1 ]]; then
    echo "${name} must be a positive integer" >&2
    exit 2
  fi
done

require_count_at_least UAT_USER_ACCESS_TOKENS "$UAT_USER_ACCESS_TOKENS" "$UAT_MIN_USERS"
require_count_at_least UAT_ACCESS_NODE_IDS "$UAT_ACCESS_NODE_IDS" "$UAT_MIN_ACCESS_NODES"
require_count_at_least UAT_ACCESS_LINE_IDS "$UAT_ACCESS_LINE_IDS" "$UAT_MIN_REPLICAS"
require_count_at_least UAT_EXIT_ENDPOINT_IDS "$UAT_EXIT_ENDPOINT_IDS" "$UAT_MIN_REPLICAS"
require_count_at_least UAT_CLIENT_PROXY_URLS "$UAT_CLIENT_PROXY_URLS" "$UAT_MIN_REPLICAS"
require_count_at_least UAT_EXPECTED_EXIT_IPS "$UAT_EXPECTED_EXIT_IPS" "$UAT_MIN_REPLICAS"
case "$UAT_AGENT_API_MODE" in
  observe|post) ;;
  *)
    echo "UAT_AGENT_API_MODE must be observe or post" >&2
    exit 2
    ;;
esac
if xrayc_real_e2e_bool_is_true "$UAT_REQUIRE_AGENT_API" && [[ "$UAT_AGENT_API_MODE" == "post" ]]; then
  require_count_at_least UAT_AGENT_TOKENS "$UAT_AGENT_TOKENS" "$UAT_MIN_ACCESS_NODES"
fi

case "$UAT_PROFILE" in
  10m)
    if [[ "$UAT_DURATION_SECONDS" -lt 600 ]]; then
      echo "10m stability UAT requires UAT_DURATION_SECONDS >= 600" >&2
      exit 2
    fi
    ;;
  6h)
    if [[ "$UAT_DURATION_SECONDS" -lt 21600 ]]; then
      echo "6h stability UAT requires UAT_DURATION_SECONDS >= 21600" >&2
      exit 2
    fi
    ;;
  24h)
    if [[ "$UAT_DURATION_SECONDS" -lt 86400 ]]; then
      echo "24h stability UAT requires UAT_DURATION_SECONDS >= 86400" >&2
      exit 2
    fi
    ;;
esac

if xrayc_real_e2e_bool_is_true "${UAT_VALIDATE_ONLY:-0}"; then
  echo "stability UAT validation passed"
  exit 0
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for real runtime stability UAT" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
deadline=$((SECONDS + UAT_DURATION_SECONDS))
iteration=0
failure_count=0
failure_log="$tmp_dir/failures.log"
subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
uat_proxy_urls=()
uat_expected_exit_ips=()
uat_user_tokens=()
uat_access_node_ids=()
uat_agent_tokens=()
split_csv "$UAT_CLIENT_PROXY_URLS" uat_proxy_urls
split_csv "$UAT_EXPECTED_EXIT_IPS" uat_expected_exit_ips
split_csv "$UAT_USER_ACCESS_TOKENS" uat_user_tokens
split_csv "$UAT_ACCESS_NODE_IDS" uat_access_node_ids
split_csv "$UAT_AGENT_TOKENS" uat_agent_tokens
usage_baselines=()

for token_index in "${!uat_user_tokens[@]}"; do
  [[ -n "${uat_user_tokens[$token_index]}" ]] || continue
  if ! usage_baselines[$token_index]="$(xrayc_real_e2e_capture_billing_baseline "$BASE_URL" "${uat_user_tokens[$token_index]}" "$tmp_dir")"; then
    usage_baselines[$token_index]=""
    record_failure "billing-baseline-${token_index}"
  fi
done

echo "stability UAT started"
while [[ "$SECONDS" -lt "$deadline" ]]; do
  iteration=$((iteration + 1))
  run_stage "health" check_health || true
  run_stage "subscription" check_subscription || true
  run_stage "agent-${UAT_AGENT_API_MODE}" check_agent_api || true
  for proxy_index in "${!uat_proxy_urls[@]}"; do
    [[ -n "${uat_proxy_urls[$proxy_index]}" ]] || continue
    expected_ip="${uat_expected_exit_ips[$proxy_index]:-${uat_expected_exit_ips[0]}}"
    run_stage "egress-${proxy_index}" \
      xrayc_real_e2e_assert_expected_egress_ip \
      "${uat_proxy_urls[$proxy_index]}" \
      "$PUBLIC_IP_URL" \
      "$expected_ip" \
      "$tmp_dir" || true
  done
  run_stage "client-traffic" generate_client_traffic || true
  run_stage "runtime-rows" check_runtime_rows || true
  if [[ "$failure_count" -eq 0 ]]; then
    echo "stability UAT iteration ${iteration}: passed"
  else
    echo "stability UAT iteration ${iteration}: diagnostics collected; failures_so_far=${failure_count}"
  fi
  sleep "$UAT_POLL_INTERVAL_SECONDS"
done

for token_index in "${!uat_user_tokens[@]}"; do
  [[ -n "${uat_user_tokens[$token_index]}" ]] || continue
  run_stage "billing-${token_index}" \
    xrayc_real_e2e_wait_billing_increase \
    "$BASE_URL" \
    "${uat_user_tokens[$token_index]}" \
    "${usage_baselines[$token_index]}" \
    "$tmp_dir" || true
done
if [[ "$failure_count" -gt 0 ]]; then
  echo "stability UAT failed after collecting diagnostics; failure_count=${failure_count}" >&2
  if [[ -s "$failure_log" ]]; then
    sed 's/[[:cntrl:]]//g' "$failure_log" >&2
  fi
  exit 1
fi
echo "stability UAT completed"
