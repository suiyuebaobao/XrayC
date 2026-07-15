#!/usr/bin/env bash
# 用途：为 check-real-protocol-matrix-env.sh 提供环境校验函数。
# 用途：集中保存占位符、URL、主机、端口和协议分组检查。
# 用途：该文件只定义函数，依赖入口脚本提供 FAILURES 与环境变量。

report() {
  printf 'real-protocol-matrix-env: %s\n' "$1" >&2
  FAILURES=$((FAILURES + 1))
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-host* ]] && return 0
  [[ "$value" == *private-file* ]] && return 0
  [[ "$value" == *provider-console* ]] && return 0
  [[ "$value" == *generated-exit* ]] && return 0
  [[ "$value" == *client-runtime* ]] && return 0
  [[ "$value" == *observation* ]] && return 0
  [[ "$value" == *admin-backend* ]] && return 0
  [[ "$value" == *database* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  return 1
}

value_ready() {
  local value="${1:-}"
  [[ -n "$value" ]] || return 1
  value_is_placeholder "$value" && return 1
  return 0
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

require_any_value() {
  local label="$1"
  shift
  local name found=0 placeholder=0

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

require_any_ready_value() {
  local label="$1"
  shift
  local name

  for name in "$@"; do
    if value_ready "${!name:-}"; then
      return 0
    fi
  done

  report "${label} is missing"
}

require_url_scheme() {
  local name="$1"
  local value="${!name:-}"

  require_value "$name"
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

require_url_scheme_if_ready() {
  local name="$1"
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
    report "Trojan endpoints must set THIRD_PARTY_TROJAN_SECURITY=tls"
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

require_hy2_tls_ready() {
  local raw_url="${THIRD_PARTY_HY2_RAW_URL:-}"
  local allow_insecure="${THIRD_PARTY_HY2_ALLOW_INSECURE:-}"
  local sni="${THIRD_PARTY_HY2_SNI:-${THIRD_PARTY_HY2_SERVER_NAME:-}}"
  local parsed_flags=""

  if value_ready "$allow_insecure" && [[ "${allow_insecure,,}" =~ ^(1|true|yes)$ ]]; then
    report "THIRD_PARTY_HY2_ALLOW_INSECURE must not be enabled because release outbound TLS is verified"
  fi

  if value_ready "$raw_url"; then
    parsed_flags="$(
      python3 - "$raw_url" <<'PY' 2>/dev/null || true
from urllib.parse import parse_qs, urlparse
import sys

parsed = urlparse(sys.argv[1])
query = parse_qs(parsed.query)
has_sni = bool(
    (query.get("sni", [""])[0] or "").strip()
    or (query.get("servername", [""])[0] or "").strip()
    or (query.get("serverName", [""])[0] or "").strip()
    or (query.get("peer", [""])[0] or "").strip()
)
allow_insecure = (
    (query.get("insecure", [""])[0] or "").strip().lower()
    or (query.get("allow_insecure", [""])[0] or "").strip().lower()
    or (query.get("allowInsecure", [""])[0] or "").strip().lower()
)
print(f"has_sni={int(has_sni)}")
print(f"allow_insecure={allow_insecure}")
PY
    )"
    if grep -Eq '^allow_insecure=(1|true|yes)$' <<<"$parsed_flags"; then
      report "HY2 raw URL must not use insecure/allow_insecure because release outbound TLS is verified"
    fi
    if grep -q '^has_sni=0$' <<<"$parsed_flags" && ! value_ready "$sni"; then
      report "HY2 TLS requires THIRD_PARTY_HY2_SERVER_NAME/SNI or a raw URL with SNI"
    fi
  fi

  if ! value_ready "$sni" && ! value_ready "$raw_url"; then
    report "HY2 TLS requires THIRD_PARTY_HY2_SERVER_NAME/SNI or a raw URL with SNI"
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

require_uuid() {
  local name="$1"
  local value="${!name:-}"

  require_value "$name"
  value_ready "$value" || return 0
  if [[ ! "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
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

require_endpoint_uuid_for_matrix() {
  local name="$1"
  if [[ "${ALLOW_MISSING_ENDPOINTS:-0}" == "1" && "${MATRIX_MODE:-relay}" == "relay" ]]; then
    require_uuid_if_ready "$name"
  else
    require_uuid "$name"
  fi
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
  local value="${!name:-}"

  require_value "$name"
  value_ready "$value" || return 0
  if [[ ! -f "$value" ]]; then
    report "${name} file does not exist"
  fi
}

require_deploy_token_source() {
  if value_ready "${DEPLOY_ARTIFACT_TOKEN:-}" || value_ready "${XRAYC_DEPLOY_ARTIFACT_TOKEN:-}"; then
    return 0
  fi
  XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE="${XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE:-文档/私有/deploy-artifact-token.env}"
  require_file XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE
}

check_inventory() {
  local inventory="${XRAYC_REAL_E2E_INVENTORY:-}"
  local required_count="${XRAYC_REAL_TEST_SERVER_COUNT:-3}"

  require_file XRAYC_REAL_E2E_INVENTORY
  value_ready "$inventory" || return 0
  [[ -f "$inventory" ]] || return 0
  [[ "$required_count" =~ ^[1-9][0-9]*$ ]] || required_count=3
  if (( required_count < 3 )); then
    required_count=3
  fi

  if ! python3 - "$inventory" "$required_count" <<'PY' >/dev/null 2>&1
import json
import sys

inventory, required_raw = sys.argv[1:3]
required = int(required_raw)
with open(inventory, "r", encoding="utf-8") as fh:
    payload = json.load(fh)

targets = payload.get("targets", [])
if len(targets) < required:
    raise SystemExit(1)

hosts = []
for index, target in enumerate(targets[:required], 1):
    if target.get("alias") != f"server_{index}":
        raise SystemExit(1)
    for key in ("ssh_host", "ssh_user", "ssh_port"):
        if not str(target.get(key, "")).strip():
            raise SystemExit(1)
    if not target.get("ssh_password_file") and not target.get("ssh_identity_file"):
        raise SystemExit(1)
    hosts.append(str(target.get("ssh_host", "")).strip())

if len(set(hosts)) != required:
    raise SystemExit(1)
PY
  then
    report "XRAYC_REAL_E2E_INVENTORY must contain the required distinct server_N SSH targets"
    return 0
  fi

  if ! python3 - "$inventory" "$required_count" <<'PY' >/dev/null 2>&1
import json
import os
import sys

inventory, required_raw = sys.argv[1:3]
required = int(required_raw)
with open(inventory, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])[:required]

for target in targets:
    for key in ("ssh_password_file", "ssh_identity_file"):
        path = str(target.get(key, "")).strip()
        if path and not os.path.isfile(path):
            raise SystemExit(1)
PY
  then
    report "XRAYC_REAL_E2E_INVENTORY references a missing SSH auth file"
  fi
}

protocol_suffix() {
  case "$1" in
    socks|socks5) printf 'SOCKS' ;;
    http) printf 'HTTP' ;;
    vless) printf 'VLESS' ;;
    trojan) printf 'TROJAN' ;;
    shadowsocks|ss) printf 'SHADOWSOCKS' ;;
    hy2|hysteria|hysteria2) printf 'HY2' ;;
    *) printf '' ;;
  esac
}

