#!/usr/bin/env bash
# 用途：执行真实第三方 Trojan 出口接入 E2E 验证。
# 范围：验证 Trojan 上游、订阅链路、客户端代理出口和可选计费。
# 输入：需要 BASE_URL、订阅信息、Trojan 上游、客户端代理和期望出口 IP。
# 输出：只输出脱敏后的校验进度，不打印密码、raw URL 或订阅令牌。
# 依赖：source real-e2e-lib.sh，并复用 real-smoke.sh 的真实基础检查。
# 安全：所有凭据字段仅参与本地临时配置和命令输入，退出后清理。
# 约束：上游可由 raw URL 或 host、port、password 组合提供。
# 行为：生成真实客户端请求后核对出口 IP 和可选 billed_bytes 增长。
# 失败：必需环境缺失、协议参数非法或出口与计费断言失败会退出。
# 维护：Trojan 参数或订阅格式变化时同步 usage 和注入逻辑。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  THIRD_PARTY_TROJAN_RAW_URL="<provider-raw-url>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="203.0.113.10" \
  bash scripts/real-access-third-party-trojan-e2e.sh

Required:
  BASE_URL, SUB_TOKEN or SUBSCRIPTION_URL,
  THIRD_PARTY_TROJAN_RAW_URL or THIRD_PARTY_TROJAN_HOST + THIRD_PARTY_TROJAN_PORT + password/key,
  CLIENT_PROXY_URL, EXPECTED_EXIT_IP.

Optional:
  THIRD_PARTY_TROJAN_HOST, THIRD_PARTY_TROJAN_PORT,
  THIRD_PARTY_TROJAN_PASSWORD, THIRD_PARTY_TROJAN_SNI,
  THIRD_PARTY_TROJAN_SERVER_NAME, THIRD_PARTY_TROJAN_SECURITY=tls,
  THIRD_PARTY_TROJAN_KEY, THIRD_PARTY_TROJAN_RAW_URL, PUBLIC_IP_URL.
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
THIRD_PARTY_TROJAN_HOST="${THIRD_PARTY_TROJAN_HOST:-}"
THIRD_PARTY_TROJAN_PORT="${THIRD_PARTY_TROJAN_PORT:-}"
THIRD_PARTY_TROJAN_PASSWORD="${THIRD_PARTY_TROJAN_PASSWORD:-}"
THIRD_PARTY_TROJAN_SNI="${THIRD_PARTY_TROJAN_SNI:-}"
THIRD_PARTY_TROJAN_KEY="${THIRD_PARTY_TROJAN_KEY:-}"
THIRD_PARTY_TROJAN_RAW_URL="${THIRD_PARTY_TROJAN_RAW_URL:-}"
THIRD_PARTY_TROJAN_SECURITY="${THIRD_PARTY_TROJAN_SECURITY:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

xrayc_real_e2e_require_url_scheme BASE_URL "real third-party Trojan e2e"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "real third-party Trojan e2e" SUB_TOKEN SUBSCRIPTION_URL
if [[ -z "$THIRD_PARTY_TROJAN_RAW_URL" ]]; then
  xrayc_real_e2e_require_env THIRD_PARTY_TROJAN_HOST "real third-party Trojan e2e"
  xrayc_real_e2e_require_env THIRD_PARTY_TROJAN_PORT "real third-party Trojan e2e"
  xrayc_real_e2e_require_any_env "THIRD_PARTY_TROJAN_PASSWORD, THIRD_PARTY_TROJAN_KEY, or raw URL credentials" "real third-party Trojan e2e" THIRD_PARTY_TROJAN_PASSWORD THIRD_PARTY_TROJAN_KEY
fi
if [[ -n "$THIRD_PARTY_TROJAN_SECURITY" && "${THIRD_PARTY_TROJAN_SECURITY,,}" != "tls" ]]; then
  echo "real third-party Trojan e2e: Trojan endpoints must use TLS" >&2
  exit 2
fi
if [[ -n "$THIRD_PARTY_TROJAN_RAW_URL" ]]; then
  trojan_flags="$(
    python3 - "$THIRD_PARTY_TROJAN_RAW_URL" <<'PY' 2>/dev/null || true
from urllib.parse import parse_qs, urlparse
import ipaddress
import sys

parsed = urlparse(sys.argv[1])
query = parse_qs(parsed.query)
security = (query.get("security", [""])[0] or "").lower()
has_sni = bool(
    (query.get("sni", [""])[0] or "").strip()
    or (query.get("servername", [""])[0] or "").strip()
    or (query.get("serverName", [""])[0] or "").strip()
    or (query.get("peer", [""])[0] or "").strip()
)
try:
    ipaddress.ip_address(parsed.hostname or "")
    host_is_ip = True
except ValueError:
    host_is_ip = False
print(f"security={security}")
print(f"has_sni={int(has_sni)}")
print(f"host_is_ip={int(host_is_ip)}")
PY
  )"
  if grep -q '^security=none$' <<<"$trojan_flags"; then
    echo "real third-party Trojan e2e: Trojan raw URL must not use security=none" >&2
    exit 2
  fi
  if grep -q '^host_is_ip=1$' <<<"$trojan_flags" \
    && grep -q '^has_sni=0$' <<<"$trojan_flags" \
    && [[ -z "$THIRD_PARTY_TROJAN_SNI" ]]; then
    echo "real third-party Trojan e2e: Trojan TLS with an IP host requires SNI" >&2
    exit 2
  fi
elif [[ -z "$THIRD_PARTY_TROJAN_SNI" && -z "${THIRD_PARTY_TROJAN_SERVER_NAME:-}" ]]; then
  echo "real third-party Trojan e2e: Trojan TLS requires SNI/server_name" >&2
  exit 2
fi
xrayc_real_e2e_require_env CLIENT_PROXY_URL "real third-party Trojan e2e"
xrayc_real_e2e_require_env EXPECTED_EXIT_IP "real third-party Trojan e2e"
xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_TROJAN_RAW_URL
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real third-party Trojan e2e"

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
ledger_before="$(xrayc_real_e2e_capture_ledger_baseline "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "trojan")"

subscription_url="${SUBSCRIPTION_URL:-${BASE_URL%/}/sub/${SUB_TOKEN}}"
subscription_file="$tmp_dir/subscription.yaml"
xrayc_real_e2e_fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"
xrayc_real_e2e_assert_client_proxy_endpoint "$CLIENT_PROXY_URL"

xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_HOST "subscription leaked third-party Trojan host"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_PORT "subscription leaked third-party Trojan port"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_PASSWORD "subscription leaked third-party Trojan password"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_SNI "subscription leaked third-party Trojan SNI"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_KEY "subscription leaked third-party Trojan key"
xrayc_real_e2e_assert_subscription_missing_env "$subscription_file" THIRD_PARTY_TROJAN_RAW_URL "subscription leaked raw third-party Trojan URL"
xrayc_real_e2e_assert_subscription_missing_pattern "$subscription_file" 'trojan://[^[:space:]]+' "subscription contains raw Trojan URL"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"

xrayc_real_e2e_assert_expected_egress_ip "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" "$EXPECTED_EXIT_IP" "$tmp_dir"

xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"
xrayc_real_e2e_wait_ledger_source_increase "$ledger_before" "${ACCESS_LINE_ID:-}" "${EXIT_ENDPOINT_ID:-}" "trojan"

echo "real third-party Trojan e2e completed"
