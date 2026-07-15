#!/usr/bin/env bash
# 这个脚本执行真实用户入站协议矩阵端到端验证。
# 这个脚本负责主流程编排、参数解析、环境加载和清理。
# 具体的 inventory、订阅准备、远程客户端逻辑已拆到 helper。
# 脚本输出只保留粗粒度状态，避免打印真实主机、IP、令牌或 URL。
# 运行前需要至少三台真实服务器 inventory、订阅令牌或订阅地址。
# 可选自动准备缺失入站协议并在退出时清理临时配置。
# 修改本脚本时需同步保持 helper 的 bash 语法校验通过。
# 主脚本不承载远程执行细节，避免单文件继续膨胀。
# helper 仅服务本真实入站矩阵脚本，不作为公共库承诺。
# 本脚本修改后必须保持自身行数低于 500 行。
set -euo pipefail
IFS=$'\n\t'
set +x

umask 077

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
SCRIPT_DIR="${BASE_DIR}/scripts"
HELPER_DIR="${SCRIPT_DIR}/lib/real-access-inbound-matrix"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/inventory-ssh.sh
source "${HELPER_DIR}/inventory-ssh.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/subscription-prepare.sh
source "${HELPER_DIR}/subscription-prepare.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/subscription-ports.sh
source "${HELPER_DIR}/subscription-ports.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/remote-client.sh
source "${HELPER_DIR}/remote-client.sh"

usage() {
  cat <<'USAGE'
usage: bash scripts/real-access-inbound-matrix-e2e.sh [--validate-only]

Loads .env.real-release or XRAYC_REAL_RELEASE_ENV_FILE unless
XRAYC_REAL_ACCESS_INBOUND_MATRIX_LOAD_ENV_FILE=0 is set.

Required:
  BASE_URL
  SUB_TOKEN or SUBSCRIPTION_URL
  XRAYC_REAL_E2E_INVENTORY with at least three distinct server_N targets

Optional:
  REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS    Default:
                                           vless,trojan,shadowsocks
  DATABASE_URL, ACCESS_LINE_ID            Used with SUB_TOKEN to self-prepare
                                           missing temporary inbound protocols
  REAL_ACCESS_INBOUND_MATRIX_AUTO_PREPARE Default: 1
  REAL_ACCESS_INBOUND_MATRIX_FORCE_PREPARE Default: 0, set 1 to prepare
                                           all matrix protocols instead of
                                           reusing existing subscription lines.
  REAL_ACCESS_INBOUND_MATRIX_TEMP_PORT_BASE Default: 34200
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_VLESS
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_TROJAN
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_SHADOWSOCKS
  REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET Default: server_1
                                           Current role: direct/non-CF target.
                                           Use server_4 only for Cloudflare
                                           entry validation.
  REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET Default: server_2, used to
                                           temporarily allow prepared ports.
  REAL_ACCESS_INBOUND_MATRIX_PUBLIC_IP_URL Default: PUBLIC_IP_URL or https://api.ipify.org
  REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_URL   Default: public IP URL
  REAL_ACCESS_INBOUND_MATRIX_CLIENT_PORT_BASE Default: 33180
  REAL_ACCESS_INBOUND_MATRIX_DOCKER_IMAGE  Default: xrayc/xray:local
  DEPLOY_ARTIFACT_TOKEN                    Used to load xray image when absent

The script prints coarse protocol status only. It does not print real hosts,
IPs, passwords, tokens, proxy URLs, subscription URLs, DB URLs, or curl output.
USAGE
}

VALIDATE_ONLY=0
if [[ "${1:-}" == "--validate-only" ]]; then
  VALIDATE_ONLY=1
  shift
