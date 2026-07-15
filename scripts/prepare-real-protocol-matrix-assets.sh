#!/usr/bin/env bash
# 用途：准备真实协议矩阵测试所需的私有 env 草稿和 inventory。
# 说明：主流程负责参数解析和编排，账号解析与远端探测逻辑拆到 lib。
# 安全：脚本输出保持脱敏，不打印真实主机、凭据、URL 或远端 stderr。
set -euo pipefail
IFS=$'\n\t'
set +x

umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
LIB_DIR="${SCRIPT_DIR}/lib/prepare-real-protocol-matrix-assets"

# shellcheck disable=SC1091
. "${LIB_DIR}/common.sh"
# shellcheck disable=SC1091
. "${LIB_DIR}/targets.sh"

cd "$BASE_DIR"

PRIVATE_DIR="${XRAYC_PRIVATE_DIR:-${BASE_DIR}/文档/私有}"
ACCOUNT_FILE="${XRAYC_SERVER_ACCOUNT_FILE:-}"
OUTPUT_ENV="${XRAYC_REAL_PROTOCOL_MATRIX_ENV_OUT:-}"
OUTPUT_INVENTORY="${XRAYC_REAL_PROTOCOL_MATRIX_INVENTORY_OUT:-}"
DEPLOY_UPSTREAM_LAB="${XRAYC_REAL_PROTOCOL_UPSTREAM_LAB:-0}"
CLEANUP_UPSTREAM_LAB=0
UPSTREAM_LAB_ENV="${XRAYC_REAL_PROTOCOL_UPSTREAM_LAB_ENV_OUT:-}"
RUN_ID="$(date +%Y%m%d%H%M%S)-$$"
TMP_DIR="$(mktemp -d)"
GAP_FILE="${TMP_DIR}/gaps.txt"
WARN_FILE="${TMP_DIR}/warnings.txt"
TARGETS_ENV="${TMP_DIR}/targets.env"
TARGETS_JSON="${TMP_DIR}/targets.json"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage:
  XRAYC_SERVER_ACCOUNT_FILE=/private/server-accounts.json \
    bash scripts/prepare-real-protocol-matrix-assets.sh

Options:
  --account-file PATH      Private server account file. JSON and dotenv-style
                           KEY=VALUE files are supported.
  --env-out PATH           Private env draft path. Default is a timestamped
                           file under XRAYC_PRIVATE_DIR or 文档/私有.
  --inventory-out PATH     Private generated inventory path. Default is a
                           timestamped file under XRAYC_PRIVATE_DIR or 文档/私有.
  --deploy-upstream-lab    Deploy real HTTP CONNECT, VLESS, Trojan,
                           Shadowsocks, and HY2 upstream samples on the
                           exit targets, then append the private env
                           fragment to the matrix env draft.
  --cleanup-upstream-lab   Clean only upstream lab resources previously
                           created on the exit targets, then exit.
  -h, --help               Show this help.

Supported account inputs:
  JSON:
    {"servers":{"server_1":{...},"server_2":{...},"server_3":{...}}}
    {"targets":[{"alias":"server_1",...},{"alias":"server_2",...},...]}

  Dotenv:
    SERVER_1_SSH_HOST=...
    SERVER_1_SSH_USER=root
    SERVER_1_SSH_PORT=22
    SERVER_1_SSH_PASSWORD_FILE points to a private password file
    SERVER_1_SSH_IDENTITY_FILE points to a private identity file

  Plain triples or domain blocks:
    <server_1_host>
    <server_1_user>
    <server_1_password>
    <server_1_public_domain>   # optional, preferred for TLS/SNI/public host
    Blank, # comment, and one-character separator lines are ignored.
    <server_2_host>
    <server_2_user>
    <server_2_password>
    <server_2_public_domain>   # optional
    <server_3_host>
    <server_3_user>
    <server_3_password>
    <server_3_public_domain>   # optional
    Additional server blocks are optional.

The script never prints real hosts, IPs, usernames, passwords, tokens, private
file contents, URLs, proxy URLs, or raw remote stderr. Output is limited to
server_N role labels, coarse capability status, and private draft
paths.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --account-file)
      ACCOUNT_FILE="${2:-}"
      shift 2
      ;;
    --env-out)
      OUTPUT_ENV="${2:-}"
      shift 2
      ;;
    --inventory-out)
      OUTPUT_INVENTORY="${2:-}"
      shift 2
      ;;
    --deploy-upstream-lab)
      DEPLOY_UPSTREAM_LAB=1
      shift
      ;;
    --cleanup-upstream-lab)
      CLEANUP_UPSTREAM_LAB=1
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

