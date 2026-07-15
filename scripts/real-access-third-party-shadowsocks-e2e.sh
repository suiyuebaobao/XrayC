#!/usr/bin/env bash
# 用途：执行真实第三方 Shadowsocks 出口接入 E2E 验证。
# 范围：验证 Shadowsocks 上游、订阅下发、客户端代理和出口 IP。
# 输入：需要 BASE_URL、订阅信息、Shadowsocks 上游、客户端代理和出口 IP。
# 输出：仅输出脱敏后的阶段结果，不打印密码、raw URL 或订阅 token。
# 依赖：source real-e2e-lib.sh，并复用 real-smoke.sh 的基础检查。
# 安全：临时目录退出即清理，任何敏感环境值只参与命令输入。
# 约束：上游可由 raw URL 或 host、port、method、password 组合提供。
# 行为：可按需检查用户账单流量在真实请求后增长。
# 失败：环境、协议参数、出口 IP 或计费断言不满足时返回非零。
# 维护：Shadowsocks 加密参数变化时同步 usage 和校验逻辑。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  THIRD_PARTY_SHADOWSOCKS_RAW_URL="<provider-raw-url>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="203.0.113.10" \
  bash scripts/real-access-third-party-shadowsocks-e2e.sh

Required:
  BASE_URL, SUB_TOKEN or SUBSCRIPTION_URL,
  THIRD_PARTY_SHADOWSOCKS_RAW_URL or THIRD_PARTY_SHADOWSOCKS_HOST + THIRD_PARTY_SHADOWSOCKS_PORT,
  CLIENT_PROXY_URL, EXPECTED_EXIT_IP.

Optional:
  THIRD_PARTY_SHADOWSOCKS_HOST, THIRD_PARTY_SHADOWSOCKS_PORT,
  THIRD_PARTY_SHADOWSOCKS_METHOD, THIRD_PARTY_SHADOWSOCKS_CIPHER,
  THIRD_PARTY_SHADOWSOCKS_PASSWORD, THIRD_PARTY_SHADOWSOCKS_KEY,
  THIRD_PARTY_SHADOWSOCKS_RAW_URL, PUBLIC_IP_URL.
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
THIRD_PARTY_SHADOWSOCKS_HOST="${THIRD_PARTY_SHADOWSOCKS_HOST:-}"
THIRD_PARTY_SHADOWSOCKS_PORT="${THIRD_PARTY_SHADOWSOCKS_PORT:-}"
THIRD_PARTY_SHADOWSOCKS_METHOD="${THIRD_PARTY_SHADOWSOCKS_METHOD:-}"
THIRD_PARTY_SHADOWSOCKS_CIPHER="${THIRD_PARTY_SHADOWSOCKS_CIPHER:-}"
THIRD_PARTY_SHADOWSOCKS_PASSWORD="${THIRD_PARTY_SHADOWSOCKS_PASSWORD:-}"
THIRD_PARTY_SHADOWSOCKS_KEY="${THIRD_PARTY_SHADOWSOCKS_KEY:-}"
THIRD_PARTY_SHADOWSOCKS_RAW_URL="${THIRD_PARTY_SHADOWSOCKS_RAW_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

xrayc_real_e2e_require_url_scheme BASE_URL "real third-party Shadowsocks e2e"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "real third-party Shadowsocks e2e" SUB_TOKEN SUBSCRIPTION_URL
if [[ -z "$THIRD_PARTY_SHADOWSOCKS_RAW_URL" ]]; then
  xrayc_real_e2e_require_env THIRD_PARTY_SHADOWSOCKS_HOST "real third-party Shadowsocks e2e"
  xrayc_real_e2e_require_env THIRD_PARTY_SHADOWSOCKS_PORT "real third-party Shadowsocks e2e"
  xrayc_real_e2e_require_any_env "THIRD_PARTY_SHADOWSOCKS_PASSWORD, THIRD_PARTY_SHADOWSOCKS_KEY, or raw URL credentials" "real third-party Shadowsocks e2e" THIRD_PARTY_SHADOWSOCKS_PASSWORD THIRD_PARTY_SHADOWSOCKS_KEY
  if [[ -z "$THIRD_PARTY_SHADOWSOCKS_KEY" ]]; then
    xrayc_real_e2e_require_any_env "THIRD_PARTY_SHADOWSOCKS_METHOD or THIRD_PARTY_SHADOWSOCKS_CIPHER" "real third-party Shadowsocks e2e" THIRD_PARTY_SHADOWSOCKS_METHOD THIRD_PARTY_SHADOWSOCKS_CIPHER
  fi
fi
xrayc_real_e2e_require_env CLIENT_PROXY_URL "real third-party Shadowsocks e2e"
xrayc_real_e2e_require_env EXPECTED_EXIT_IP "real third-party Shadowsocks e2e"
xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_SHADOWSOCKS_RAW_URL
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real third-party Shadowsocks e2e"

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
ledger_before="$(xrayc_real_e2e_capture_ledger_baseline "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "shadowsocks")"

subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
subscription_file="$tmp_dir/subscription.yaml"
xrayc_real_e2e_fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"
xrayc_real_e2e_assert_client_proxy_endpoint "$CLIENT_PROXY_URL"

xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_HOST "subscription leaked third-party Shadowsocks host"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_PORT "subscription leaked third-party Shadowsocks port"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_METHOD "subscription leaked third-party Shadowsocks method"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_CIPHER "subscription leaked third-party Shadowsocks cipher"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_PASSWORD "subscription leaked third-party Shadowsocks password"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_KEY "subscription leaked third-party Shadowsocks key"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_SHADOWSOCKS_RAW_URL "subscription leaked raw third-party Shadowsocks URL"
xrayc_real_e2e_assert_subscription_missing_pattern "$subscription_file" '(ss|ssr)://[^[:space:]]+' "subscription contains raw Shadowsocks URL"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"

xrayc_real_e2e_assert_expected_egress_ip "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" "$EXPECTED_EXIT_IP" "$tmp_dir"

xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"
xrayc_real_e2e_wait_ledger_source_increase "$ledger_before" "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "shadowsocks"

echo "real third-party Shadowsocks e2e completed"