fi
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
EXPLICIT_PROTOCOLS="${REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS:-}"
EXPLICIT_CLIENT_TARGET="${REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET:-}"
EXPLICIT_ACCESS_TARGET="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET:-}"
EXPLICIT_ACCESS_NODE_HINT="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_NODE_HINT:-}"
EXPLICIT_LISTEN_HOST="${REAL_ACCESS_INBOUND_MATRIX_LISTEN_HOST:-}"
if [[ "${XRAYC_REAL_ACCESS_INBOUND_MATRIX_LOAD_ENV_FILE:-1}" == "1" && -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi
[[ -n "$EXPLICIT_PROTOCOLS" ]] && REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS="$EXPLICIT_PROTOCOLS"
[[ -n "$EXPLICIT_CLIENT_TARGET" ]] && REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET="$EXPLICIT_CLIENT_TARGET"
[[ -n "$EXPLICIT_ACCESS_TARGET" ]] && REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET="$EXPLICIT_ACCESS_TARGET"
[[ -n "$EXPLICIT_ACCESS_NODE_HINT" ]] && REAL_ACCESS_INBOUND_MATRIX_ACCESS_NODE_HINT="$EXPLICIT_ACCESS_NODE_HINT"
[[ -n "$EXPLICIT_LISTEN_HOST" ]] && REAL_ACCESS_INBOUND_MATRIX_LISTEN_HOST="$EXPLICIT_LISTEN_HOST"

INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
PROTOCOLS="${REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS:-vless,trojan,shadowsocks}"
CLIENT_TARGET="${REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET:-server_1}"
ACCESS_TARGET="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET:-server_2}"
PUBLIC_IP_URL="${REAL_ACCESS_INBOUND_MATRIX_PUBLIC_IP_URL:-${PUBLIC_IP_URL:-https://api.ipify.org}}"
TRAFFIC_URL="${REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_URL:-$PUBLIC_IP_URL}"
CLIENT_PORT_BASE="${REAL_ACCESS_INBOUND_MATRIX_CLIENT_PORT_BASE:-33180}"
DOCKER_IMAGE="${REAL_ACCESS_INBOUND_MATRIX_DOCKER_IMAGE:-xrayc/xray:local}"
CURL_TIMEOUT="${CURL_TIMEOUT:-30}"
AUTO_PREPARE="${REAL_ACCESS_INBOUND_MATRIX_AUTO_PREPARE:-1}"
DEBUG_OUTPUT="${REAL_ACCESS_INBOUND_MATRIX_DEBUG:-0}"
DB_PREPARE_SCRIPT="${SCRIPT_DIR}/real-access-inbound-matrix-db-prepare.sh"

TMP_DIR="$(mktemp -d)"
TEMP_PREP_MANIFEST="$TMP_DIR/db-prepare.env"
TEMP_OPENED_PORTS=""
TEMP_OPENED_UDP_PORTS=""
CLIENT_CONFIG_PREFERRED_PORT_ARGS=()
completed_ok=0

cleanup() {
  local status=$?
  cleanup_prepared_access_ports
  if [[ -f "$TEMP_PREP_MANIFEST" && -f "$DB_PREPARE_SCRIPT" ]]; then
    bash "$DB_PREPARE_SCRIPT" cleanup "$TEMP_PREP_MANIFEST" >/dev/null 2>&1 || true
  fi
  if [[ "$VALIDATE_ONLY" != "1" && "$completed_ok" != "1" ]] && declare -F remote_exec >/dev/null 2>&1; then
    remote_exec "docker rm -f \$(docker ps -aq --filter name=xrayc-inbound-matrix-) >/dev/null 2>&1 || true; rm -rf /opt/xrayc-inbound-matrix" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

die() {
  echo "real_access_inbound_matrix_e2e: $*" >&2
  exit 1
}

die_usage() {
  echo "real_access_inbound_matrix_e2e: $*" >&2
  exit 2
}

status() {
  printf 'real_access_inbound_matrix_e2e: %s\n' "$1"
}

value_is_placeholder() {
  local value="${1:-}"
  value="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *mock* ]] && return 0
  [[ "$value" == *fake* ]] && return 0
  return 1
}

require_real_value() {
  local name="$1"
  local value="${2:-}"
  value_is_placeholder "$value" && die_usage "${name} must be a real private value"
  return 0
}

require_real_env() {
  local name="$1"
  require_real_value "$name" "${!name:-}"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die_usage "local ${1} command is missing"
}

normalize_protocol() {
  local protocol="$1"
  protocol="$(printf '%s' "$protocol" | tr '[:upper:]' '[:lower:]' | xargs)"
  protocol="${protocol//-/_}"
  case "$protocol" in
    ss) printf 'shadowsocks' ;;
    *) printf '%s' "$protocol" ;;
  esac
}

protocol_suffix() {
  case "$1" in
    vless) printf 'VLESS' ;;
    trojan) printf 'TROJAN' ;;
    shadowsocks) printf 'SHADOWSOCKS' ;;
    *) printf '' ;;
  esac
}