normalize_protocol() {
  local protocol="$1"
  protocol="$(printf '%s' "$protocol" | tr '[:upper:]' '[:lower:]' | xargs)"
  case "$protocol" in
    socks5) printf 'socks' ;;
    ss) printf 'shadowsocks' ;;
    hysteria|hysteria2) printf 'hy2' ;;
    *) printf '%s' "$protocol" ;;
  esac
}

require_raw_or_host_port() {
  local label="$1"
  local raw_name="$2"
  local host_name="$3"
  local port_name="$4"

  if value_ready "${!raw_name:-}"; then
    require_url_scheme_if_ready "$raw_name"
    return 0
  fi

  require_value "$host_name"
  require_value "$port_name"
  require_host_if_ready "$host_name"
  require_port_if_ready "$port_name"
  if ! value_ready "${!host_name:-}" || ! value_ready "${!port_name:-}"; then
    report "${label} requires either ${raw_name} or ${host_name}+${port_name}"
  fi
}

shadowsocks_key_includes_method() {
  local key="${THIRD_PARTY_SHADOWSOCKS_KEY:-}"
  value_ready "$key" || return 1
  python3 - "$key" <<'PY' >/dev/null 2>&1
import base64
import sys

value = sys.argv[1].strip()
candidates = [value]
padding = "=" * (-len(value) % 4)
for decoder in (base64.urlsafe_b64decode, base64.b64decode):
    try:
        candidates.append(decoder(value + padding).decode("utf-8", "ignore"))
    except Exception:
        pass
for candidate in candidates:
    method, sep, password = candidate.partition(":")
    if sep and method.strip() and password.strip():
        raise SystemExit(0)
raise SystemExit(1)
PY
}

