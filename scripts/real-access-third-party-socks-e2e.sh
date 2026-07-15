#!/usr/bin/env bash
# 用途：执行真实第三方 SOCKS 出口接入 E2E 验证。
# 范围：先跑真实 smoke，再通过客户端代理验证订阅出口与计费增长。
# 输入：需要 BASE_URL、订阅信息、SOCKS 上游、客户端代理和期望出口 IP。
# 输出：只输出阶段性校验结果，避免展示订阅、代理和凭据明文。
# 依赖：source real-e2e-lib.sh，并调用 real-smoke.sh 复用基础校验。
# 安全：临时目录自动清理，敏感 URL、token、密码不得写入日志。
# 约束：上游可由 raw URL 或 host、port、认证字段组合提供。
# 行为：根据可选 USER_ACCESS_TOKEN 采集并验证 billed_bytes 增长。
# 失败：环境缺失、URL scheme 不合法、出口 IP 不符或计费未增长会失败。
# 维护：SOCKS 专属字段变更时需同步 usage、校验和订阅注入逻辑。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  THIRD_PARTY_SOCKS_RAW_URL="<provider-raw-url>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="203.0.113.10" \
  bash scripts/real-access-third-party-socks-e2e.sh

Required:
  BASE_URL, SUB_TOKEN or SUBSCRIPTION_URL,
  THIRD_PARTY_SOCKS_RAW_URL or THIRD_PARTY_SOCKS_HOST + THIRD_PARTY_SOCKS_PORT,
  CLIENT_PROXY_URL, EXPECTED_EXIT_IP.

Optional:
  THIRD_PARTY_SOCKS_HOST, THIRD_PARTY_SOCKS_PORT,
  THIRD_PARTY_SOCKS_USERNAME, THIRD_PARTY_SOCKS_PASSWORD,
  THIRD_PARTY_SOCKS_KEY, THIRD_PARTY_SOCKS_RAW_URL, PUBLIC_IP_URL.
  AGENT_TOKEN, ACCESS_NODE_ID, ACCESS_LINE_ID, EXIT_ENDPOINT_ID, XRAY_USER_KEY.
  USER_ACCESS_TOKEN  When set, verify /api/user/usage billed_bytes increases.
  REQUIRE_BILLING_CHECK=true  Fail if USER_ACCESS_TOKEN is missing.
  EXPECTED_ACCESS_SERVERS      Comma-separated access host:port allowlist.
  REQUIRE_ACCESS_SERVER_CHECK=true  Fail if EXPECTED_ACCESS_SERVERS is missing.
  BILLING_POLL_SECONDS, BILLING_POLL_INTERVAL_SECONDS
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

BASE_URL="${BASE_URL:-}"
SUB_TOKEN="${SUB_TOKEN:-}"
SUBSCRIPTION_URL="${SUBSCRIPTION_URL:-}"
THIRD_PARTY_SOCKS_HOST="${THIRD_PARTY_SOCKS_HOST:-}"
THIRD_PARTY_SOCKS_PORT="${THIRD_PARTY_SOCKS_PORT:-}"
THIRD_PARTY_SOCKS_USERNAME="${THIRD_PARTY_SOCKS_USERNAME:-}"
THIRD_PARTY_SOCKS_PASSWORD="${THIRD_PARTY_SOCKS_PASSWORD:-}"
THIRD_PARTY_SOCKS_KEY="${THIRD_PARTY_SOCKS_KEY:-}"
THIRD_PARTY_SOCKS_RAW_URL="${THIRD_PARTY_SOCKS_RAW_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

xrayc_real_e2e_require_url_scheme BASE_URL "real third-party SOCKS e2e"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "real third-party SOCKS e2e" SUB_TOKEN SUBSCRIPTION_URL
if [[ -z "$THIRD_PARTY_SOCKS_RAW_URL" ]]; then
  xrayc_real_e2e_require_env THIRD_PARTY_SOCKS_HOST "real third-party SOCKS e2e"
  xrayc_real_e2e_require_env THIRD_PARTY_SOCKS_PORT "real third-party SOCKS e2e"
fi
xrayc_real_e2e_require_env CLIENT_PROXY_URL "real third-party SOCKS e2e"
xrayc_real_e2e_require_env EXPECTED_EXIT_IP "real third-party SOCKS e2e"
xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_SOCKS_RAW_URL
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real third-party SOCKS e2e"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

BASE_URL="$BASE_URL" \
SUB_TOKEN="$SUB_TOKEN" \
SUBSCRIPTION_URL="$SUBSCRIPTION_URL" \
AGENT_TOKEN="${AGENT_TOKEN:-}" \
ACCESS_NODE_ID="${ACCESS_NODE_ID:-}" \
ACCESS_LINE_ID="${ACCESS_LINE_ID:-}" \
EXIT_ENDPOINT_ID="${EXIT_ENDPOINT_ID:-}" \
XRAY_USER_KEY="${XRAY_USER_KEY:-}" \
bash scripts/real-smoke.sh

billing_before="$(xrayc_real_e2e_capture_billing_baseline "$BASE_URL" "$USER_ACCESS_TOKEN" "$tmp_dir")"
ledger_before="$(xrayc_real_e2e_capture_ledger_baseline "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "socks")"

subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
subscription_file="$tmp_dir/subscription.yaml"
xrayc_real_e2e_fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"
xrayc_real_e2e_assert_client_proxy_endpoint "$CLIENT_PROXY_URL"

xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_HOST "subscription leaked third-party SOCKS host"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_PORT "subscription leaked third-party SOCKS port"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_USERNAME "subscription leaked third-party SOCKS username"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_PASSWORD "subscription leaked third-party SOCKS password"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_KEY "subscription leaked third-party SOCKS key"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SOCKS_RAW_URL "subscription leaked raw third-party SOCKS URL"
xrayc_real_e2e_assert_subscription_missing_pattern "$subscription_file" 'socks(4|4a|5|5h)?://[^[:space:]]+' "subscription contains raw SOCKS URL"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"

xrayc_real_e2e_assert_expected_egress_ip "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" "$EXPECTED_EXIT_IP" "$tmp_dir"

xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"
xrayc_real_e2e_wait_ledger_source_increase "$ledger_before" "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "socks"

echo "real third-party SOCKS e2e completed"