if [[ -z "$ACCOUNT_FILE" ]]; then
  record_gap "XRAYC_SERVER_ACCOUNT_FILE or --account-file is required"
elif [[ ! -f "$ACCOUNT_FILE" ]]; then
  record_gap "private server account file is missing"
fi

require_local_command python3
require_local_command ssh
if [[ "$DEPLOY_UPSTREAM_LAB" == "1" || "$CLEANUP_UPSTREAM_LAB" == "1" ]]; then
  require_local_command tar
fi

exit_if_gaps 0

mkdir -p "$PRIVATE_DIR"
if [[ -z "$OUTPUT_ENV" ]]; then
  OUTPUT_ENV="${PRIVATE_DIR}/real-protocol-matrix.${RUN_ID}.env"
fi
if [[ -z "$OUTPUT_INVENTORY" ]]; then
  OUTPUT_INVENTORY="${PRIVATE_DIR}/remote-e2e-inventory.${RUN_ID}.json"
fi

prepare_targets_from_account_file \
  "$ACCOUNT_FILE" \
  "$TARGETS_JSON" \
  "$TARGETS_ENV" \
  "$TMP_DIR" \
  "$PRIVATE_DIR" \
  "$RUN_ID"

# shellcheck disable=SC1090
. "$TARGETS_ENV"

validate_account_targets
exit_if_gaps 0

probe_remote_capabilities
exit_if_gaps 1

write_inventory_json "$TARGETS_JSON" "$OUTPUT_INVENTORY"

if [[ "$CLEANUP_UPSTREAM_LAB" == "1" ]]; then
  XRAYC_REAL_E2E_INVENTORY="$OUTPUT_INVENTORY" \
    bash scripts/real-protocol-upstream-lab.sh \
      --inventory "$OUTPUT_INVENTORY" \
      --cleanup
  echo "real protocol matrix assets: upstream lab cleanup completed"
  exit 0
fi

if [[ "$DEPLOY_UPSTREAM_LAB" == "1" ]]; then
  if [[ -z "$UPSTREAM_LAB_ENV" ]]; then
    UPSTREAM_LAB_ENV="${PRIVATE_DIR}/real-protocol-upstream-lab.${RUN_ID}.env"
  fi
  XRAYC_REAL_E2E_INVENTORY="$OUTPUT_INVENTORY" \
    XRAYC_REAL_PROTOCOL_UPSTREAM_LAB_ENV_OUT="$UPSTREAM_LAB_ENV" \
    bash scripts/real-protocol-upstream-lab.sh \
      --inventory "$OUTPUT_INVENTORY" \
      --env-out "$UPSTREAM_LAB_ENV"
fi

control_plane_placeholder="https://<real-control-plane-host>"
if [[ -n "${CONTROL_PLANE_URL1:-}" ]]; then
  control_plane_placeholder="<copy-from-private-server-account-file>"
fi

cat > "$OUTPUT_ENV" <<EOF
# Private real protocol matrix env draft.
# Generated by scripts/prepare-real-protocol-matrix-assets.sh.
# Keep this file untracked. Values below are placeholders or private paths only.

XRAYC_ENV="production"
SEED_DEMO_DATA="false"
BASE_URL="${control_plane_placeholder}"
XRAYC_SERVER_ACCOUNT_FILE=$(shell_quote "$ACCOUNT_FILE")
XRAYC_REAL_E2E_INVENTORY=$(shell_quote "$OUTPUT_INVENTORY")
XRAYC_REAL_E2E_TARGET="server_2"
REAL_PROTOCOL_MATRIX_PROTOCOLS="socks,http,vless,trojan,shadowsocks,hy2"
XRAYC_REAL_TEST_SERVER_COUNT="${TARGET_COUNT:-3}"

DEPLOY_ARTIFACT_TOKEN="<copy-from-private-file-or-admin-backend>"
SUB_TOKEN="<copy-from-private-file-or-admin-backend>"
AGENT_TOKEN="<copy-from-private-file-or-admin-backend>"
ACCESS_NODE_ID="<copy-from-admin-backend-or-database>"
ACCESS_LINE_ID="<copy-from-admin-backend-or-database>"
EXIT_ENDPOINT_ID="<copy-from-admin-backend-or-database>"
USER_ACCESS_TOKEN="<copy-from-private-file-or-admin-backend>"
ADMIN_LOGIN_ACCOUNT="<copy-from-private-control-plane>"
ADMIN_LOGIN_PASSWORD="<copy-from-private-control-plane>"
USER_LOGIN_ACCOUNT="<copy-from-private-control-plane>"
USER_LOGIN_PASSWORD="<copy-from-private-control-plane>"
EXPECTED_ACCESS_SERVERS="<copy-from-subscription-client-observation>"
DATABASE_URL="postgres://<user>:<password>@<host>:<port>/<database>"

