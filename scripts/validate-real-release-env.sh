#!/usr/bin/env bash
# 用途：校验真实发布环境变量是否满足完整发布门禁。
# 说明：主脚本负责加载私有环境文件并汇总校验结果，不打印密钥值。
# 拆分：具体门禁规则放在 scripts/lib/validate-real-release-env/ helper 中。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

usage() {
  cat <<'EOF'
usage: bash scripts/validate-real-release-env.sh

Validate the private real-release env without printing secret values. The script
loads .env.real-release when it exists, or XRAYC_REAL_RELEASE_ENV_FILE when set.
It reports missing variables, placeholder values, and coarse format issues only.
EOF
}

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

failures=0

report() {
  local message="$1"
  printf 'real-release-env: %s\n' "$message" >&2
  failures=$((failures + 1))
}

value_ready() {
  local value="${1:-}"
  [[ -n "$value" ]] || return 1
  value_is_placeholder "$value" && return 1
  return 0
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-target-name* ]] && return 0
  [[ "$value" == *private-inventory-path* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *upstream-host* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *your-secret* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *example* ]] && return 0
  return 1
}

require_value() {
  local name="$1"
  local value="${!name:-}"
  if [[ -z "$value" ]]; then
    report "${name} is missing"
  elif value_is_placeholder "$value"; then
    report "${name} still contains a placeholder"
  fi
}

require_exact_value() {
  local name="$1"
  local expected="$2"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ "$value" != "$expected" ]]; then
    report "${name} must be ${expected}"
  fi
}

require_any_value() {
  local label="$1"
  shift
  local name=""
  local found=0
  local placeholder=0
  for name in "$@"; do
    if [[ -n "${!name:-}" ]]; then
      found=1
      if value_is_placeholder "${!name:-}"; then
        placeholder=1
      fi
    fi
  done
  if [[ "$found" -ne 1 ]]; then
    report "${label} is missing"
  elif [[ "$placeholder" -eq 1 ]]; then
    report "${label} still contains a placeholder"
  fi
}

require_enabled_gate() {
  local name="$1"
  if [[ ! "${!name:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    report "${name}=1 is required for the full real release gate"
  fi
}

forbid_true_flag() {
  local name="$1"
  case "${!name:-0}" in
    1|true|TRUE|yes|YES|on|ON)
      report "${name} must not be enabled for the full real release gate"
      ;;
  esac
}

require_uuid() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ -n "$value" && ! "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
    report "${name} is not a UUID"
  fi
}

require_uuid_if_ready() {
  local name="$1"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ ! "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
    report "${name} is not a UUID"
  fi
}

require_matrix_endpoint_uuid() {
  local name="$1"
  if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "direct" ]]; then
    require_uuid "$name"
  else
    require_uuid_if_ready "$name"
  fi
}

require_https_or_private_http() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ -z "$value" || "$value" == https://* ]]; then
    return
  fi
  if [[ "$value" == http://* && "${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-0}" == "1" ]]; then
    return
  fi
  report "${name} must use HTTPS unless the private HTTP E2E override is enabled"
}

require_url_scheme() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if ! python3 - "$value" <<'PY' >/dev/null 2>&1
from urllib.parse import urlparse
import sys

parsed = urlparse(sys.argv[1])
if not parsed.scheme or not parsed.hostname:
    raise SystemExit(1)
if parsed.port is not None and (parsed.port < 1 or parsed.port > 65535):
    raise SystemExit(1)
raise SystemExit(0)
PY
  then
    report "${name} must be a valid URL with scheme and host"
  fi
}