load_protocols() {
  local item protocol suffix
  protocol_names=()
  protocol_suffixes=()
  IFS=',' read -r -a protocol_items <<< "$PROTOCOLS"
  [[ "${#protocol_items[@]}" -gt 0 ]] || die_usage "REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS must not be empty"
  for item in "${protocol_items[@]}"; do
    protocol="$(normalize_protocol "$item")"
    suffix="$(protocol_suffix "$protocol")"
    [[ -n "$suffix" ]] || die_usage "REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS contains an unsupported protocol"
    protocol_names+=("$protocol")
    protocol_suffixes+=("$suffix")
  done
}

validate_base_env() {
  require_command python3
  require_command curl
  require_real_env BASE_URL
  xrayc_real_e2e_require_url_scheme BASE_URL "real inbound protocol matrix e2e"
  if [[ -n "${SUBSCRIPTION_URL:-}" ]]; then
    require_real_env SUBSCRIPTION_URL
    xrayc_real_e2e_require_url_scheme SUBSCRIPTION_URL "real inbound protocol matrix e2e"
  else
    require_real_env SUB_TOKEN
  fi
  require_real_env INVENTORY
  [[ -f "$INVENTORY" ]] || die_usage "XRAYC_REAL_E2E_INVENTORY file is missing"
  xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "real inbound protocol matrix e2e"
  xrayc_real_e2e_require_url_scheme TRAFFIC_URL "real inbound protocol matrix e2e"
  if [[ -n "${DATABASE_URL:-}" ]]; then
    require_real_env DATABASE_URL
    xrayc_real_e2e_require_url_scheme DATABASE_URL "real inbound protocol matrix e2e"
  fi
  if [[ -n "${ACCESS_LINE_ID:-}" ]]; then
    require_real_env ACCESS_LINE_ID
    [[ "$ACCESS_LINE_ID" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
      || die_usage "ACCESS_LINE_ID must be a UUID"
  fi
  [[ "$CLIENT_PORT_BASE" =~ ^[0-9]+$ && "$CLIENT_PORT_BASE" -gt 0 && "$CLIENT_PORT_BASE" -lt 65000 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_CLIENT_PORT_BASE must be a TCP port"
}

load_protocols
validate_base_env
load_inventory_client

if [[ "$VALIDATE_ONLY" == "1" ]]; then
  load_inventory_access_target
  status "validate_only_ok"
  completed_ok=1
  exit 0
fi

require_command ssh
build_ssh_command
if [[ "${REAL_ACCESS_INBOUND_MATRIX_FORCE_PREPARE:-0}" == "1" ]]; then
  prepare_missing_subscription_protocols "$PROTOCOLS"
else
  download_subscription
  prepare_missing_subscription_protocols "$(missing_subscription_protocols)"
fi
ports_output="$(build_client_configs)"
remote_prepare_image

for index in "${!protocol_names[@]}"; do
  protocol="${protocol_names[$index]}"
  suffix="${protocol_suffixes[$index]}"
  port="$(printf '%s\n' "$ports_output" | awk -F: -v p="$protocol" '$1 == p {print $2; exit}')"
  [[ -n "$port" ]] || die "missing generated client port"
  remote_run_protocol "$protocol" "$suffix" "$port"
done

completed_ok=1
status "completed"
