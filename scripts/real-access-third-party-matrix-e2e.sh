#!/usr/bin/env bash
# 用途：串行执行真实第三方协议矩阵 E2E，覆盖多个出口协议。
# 范围：调度 SOCKS、HTTP、VLESS、Trojan、Shadowsocks 和 HY2 等脚本。
# 输入：读取 BASE_URL、订阅、客户端代理、期望出口和协议专属变量。
# 输出：输出每个协议的粗粒度执行结果，不打印任何私有凭据。
# 依赖：依赖各 real-access-third-party-*-e2e.sh 子脚本和真实 E2E 库。
# 安全：协议 raw URL、密码、token、代理和真实主机不得进入日志。
# 约束：可通过协议列表选择子集，但不在此处实现协议细节。
# 行为：逐个协议运行并保留失败状态，便于定位矩阵覆盖缺口。
# 失败：任一被选择协议失败时整体返回非零用于发布门禁。
# 维护：新增协议时需同步 usage、协议列表和子脚本调用分支。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" \
  SUB_TOKEN="<subscription-token>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IP="<expected-egress-ip>" \
  USER_ACCESS_TOKEN="<user-access-token>" \
  EXPECTED_ACCESS_SERVERS="access.example.com:443" \
  THIRD_PARTY_SOCKS_HOST="<host>" \
  THIRD_PARTY_HTTP_HOST="<host>" \
  THIRD_PARTY_VLESS_HOST="<host>" \
  THIRD_PARTY_TROJAN_HOST="<host>" \
  THIRD_PARTY_SHADOWSOCKS_HOST="<host>" \
  THIRD_PARTY_HY2_HOST="<host>" \
  bash scripts/real-access-third-party-matrix-e2e.sh

Optional:
  REAL_ACCESS_PROTOCOLS  Comma list; default socks,http,vless,trojan,shadowsocks,hy2.
  CLIENT_PROXY_URL_SOCKS, CLIENT_PROXY_URL_HTTP, CLIENT_PROXY_URL_VLESS,
  CLIENT_PROXY_URL_TROJAN, CLIENT_PROXY_URL_SHADOWSOCKS, CLIENT_PROXY_URL_HY2.
  Per-protocol client proxy overrides fall back to CLIENT_PROXY_URL outside
  release mode.
  EXPECTED_CLIENT_PROXY_ENDPOINTS optionally restricts local client proxy
  endpoints, for example 127.0.0.1:7890. It is separate from
  EXPECTED_ACCESS_SERVERS, which only validates subscription access lines.
  EXPECTED_EXIT_IP_SOCKS, EXPECTED_EXIT_IP_HTTP, EXPECTED_EXIT_IP_VLESS,
  EXPECTED_EXIT_IP_TROJAN, EXPECTED_EXIT_IP_SHADOWSOCKS, EXPECTED_EXIT_IP_HY2.
  Per-protocol expected egress IP overrides fall back to EXPECTED_EXIT_IP
  outside release mode.
  EXIT_ENDPOINT_ID_SOCKS, EXIT_ENDPOINT_ID_HTTP, EXIT_ENDPOINT_ID_VLESS,
  EXIT_ENDPOINT_ID_TROJAN, EXIT_ENDPOINT_ID_SHADOWSOCKS, EXIT_ENDPOINT_ID_HY2.
  Per-protocol endpoint IDs are used for ledger source and billing
  attribution checks.

This script does not print hosts, credentials, subscription URLs, tokens, or
raw proxy URLs. It only prints protocol names and coarse pass/fail status.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
RELEASE_PROTOCOLS="socks,http,vless,trojan,shadowsocks,hy2"
PROTOCOLS="${REAL_ACCESS_PROTOCOLS:-${REAL_PROTOCOL_MATRIX_PROTOCOLS:-$RELEASE_PROTOCOLS}}"
if [[ "${XRAYC_REAL_RELEASE:-0}" == "1" ]]; then
  PROTOCOLS="$RELEASE_PROTOCOLS"
fi

require_env() {
  local name="$1"
  xrayc_real_e2e_require_env "$name" "real third-party matrix e2e"
}

require_common_env() {
  xrayc_real_e2e_require_url_scheme BASE_URL "real third-party matrix e2e"
  if ! release_mode; then
    require_env CLIENT_PROXY_URL
    require_env EXPECTED_EXIT_IP
  fi
  require_env USER_ACCESS_TOKEN
  require_env EXPECTED_ACCESS_SERVERS
  require_env AGENT_TOKEN
  require_env ACCESS_NODE_ID
  require_env ACCESS_LINE_ID
  xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
  if xrayc_real_e2e_bool_is_true "${REQUIRE_LEDGER_SOURCE_CHECK:-false}"; then
    require_env DATABASE_URL
  fi
  if [[ -z "${SUB_TOKEN:-}" && -z "${SUBSCRIPTION_URL:-}" ]]; then
    echo "SUB_TOKEN or SUBSCRIPTION_URL is required for real third-party matrix e2e" >&2
    exit 2
  fi
}

release_mode() {
  [[ "${XRAYC_REAL_RELEASE:-0}" == "1" ]]
}

require_release_env() {
  local name="$1"

  if release_mode; then
    require_env "$name"
  fi
}

protocol_suffix() {
  local protocol="$1"

  case "$protocol" in
    socks)
      printf 'SOCKS'
      ;;
    http)
      printf 'HTTP'
      ;;
    vless)
      printf 'VLESS'
      ;;
    trojan)
      printf 'TROJAN'
      ;;
    shadowsocks|ss)
      printf 'SHADOWSOCKS'
      ;;
    hy2|hysteria|hysteria2)
      printf 'HY2'
      ;;
    *)
      printf ''
      ;;
  esac
}