require_trojan_tls_ready() {
  local raw_url="${THIRD_PARTY_TROJAN_RAW_URL:-}"
  local security="${THIRD_PARTY_TROJAN_SECURITY:-}"
  local sni="${THIRD_PARTY_TROJAN_SNI:-${THIRD_PARTY_TROJAN_SERVER_NAME:-}}"
  local parsed_flags=""

  if value_ready "$security" && [[ "${security,,}" != "tls" ]]; then
    report "THIRD_PARTY_TROJAN_SECURITY must be tls"
  fi

  if value_ready "$raw_url"; then
    parsed_flags="$(
      python3 - "$raw_url" <<'PY' 2>/dev/null || true
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
    if grep -q '^security=none$' <<<"$parsed_flags"; then
      report "THIRD_PARTY_TROJAN_RAW_URL must not use security=none"
    fi
    if grep -q '^host_is_ip=1$' <<<"$parsed_flags" \
      && grep -q '^has_sni=0$' <<<"$parsed_flags" \
      && ! value_ready "$sni"; then
      report "Trojan TLS with an IP host requires THIRD_PARTY_TROJAN_SNI/server_name"
    fi
  fi

  if ! value_ready "$sni" && ! value_ready "$raw_url"; then
    report "Trojan TLS requires THIRD_PARTY_TROJAN_SNI/server_name or a raw URL with SNI/domain"
  fi
}

require_host_if_ready() {
  local name="$1"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if ! python3 - "$value" <<'PY' >/dev/null 2>&1
import sys

value = sys.argv[1]
if any(ch.isspace() for ch in value) or any(ord(ch) < 32 for ch in value):
    raise SystemExit(1)
if "://" in value or "/" in value or "?" in value or "#" in value:
    raise SystemExit(1)
if not value.strip("[]"):
    raise SystemExit(1)
raise SystemExit(0)
PY
  then
    report "${name} must be a host without scheme, path, query, or whitespace"
  fi
}

require_postgres_url() {
  local name="$1"
  local value="${!name:-}"
  local allow_local="${XRAYC_REAL_RELEASE_ALLOW_LOCAL_DATABASE:-0}"
  if [[ "$name" == "RUNTIME_LOADTEST_DATABASE_URL" \
    || "$name" == "OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL" ]]; then
    allow_local=1
  fi
  require_value "$name"
  value_ready "$value" || return 0
  if [[ "$value" != postgres://* && "$value" != postgresql://* ]]; then
    report "${name} must be a PostgreSQL URL"
    return 0
  fi
  case "$value" in
    *example*|*test*|*loadtest*|*change-me*)
      if [[ "$name" != "RUNTIME_LOADTEST_DATABASE_URL" \
        && "$name" != "OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL" ]]; then
        report "${name} must point to the production control-plane PostgreSQL database"
      fi
      ;;
    *localhost*|*127.0.0.1*|*0.0.0.0*)
      if [[ "$allow_local" != "1" ]]; then
        report "${name} is local; set XRAYC_REAL_RELEASE_ALLOW_LOCAL_DATABASE=1 only for single-server Docker production release checks"
      fi
      ;;
  esac
}

require_public_ip() {
  local name="$1"
  local value="${!name:-}"
  require_value "$name"
  value_ready "$value" || return 0
  if ! python3 - "$value" <<'PY' >/dev/null 2>&1
import ipaddress
import sys

try:
    ip = ipaddress.ip_address(sys.argv[1])
except ValueError:
    raise SystemExit(1)
raise SystemExit(0 if ip.is_global else 1)
PY
  then
    report "${name} must be a public IPv4 or IPv6 address"
  fi
}

require_port_if_ready() {
  local name="$1"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    report "${name} must be a valid port"
  elif (( value <= 0 || value > 65535 )); then
    report "${name} must be a valid port"
  fi
}

require_file() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if [[ -n "$value" && ! -f "$value" ]]; then
    report "${name} file does not exist"
  fi
}

require_optional_file() {
  local name="$1"
  local value="${!name:-}"
  if [[ -z "$value" ]]; then
    return
  fi
  if value_is_placeholder "$value"; then
    report "${name} still contains a placeholder"
    return
  fi
  if [[ ! -f "$value" ]]; then
    report "${name} file does not exist"
  elif [[ ! -s "$value" ]]; then
    report "${name} file is empty"
  fi
}

