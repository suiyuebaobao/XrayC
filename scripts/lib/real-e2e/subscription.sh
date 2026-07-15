#!/usr/bin/env bash
# 用途：提供 real e2e 订阅内容、访问节点和客户端代理端点校验函数。
# 该文件只定义断言 helper，失败时保持原有退出码和错误信息。
set -euo pipefail

xrayc_real_e2e_assert_subscription_missing_literal() {
  local subscription_file="$1"
  local literal="${2:-}"
  local failure_message="$3"

  [[ -n "$literal" ]] || return 0
  if grep -Fq -- "$literal" "$subscription_file"; then
    echo "$failure_message" >&2
    exit 1
  fi
}

xrayc_real_e2e_assert_subscription_missing_env() {
  local subscription_file="$1"
  local name="$2"
  local failure_message="$3"

  xrayc_real_e2e_assert_subscription_missing_literal "$subscription_file" "${!name:-}" "$failure_message"
}

xrayc_real_e2e_assert_subscription_missing_pattern() {
  local subscription_file="$1"
  local pattern="$2"
  local failure_message="$3"

  if grep -Eiq -- "$pattern" "$subscription_file"; then
    echo "$failure_message" >&2
    exit 1
  fi
}

xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls() {
  local subscription_file="$1"

  if grep -Eiq -- "(vless|trojan|ss|ssr|hysteria2|hysteria|hy2|socks|socks4|socks4a|socks5|socks5h)://[^[:space:]'\"]+" "$subscription_file"; then
    echo "subscription contains a raw proxy URL" >&2
    exit 1
  fi

  if grep -Eiq -- "https?://[^[:space:]'\"]+@[^[:space:]'\"]+" "$subscription_file"; then
    echo "subscription contains a credentialed HTTP proxy URL" >&2
    exit 1
  fi
}

xrayc_real_e2e_assert_subscription_access_servers() {
  local subscription_file="$1"
  local expected_servers="${EXPECTED_ACCESS_SERVERS:-}"

  if [[ -z "$expected_servers" ]]; then
    if xrayc_real_e2e_bool_is_true "${REQUIRE_ACCESS_SERVER_CHECK:-false}"; then
      echo "EXPECTED_ACCESS_SERVERS is required when REQUIRE_ACCESS_SERVER_CHECK=true" >&2
      exit 2
    fi
    echo "Skipping structured access server check: EXPECTED_ACCESS_SERVERS not provided."
    return
  fi

  EXPECTED_ACCESS_SERVERS="$expected_servers" python3 - "$subscription_file" <<'PY'
import os
import sys

try:
    import yaml
except Exception as exc:  # pragma: no cover - depends on host tooling
    raise SystemExit(f"PyYAML is required for structured subscription checks: {exc}")

expected = {
    item.strip()
    for item in os.environ["EXPECTED_ACCESS_SERVERS"].split(",")
    if item.strip()
}

if not expected:
    raise SystemExit("EXPECTED_ACCESS_SERVERS is empty")

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)

if not isinstance(payload, dict):
    raise SystemExit("subscription YAML root is not a mapping")

proxies = payload.get("proxies")
if not isinstance(proxies, list) or not proxies:
    raise SystemExit("subscription YAML has no proxies")

for proxy in proxies:
    if not isinstance(proxy, dict):
        raise SystemExit("subscription proxy entry is not a mapping")
    server = str(proxy.get("server", "")).strip()
    port = proxy.get("port")
    if not server or port is None:
        raise SystemExit("subscription proxy entry is missing server or port")
    actual = f"{server}:{int(port)}"
    if actual not in expected:
        raise SystemExit("subscription contains proxy server outside expected access lines")

print("structured access server check passed")
PY
}

xrayc_real_e2e_assert_client_proxy_endpoint() {
  local client_proxy_url="${1:-}"
  local expected_servers="${EXPECTED_CLIENT_PROXY_ENDPOINTS:-}"

  [[ -n "$client_proxy_url" ]] || { echo "CLIENT_PROXY_URL is required for client proxy check" >&2; exit 2; }

  CLIENT_PROXY_URL="$client_proxy_url" EXPECTED_CLIENT_PROXY_ENDPOINTS="$expected_servers" python3 - <<'PY'
import os
from urllib.parse import urlsplit

parsed = urlsplit(os.environ["CLIENT_PROXY_URL"])
try:
    port = parsed.port
except ValueError:
    raise SystemExit("CLIENT_PROXY_URL must include a valid numeric port")

if not parsed.scheme or not parsed.hostname or port is None:
    raise SystemExit("CLIENT_PROXY_URL must include scheme, host, and port")

raw_expected = os.environ.get("EXPECTED_CLIENT_PROXY_ENDPOINTS", "")
if not raw_expected:
    print("Skipping client proxy endpoint check: EXPECTED_CLIENT_PROXY_ENDPOINTS not provided.")
    raise SystemExit(0)

expected = {
    item.strip()
    for item in raw_expected.split(",")
    if item.strip()
}
if not expected:
    raise SystemExit("EXPECTED_CLIENT_PROXY_ENDPOINTS is empty")

actual = f"{parsed.hostname}:{port}"
if actual not in expected:
    raise SystemExit("CLIENT_PROXY_URL host is outside expected client proxy endpoints")

print("client proxy endpoint check passed")
PY
}