protocol_env_value() {
  local protocol="$1"
  local prefix="$2"
  local fallback_name="${3:-}"
  local suffix=""
  local override_name=""

  suffix="$(protocol_suffix "$protocol")"
  if [[ -n "$suffix" ]]; then
    override_name="${prefix}_${suffix}"
    if [[ -n "${!override_name:-}" ]]; then
      printf '%s' "${!override_name}"
      return
    fi
    if release_mode; then
      echo "${override_name} is required for release third-party matrix e2e" >&2
      exit 2
    fi
  fi

  if [[ -n "$fallback_name" && -n "${!fallback_name:-}" ]]; then
    printf '%s' "${!fallback_name}"
    return
  fi

  echo "${prefix}_${suffix:-UNKNOWN} is required for real third-party matrix e2e" >&2
  exit 2
}

expected_ip_for_protocol() {
  local protocol="$1"
  protocol_env_value "$protocol" "EXPECTED_EXIT_IP" "EXPECTED_EXIT_IP"
}

client_proxy_for_protocol() {
  local protocol="$1"
  protocol_env_value "$protocol" "CLIENT_PROXY_URL" "CLIENT_PROXY_URL"
}

exit_endpoint_for_protocol() {
  local protocol="$1"
  protocol_env_value "$protocol" "EXIT_ENDPOINT_ID" "EXIT_ENDPOINT_ID"
}

run_protocol() {
  local protocol="$1"
  local script=""
  local expected_exit_ip=""
  local client_proxy_url=""
  local exit_endpoint_id=""

  case "$protocol" in
    socks)
      require_env THIRD_PARTY_SOCKS_HOST
      require_release_env THIRD_PARTY_SOCKS_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_SOCKS_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-socks-e2e.sh"
      ;;
    http)
      require_env THIRD_PARTY_HTTP_HOST
      require_release_env THIRD_PARTY_HTTP_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_HTTP_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-http-e2e.sh"
      ;;
    vless)
      if [[ -z "${THIRD_PARTY_HOST:-}" ]]; then
        require_env THIRD_PARTY_VLESS_HOST
        export THIRD_PARTY_HOST="${THIRD_PARTY_VLESS_HOST}"
      fi
      require_release_env THIRD_PARTY_VLESS_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_VLESS_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-vless-e2e.sh"
      ;;
    trojan)
      require_env THIRD_PARTY_TROJAN_HOST
      require_release_env THIRD_PARTY_TROJAN_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_TROJAN_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-trojan-e2e.sh"
      ;;
    shadowsocks|ss)
      require_env THIRD_PARTY_SHADOWSOCKS_HOST
      require_release_env THIRD_PARTY_SHADOWSOCKS_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_SHADOWSOCKS_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-shadowsocks-e2e.sh"
      ;;
    hy2|hysteria|hysteria2)
      require_env THIRD_PARTY_HY2_HOST
      require_release_env THIRD_PARTY_HY2_RAW_URL
      xrayc_real_e2e_require_url_scheme_if_set THIRD_PARTY_HY2_RAW_URL
      script="${SCRIPT_DIR}/real-access-third-party-hy2-e2e.sh"
      ;;
    *)
      echo "unsupported REAL_ACCESS_PROTOCOLS item" >&2
      exit 2
      ;;
  esac

  expected_exit_ip="$(expected_ip_for_protocol "$protocol")"
  [[ -n "$expected_exit_ip" ]] || { echo "expected egress ip is empty for ${protocol}" >&2; exit 2; }
  client_proxy_url="$(client_proxy_for_protocol "$protocol")"
  [[ -n "$client_proxy_url" ]] || { echo "client proxy url is empty for ${protocol}" >&2; exit 2; }
  xrayc_real_e2e_assert_client_proxy_endpoint "$client_proxy_url"
  exit_endpoint_id="$(exit_endpoint_for_protocol "$protocol")"
  [[ -n "$exit_endpoint_id" ]] || { echo "exit endpoint id is empty for ${protocol}" >&2; exit 2; }

  echo "Running third-party ${protocol} real e2e"
  (
    cd "$BASE_DIR"
    EXPECTED_EXIT_IP="$expected_exit_ip" \
    CLIENT_PROXY_URL="$client_proxy_url" \
    EXIT_ENDPOINT_ID="$exit_endpoint_id" \
    STRICT_AGENT_TRAFFIC=1 \
    REQUIRE_BILLING_CHECK=true \
    REQUIRE_ACCESS_SERVER_CHECK=true \
    bash "$script"
  )
}

require_common_env

IFS=',' read -r -a protocol_items <<< "$PROTOCOLS"
if [[ "${#protocol_items[@]}" -eq 0 ]]; then
  echo "REAL_ACCESS_PROTOCOLS is empty" >&2
  exit 2
fi

found_protocol=0
for item in "${protocol_items[@]}"; do
  protocol="$(printf '%s' "$item" | tr '[:upper:]' '[:lower:]' | xargs)"
  if [[ -z "$protocol" ]]; then
    echo "REAL_ACCESS_PROTOCOLS contains an empty item" >&2
    exit 2
  fi
  found_protocol=1
  run_protocol "$protocol"
done
if [[ "$found_protocol" -ne 1 ]]; then
  echo "REAL_ACCESS_PROTOCOLS has no runnable protocol" >&2
  exit 2
fi

echo "real third-party protocol matrix e2e completed"