require_json_file() {
  local name="$1"
  require_file "$name"
  local value="${!name:-}"
  if [[ -n "$value" && -f "$value" ]]; then
    if ! python3 - "$value" <<'PY' >/dev/null 2>&1
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    json.load(fh)
PY
    then
      report "${name} is not valid JSON"
    fi
  fi
}

require_protocol_group() {
  local suffix="$1"
  local raw_url_var="$2"
  local host_var="$3"
  local port_var="$4"
  local proxy_var="CLIENT_PROXY_URL_${suffix}"
  local exit_ip_var="EXPECTED_EXIT_IP_${suffix}"
  local endpoint_var="EXIT_ENDPOINT_ID_${suffix}"

  if value_ready "${!raw_url_var:-}"; then
    require_url_scheme "$raw_url_var"
  else
    require_value "$host_var"
    require_value "$port_var"
    require_host_if_ready "$host_var"
    require_port_if_ready "$port_var"
  fi
  if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "direct" ]]; then
    require_url_scheme "$proxy_var"
  fi
  require_public_ip "$exit_ip_var"
  require_matrix_endpoint_uuid "$endpoint_var"
}

# 协议矩阵变量显式清单，供同步检查脚本确认模板、文档和校验逻辑没有漂移：
# CLIENT_PROXY_URL_SOCKS EXPECTED_EXIT_IP_SOCKS EXIT_ENDPOINT_ID_SOCKS
# CLIENT_PROXY_URL_HTTP EXPECTED_EXIT_IP_HTTP EXIT_ENDPOINT_ID_HTTP
# CLIENT_PROXY_URL_VLESS EXPECTED_EXIT_IP_VLESS EXIT_ENDPOINT_ID_VLESS
# CLIENT_PROXY_URL_TROJAN EXPECTED_EXIT_IP_TROJAN EXIT_ENDPOINT_ID_TROJAN
# CLIENT_PROXY_URL_SHADOWSOCKS EXPECTED_EXIT_IP_SHADOWSOCKS EXIT_ENDPOINT_ID_SHADOWSOCKS
# CLIENT_PROXY_URL_HY2 EXPECTED_EXIT_IP_HY2 EXIT_ENDPOINT_ID_HY2
# 真实发布强门禁变量显式清单；只有 24h/6h 扩展长稳属于显式追加 UAT：
# ADMIN_ACCESS_TOKEN RUN_REAL_STABILITY_UAT RUN_REAL_STABILITY_UAT_24H UAT_AGENT_API_MODE
# UAT_PROFILE UAT_ACCESS_NODE_IDS UAT_ACCESS_LINE_IDS UAT_EXIT_ENDPOINT_IDS
# UAT_CLIENT_PROXY_URLS UAT_EXPECTED_EXIT_IPS UAT_USER_ACCESS_TOKENS
# RUN_ACCESS_TRAFFIC_BACKLOG_UAT BACKLOG_VALIDATE_ONLY BACKLOG_TARGET_INDEX
# BACKLOG_INSTALL_DIR BACKLOG_WAIT_SECONDS BACKLOG_TRAFFIC_ROUNDS BACKLOG_SKIP_AGENT_RESTART
# RUN_REAL_ACCESS_INBOUND_MATRIX_E2E REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS
# RUN_RUNTIME_LOADTEST RUNTIME_LOADTEST_DATABASE_URL RUNTIME_LOADTEST_PROFILE
# RUNTIME_HTTP_LOADTEST_PROFILE RUNTIME_HTTP_LOADTEST_REQUIRE_FULL
# RUN_AUTH_HA_UAT RUN_AUTH_HA_STABILITY_UAT AUTH_HA_STABILITY_PROFILE
# RUN_OPS_MISTAKE_RECOVERY_UAT RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT
# OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL

# shellcheck disable=SC1091
. "${SCRIPT_DIR}/lib/validate-real-release-env/rules.sh"
run_real_release_env_validation

if [[ "$failures" -gt 0 ]]; then
  printf 'real-release-env: failed with %s issue(s)\n' "$failures" >&2
  exit 2
fi

echo "real-release-env: passed"
