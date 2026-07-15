#!/usr/bin/env bash
# 用途：执行真实接入池 E2E，验证接入线路、出口池和运行时上报链路。
# 范围：覆盖真实 smoke、订阅下载、客户端流量和数据库状态核对。
# 输入：需要 BASE_URL、订阅或登录信息、数据库、客户端代理和期望出口。
# 输出：只输出阶段名和断言结果，避免泄露 URL、token、DB 或代理信息。
# 依赖：source real-e2e-lib.sh，并使用 curl、psql 等真实环境工具。
# 安全：所有私有值来自环境变量，日志必须保持脱敏。
# 约束：仅面向私有真实环境，不应对生产库或公共凭据直接运行。
# 行为：生成真实请求后核对 access line、exit pool 与账单/运行时数据。
# 失败：缺少环境、订阅不可用、出口不符或数据库断言失败即退出。
# 维护：接入池数据模型变化时需同步 SQL 断言和 usage 文档。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="203.0.113.10" \
  bash scripts/real-access-pool-e2e.sh

Required:
  BASE_URL           Control-plane origin.
  SUB_TOKEN or SUBSCRIPTION_URL
  CLIENT_PROXY_URL  Local client proxy after importing the subscription.
  EXPECTED_EXIT_IP  Public IP expected for the selected exit-pool endpoint.

Optional:
  PUBLIC_IP_URL      Default: https://api.ipify.org
  AGENT_TOKEN, ACCESS_NODE_ID, ACCESS_LINE_ID, EXIT_ENDPOINT_ID, XRAY_USER_KEY
  USER_ACCESS_TOKEN  When set, verify /api/user/usage billed_bytes increases.
  REQUIRE_BILLING_CHECK=true  Fail if USER_ACCESS_TOKEN is missing.
  EXPECTED_ACCESS_SERVERS      Comma-separated access host:port allowlist.
  REQUIRE_ACCESS_SERVER_CHECK=true  Fail if EXPECTED_ACCESS_SERVERS is missing.
  BILLING_POLL_SECONDS, BILLING_POLL_INTERVAL_SECONDS
  BILLING_TRAFFIC_PROBE_COUNT, BILLING_TRAFFIC_PROBE_BYTES
                     Extra real proxy downloads before billing polling, default 3 MiB total.

The script fails when real client traffic cannot be sent through CLIENT_PROXY_URL.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

BASE_URL="${BASE_URL:-}"
SUB_TOKEN="${SUB_TOKEN:-}"
SUBSCRIPTION_URL="${SUBSCRIPTION_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

[[ -n "$BASE_URL" ]] || { echo "BASE_URL is required" >&2; exit 2; }
[[ -n "$SUB_TOKEN" || -n "$SUBSCRIPTION_URL" ]] || { echo "SUB_TOKEN or SUBSCRIPTION_URL is required" >&2; exit 2; }
[[ -n "$CLIENT_PROXY_URL" ]] || { echo "CLIENT_PROXY_URL is required" >&2; exit 2; }
[[ -n "$EXPECTED_EXIT_IP" ]] || { echo "EXPECTED_EXIT_IP is required" >&2; exit 2; }

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
billing_before="$(xrayc_real_e2e_capture_billing_baseline "$BASE_URL" "$USER_ACCESS_TOKEN" "$tmp_dir")"

fetch_url_to_file() {
  local url="$1"
  local output_file="$2"
  local failure_message="$3"

  if ! curl --fail --silent --location --max-time 30 "$url" >"$output_file" 2>/dev/null; then
    echo "${failure_message}; raw curl output redacted" >&2
    exit 1
  fi
}

fetch_url_via_proxy_to_file() {
  local proxy_url="$1"
  local url="$2"
  local output_file="$3"
  local failure_message="$4"

  if ! curl --fail --silent --location --max-time 30 --proxy "$proxy_url" "$url" >"$output_file" 2>/dev/null; then
    echo "${failure_message}; raw curl output redacted" >&2
    exit 1
  fi
}

generate_billing_probe_traffic() {
  local proxy_url="$1"
  local count="${BILLING_TRAFFIC_PROBE_COUNT:-3}"
  local bytes="${BILLING_TRAFFIC_PROBE_BYTES:-1048576}"
  local url="${BILLING_TRAFFIC_PROBE_URL:-https://speed.cloudflare.com/__down?bytes=${bytes}}"
  local index

  [[ "$count" =~ ^[0-9]+$ && "$count" -gt 0 ]] || return 0
  echo "Generating billed traffic sample"
  for ((index = 1; index <= count; index += 1)); do
    if ! curl --fail --silent --location --max-time 60 --proxy "$proxy_url" "$url" >/dev/null 2>/dev/null; then
      echo "billed traffic sample failed; raw curl output redacted" >&2
      exit 1
    fi
  done
}

echo "Running control-plane smoke"
BASE_URL="$BASE_URL" \
SUB_TOKEN="$SUB_TOKEN" \
SUBSCRIPTION_URL="$SUBSCRIPTION_URL" \
AGENT_TOKEN="${AGENT_TOKEN:-}" \
ACCESS_NODE_ID="${ACCESS_NODE_ID:-}" \
ACCESS_LINE_ID="${ACCESS_LINE_ID:-}" \
EXIT_ENDPOINT_ID="${EXIT_ENDPOINT_ID:-}" \
XRAY_USER_KEY="${XRAY_USER_KEY:-}" \
REQUIRE_SUBSCRIPTION_DOWNLOAD="${REQUIRE_SUBSCRIPTION_DOWNLOAD:-0}" \
DISABLE_SYNTHETIC_AGENT_POSTS="${DISABLE_SYNTHETIC_AGENT_POSTS:-0}" \
bash scripts/real-smoke.sh

subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
subscription_file="$tmp_dir/subscription.yaml"
fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"

if grep -Eiq -- '(outbound_proxy_url|exit_endpoint|agent_token|private[ _-]?key)[[:space:]]*[:=]' "$subscription_file"; then
  echo "subscription leaked control-plane or upstream fields" >&2
  exit 1
fi

echo "Checking real client egress"
actual_ip_file="$tmp_dir/public-ip.txt"
fetch_url_via_proxy_to_file "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" "$actual_ip_file" "public IP probe failed"
actual_ip="$(tr -d '[:space:]' < "$actual_ip_file")"
if [[ "$actual_ip" != "$EXPECTED_EXIT_IP" ]]; then
  echo "unexpected egress ip: actual value differs from expected value" >&2
  exit 1
fi

if [[ -n "$billing_before" ]]; then
  generate_billing_probe_traffic "$CLIENT_PROXY_URL"
fi
xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"

echo "real exit pool e2e completed"
