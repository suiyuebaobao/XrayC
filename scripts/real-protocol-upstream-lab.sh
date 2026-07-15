#!/usr/bin/env bash
# 用途：真实协议上游实验入口，负责参数解析、校验和调用拆分后的库函数。
# 该脚本部署或清理出口目标上的 HTTP、SOCKS、VLESS、Trojan、Shadowsocks、HY2 示例。
# 敏感主机、凭据和代理 URL 仅写入私有输出文件，终端输出保持脱敏。
set -euo pipefail
IFS=$'\n\t'
set +x

umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
LIB_DIR="${SCRIPT_DIR}/lib/real-protocol-upstream-lab"

# shellcheck disable=SC1091
. "${LIB_DIR}/common.sh"
# shellcheck disable=SC1091
. "${LIB_DIR}/payload.sh"
# shellcheck disable=SC1091
. "${LIB_DIR}/remote.sh"

cd "$BASE_DIR"

PRIVATE_DIR="${XRAYC_PRIVATE_DIR:-${BASE_DIR}/文档/私有}"
INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
ENV_OUT="${XRAYC_REAL_PROTOCOL_UPSTREAM_LAB_ENV_OUT:-}"
ACTION="deploy"
RUN_ID="$(date +%Y%m%d%H%M%S)-$$"
TMP_DIR="$(mktemp -d)"
TARGETS_ENV="${TMP_DIR}/targets.env"
ENV_VALUES_JSON="${TMP_DIR}/env-values.json"
HTTP_PORT="${XRAYC_REAL_PROTOCOL_HTTP_PORT:-38080}"
SOCKS_PORT="${XRAYC_REAL_PROTOCOL_SOCKS_PORT:-38081}"
VLESS_PORT="${XRAYC_REAL_PROTOCOL_VLESS_PORT:-38443}"
SHADOWSOCKS_PORT="${XRAYC_REAL_PROTOCOL_SHADOWSOCKS_PORT:-38388}"
TROJAN_PORT="${XRAYC_REAL_PROTOCOL_TROJAN_PORT:-39443}"
HY2_PORT="${XRAYC_REAL_PROTOCOL_HY2_PORT:-39444}"
ACME_EMAIL="${XRAYC_REAL_PROTOCOL_ACME_EMAIL:-${XRAYC_TLS_CERT_EMAIL:-}}"
XRAY_IMAGE="${XRAYC_REAL_PROTOCOL_XRAY_IMAGE:-ghcr.io/xtls/xray-core:latest}"
HYSTERIA_IMAGE="${XRAYC_REAL_PROTOCOL_HYSTERIA_IMAGE:-tobyxdd/hysteria:latest}"
REMOTE_ROOT="${XRAYC_REAL_PROTOCOL_UPSTREAM_REMOTE_ROOT:-/opt/xrayc-real-protocol-upstream-lab}"
PROJECT_PREFIX="${XRAYC_REAL_PROTOCOL_UPSTREAM_PROJECT_PREFIX:-xrayc-real-protocol-upstream-lab}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage:
  XRAYC_REAL_E2E_INVENTORY=/private/remote-e2e-inventory.json \
    bash scripts/real-protocol-upstream-lab.sh

Options:
  --inventory PATH       Private inventory with at least server_1..server_3 targets.
  --env-out PATH         Private env fragment output path.
  --cleanup              Remove only resources created by this lab script.
  -h, --help             Show this help.

The script deploys real upstream protocol samples on exit targets:
SOCKS, HTTP CONNECT, VLESS, Trojan, Shadowsocks, and Hysteria/HY2. It uses Docker
Compose, Xray, and Hysteria images. It never prints real hosts, IPs,
passwords, tokens, raw proxy URLs, private file contents, or raw remote stderr.
TLS certificate requests require XRAYC_REAL_PROTOCOL_ACME_EMAIL or XRAYC_TLS_CERT_EMAIL.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --inventory)
      INVENTORY="${2:-}"
      shift 2
      ;;
    --env-out)
      ENV_OUT="${2:-}"
      shift 2
      ;;
    --cleanup)
      ACTION="cleanup"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument" >&2
      usage >&2
      exit 2
      ;;
  esac
done

require_command python3
require_command ssh
require_command tar

[[ -n "$INVENTORY" ]] || die "XRAYC_REAL_E2E_INVENTORY or --inventory is required"
[[ -f "$INVENTORY" ]] || die "private inventory file is missing"

mkdir -p "$PRIVATE_DIR"
if [[ -z "$ENV_OUT" ]]; then
  ENV_OUT="${PRIVATE_DIR}/real-protocol-upstream-lab.${RUN_ID}.env"
fi

load_inventory_targets
validate_ssh_auth_files

if [[ "$ACTION" == "cleanup" ]]; then
  cleanup_remote BASIC_EXIT basic-exit
  cleanup_remote TLS_EXIT tls-exit
  status "cleanup completed"
  exit 0
fi

require_command openssl
generate_env_values

status "deploying basic exit protocols"
deploy_remote BASIC_EXIT basic-exit
status "deploying TLS/UDP exit protocols"
deploy_remote TLS_EXIT tls-exit

basic_exit_ip="$(remote_public_ip BASIC_EXIT)"
tls_exit_ip="$(remote_public_ip TLS_EXIT)"
write_env_output "$basic_exit_ip" "$tls_exit_ip"

status "private env fragment written"
echo "private upstream env: ${ENV_OUT}"
status "completed"