CLIENT_PROXY_URL="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP="<copy-from-exit-target-observation>"

THIRD_PARTY_SOCKS_HOST="<exit-target-private-host>"
THIRD_PARTY_SOCKS_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_SOCKS_USERNAME="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_SOCKS_PASSWORD="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_SOCKS_KEY="<copy-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_SOCKS="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_SOCKS="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_SOCKS="<copy-from-admin-backend-or-database>"

THIRD_PARTY_HTTP_HOST="<exit-target-private-host>"
THIRD_PARTY_HTTP_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HTTP_USERNAME="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HTTP_PASSWORD="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HTTP_KEY="<copy-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_HTTP="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_HTTP="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_HTTP="<copy-from-admin-backend-or-database>"

THIRD_PARTY_VLESS_HOST="<exit-target-private-host>"
THIRD_PARTY_VLESS_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_UUID="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_UUID="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_SECURITY="<tls-or-reality-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_SERVER_NAME="<sni-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_SHORT_ID="<reality-short-id-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_FINGERPRINT="<client-fingerprint-from-private-provider-or-generated-exit>"
THIRD_PARTY_PUBLIC_KEY="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_VLESS_PUBLIC_KEY="<copy-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_VLESS="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_VLESS="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_VLESS="<copy-from-admin-backend-or-database>"

THIRD_PARTY_TROJAN_HOST="<exit-target-private-host>"
THIRD_PARTY_TROJAN_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_TROJAN_PASSWORD="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_TROJAN_KEY="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_TROJAN_SECURITY="tls"
THIRD_PARTY_TROJAN_SNI="<sni-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_TROJAN="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_TROJAN="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_TROJAN="<copy-from-admin-backend-or-database>"

THIRD_PARTY_SHADOWSOCKS_HOST="<exit-target-private-host>"
THIRD_PARTY_SHADOWSOCKS_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_SHADOWSOCKS_METHOD="<cipher-from-private-provider-or-generated-exit>"
THIRD_PARTY_SHADOWSOCKS_CIPHER="<cipher-from-private-provider-or-generated-exit>"
THIRD_PARTY_SHADOWSOCKS_PASSWORD="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_SHADOWSOCKS_KEY="<copy-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_SHADOWSOCKS="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_SHADOWSOCKS="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_SHADOWSOCKS="<copy-from-admin-backend-or-database>"

THIRD_PARTY_HY2_HOST="<exit-target-private-host>"
THIRD_PARTY_HY2_RAW_URL="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HY2_PASSWORD="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HY2_AUTH="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HY2_KEY="<copy-from-private-provider-or-generated-exit>"
THIRD_PARTY_HY2_OBFS_PASSWORD="<copy-from-private-provider-or-generated-exit>"
CLIENT_PROXY_URL_HY2="<copy-from-server_1-client-runtime>"
EXPECTED_EXIT_IP_HY2="<copy-from-exit-target-observation>"
EXIT_ENDPOINT_ID_HY2="<copy-from-admin-backend-or-database>"
EOF
if [[ "$DEPLOY_UPSTREAM_LAB" == "1" && -f "$UPSTREAM_LAB_ENV" ]]; then
  {
    echo
    echo "# Real upstream protocol lab values from exit targets."
    echo "# Keep these private; they include real hosts, credentials, raw URLs, and observed exit IPs."
    cat "$UPSTREAM_LAB_ENV"
  } >> "$OUTPUT_ENV"
fi
chmod 600 "$OUTPUT_ENV"

echo "real protocol matrix assets: ready"
echo "private env draft: ${OUTPUT_ENV}"
echo "private inventory draft: ${OUTPUT_INVENTORY}"
echo "next private env check: bash scripts/check-real-protocol-matrix-env.sh --env-file ${OUTPUT_ENV}"
if [[ -s "$WARN_FILE" ]]; then
  echo "warnings:"
  sed 's/.*/  - &/' "$WARN_FILE"
fi