require_protocol_group() {
  local protocol="$1"
  local suffix=""

  suffix="$(protocol_suffix "$protocol")"
  if [[ -z "$suffix" ]]; then
    report "REAL_PROTOCOL_MATRIX_PROTOCOLS contains unsupported protocol"
    return
  fi

  case "$protocol" in
    socks)
      require_raw_or_host_port "SOCKS" THIRD_PARTY_SOCKS_RAW_URL THIRD_PARTY_SOCKS_HOST THIRD_PARTY_SOCKS_PORT
      ;;
    http)
      require_raw_or_host_port "HTTP" THIRD_PARTY_HTTP_RAW_URL THIRD_PARTY_HTTP_HOST THIRD_PARTY_HTTP_PORT
      require_any_ready_value "THIRD_PARTY_HTTP_PASSWORD, THIRD_PARTY_HTTP_KEY, or raw URL credentials" THIRD_PARTY_HTTP_PASSWORD THIRD_PARTY_HTTP_KEY THIRD_PARTY_HTTP_RAW_URL
      ;;
    vless)
      if value_ready "${THIRD_PARTY_VLESS_RAW_URL:-}"; then
        require_url_scheme_if_ready THIRD_PARTY_VLESS_RAW_URL
      else
        require_any_value "THIRD_PARTY_HOST or THIRD_PARTY_VLESS_HOST" THIRD_PARTY_HOST THIRD_PARTY_VLESS_HOST
        require_host_if_ready THIRD_PARTY_HOST
        require_host_if_ready THIRD_PARTY_VLESS_HOST
        require_value THIRD_PARTY_VLESS_PORT
        require_port_if_ready THIRD_PARTY_VLESS_PORT
        require_any_ready_value "THIRD_PARTY_UUID or THIRD_PARTY_VLESS_UUID" THIRD_PARTY_UUID THIRD_PARTY_VLESS_UUID
      fi
      if [[ "${THIRD_PARTY_SECURITY:-${THIRD_PARTY_VLESS_SECURITY:-}}" == "reality" ]] \
        || value_ready "${THIRD_PARTY_PUBLIC_KEY:-}" \
        || value_ready "${THIRD_PARTY_VLESS_PUBLIC_KEY:-}"; then
        require_any_ready_value "THIRD_PARTY_PUBLIC_KEY or THIRD_PARTY_VLESS_PUBLIC_KEY" THIRD_PARTY_PUBLIC_KEY THIRD_PARTY_VLESS_PUBLIC_KEY
      fi
      ;;
    trojan)
      require_raw_or_host_port "Trojan" THIRD_PARTY_TROJAN_RAW_URL THIRD_PARTY_TROJAN_HOST THIRD_PARTY_TROJAN_PORT
      require_any_ready_value "THIRD_PARTY_TROJAN_PASSWORD, THIRD_PARTY_TROJAN_KEY, or raw URL credentials" THIRD_PARTY_TROJAN_PASSWORD THIRD_PARTY_TROJAN_KEY THIRD_PARTY_TROJAN_RAW_URL
      require_trojan_tls_ready
      ;;
    shadowsocks|ss)
      require_raw_or_host_port "Shadowsocks" THIRD_PARTY_SHADOWSOCKS_RAW_URL THIRD_PARTY_SHADOWSOCKS_HOST THIRD_PARTY_SHADOWSOCKS_PORT
      require_any_ready_value "THIRD_PARTY_SHADOWSOCKS_PASSWORD, THIRD_PARTY_SHADOWSOCKS_KEY, or raw URL credentials" THIRD_PARTY_SHADOWSOCKS_PASSWORD THIRD_PARTY_SHADOWSOCKS_KEY THIRD_PARTY_SHADOWSOCKS_RAW_URL
      if ! value_ready "${THIRD_PARTY_SHADOWSOCKS_RAW_URL:-}" \
        && ! value_ready "${THIRD_PARTY_SHADOWSOCKS_METHOD:-}" \
        && ! value_ready "${THIRD_PARTY_SHADOWSOCKS_CIPHER:-}" \
        && ! shadowsocks_key_includes_method; then
        report "Shadowsocks without raw URL requires THIRD_PARTY_SHADOWSOCKS_METHOD/CIPHER or a method:password key"
      fi
      ;;
    hy2|hysteria|hysteria2)
      require_raw_or_host_port "HY2" THIRD_PARTY_HY2_RAW_URL THIRD_PARTY_HY2_HOST THIRD_PARTY_HY2_PORT
      require_any_ready_value "THIRD_PARTY_HY2_PASSWORD, THIRD_PARTY_HY2_AUTH, THIRD_PARTY_HY2_KEY, or raw URL credentials" THIRD_PARTY_HY2_PASSWORD THIRD_PARTY_HY2_AUTH THIRD_PARTY_HY2_KEY THIRD_PARTY_HY2_RAW_URL
      require_hy2_tls_ready
      ;;
  esac

  require_public_ip "EXPECTED_EXIT_IP_${suffix}"
  require_endpoint_uuid_for_matrix "EXIT_ENDPOINT_ID_${suffix}"
  if [[ "$MATRIX_MODE" == "direct" ]]; then
    require_url_scheme "CLIENT_PROXY_URL_${suffix}"
  fi
}
