#!/usr/bin/env bash
# 用途：执行真实第三方 HY2 出口接入 E2E 验证。
# 范围：覆盖 HY2 上游配置、订阅链路、客户端代理出口和可选计费。
# 输入：需要 BASE_URL、订阅信息、HY2 上游、客户端代理和期望出口 IP。
# 输出：只报告校验阶段与结果，禁止打印上游凭据和订阅明文。
# 依赖：source real-e2e-lib.sh，并调用 real-smoke.sh 进行基础探测。
# 安全：支持密码、auth、obfs 和 key 字段但不得出现在日志中。
# 约束：上游可由 raw URL 或 host、port、认证字段组合描述。
# 行为：生成真实客户端流量后验证公网出口和可选 billed_bytes 增量。
# 失败：必需环境缺失、URL 非法、出口不符或计费断言失败会退出。
# 维护：HY2 参数模型变化时需同时更新 usage、校验和节点构造逻辑。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  THIRD_PARTY_HY2_RAW_URL="<provider-raw-url>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="203.0.113.10" \
  bash scripts/real-access-third-party-hy2-e2e.sh

Required:
  BASE_URL, SUB_TOKEN or SUBSCRIPTION_URL,
  THIRD_PARTY_HY2_RAW_URL or THIRD_PARTY_HY2_HOST + THIRD_PARTY_HY2_PORT + password/auth/key,
  CLIENT_PROXY_URL, EXPECTED_EXIT_IP.

Optional:
  THIRD_PARTY_HY2_HOST, THIRD_PARTY_HY2_PORT,
  THIRD_PARTY_HY2_PASSWORD, THIRD_PARTY_HY2_AUTH, THIRD_PARTY_HY2_OBFS_PASSWORD,
  THIRD_PARTY_HY2_KEY, THIRD_PARTY_HY2_RAW_URL, PUBLIC_IP_URL.
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
THIRD_PARTY_HY2_HOST="${THIRD_PARTY_HY2_HOST:-}"
THIRD_PARTY_HY2_PORT="${THIRD_PARTY_HY2_PORT:-}"
THIRD_PARTY_HY2_PASSWORD="${THIRD_PARTY_HY2_PASSWORD:-}"
THIRD_PARTY_HY2_AUTH="${THIRD_PARTY_HY2_AUTH:-}"
THIRD_PARTY_HY2_OBFS_PASSWORD="${THIRD_PARTY_HY2_OBFS_PASSWORD:-}"
THIRD_PARTY_HY2_KEY="${THIRD_PARTY_HY2_KEY:-}"
THIRD_PARTY_HY2_RAW_URL="${THIRD_PARTY_HY2_RAW_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

xrayc_real_e2e_require_url_scheme BASE_URL "real third-party HY2 e2e"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "real third-party HY2 e2e" SUB_TOKEN SUBSCRIPTION_URL
if [[ -z "$THIRD_PARTY_HY2_RAW_URL" ]]; then
  xrayc_real_e2e_require_env THIRD_PARTY_HY2_HOST "real third-party HY2 e2e"
  xrayc_real_e2e_require_env THIRD_PARTY_HY2_PORT "real third-party HY2 e2e"
  xrayc_real_e2e_require_any_env "THIRD_PARTY_HY2_PASSWORD, THIRD_PARTY_HY2_AUTH, THIRD_PARTY_HY2_KEY, or raw URL credentials" "real third-party HY2 e2e" THIRD_PARTY_HY2_PASSWORD THIRD_PARTY_HY2_AUTH THIRD_PARTY_HY2_KEY
fi
xrayc_real_e2e_require_env CLIENT_PROXY_URL "real third-party HY2 e2e"
xrayc_real_e2e_require_env EXPECTED_EXIT_IP "real third-party HY2 e2e"
xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_HY2_RAW_URL
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real third-party HY2 e2e"

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
ledger_before="$(xrayc_real_e2e_capture_ledger_baseline "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "hysteria")"

subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
subscription_file="$tmp_dir/subscription.yaml"
xrayc_real_e2e_fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"
xrayc_real_e2e_assert_client_proxy_endpoint "$CLIENT_PROXY_URL"

xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_HOST "subscription leaked third-party HY2 host"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_PORT "subscription leaked third-party HY2 port"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_PASSWORD "subscription leaked third-party HY2 password"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_AUTH "subscription leaked third-party HY2 auth"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_OBFS_PASSWORD "subscription leaked third-party HY2 obfs password"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_KEY "subscription leaked third-party HY2 key"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_HY2_RAW_URL "subscription leaked raw third-party HY2 URL"
xrayc_real_e2e_assert_subscription_missing_pattern "$subscription_file" '(hysteria2|hysteria|hy2)://[^[:space:]]+' "subscription contains raw HY2 URL"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"

xrayc_real_e2e_assert_expected_egress_ip "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" "$EXPECTED_EXIT_IP" "$tmp_dir"

xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"
xrayc_real_e2e_wait_ledger_source_increase "$ledger_before" "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "hysteria"

echo "real third-party HY2 e2e completed"
