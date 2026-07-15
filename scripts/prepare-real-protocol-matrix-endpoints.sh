#!/usr/bin/env bash
# 用途：为真实协议矩阵批量创建缺失的出口 endpoint，并写回私有 env。
# 拆分：本文件只保留参数解析和主流程，具体实现位于同名专用 lib 目录。
# 安全：流程中禁止打印 token、密码、代理 URL、真实主机和 API 响应正文。
# 维护：新增逻辑优先放入 scripts/lib/prepare-real-protocol-matrix-endpoints/。
set -euo pipefail
IFS=$'\n\t'

if [[ $- == *x* ]]; then
  set +x
  echo "real-protocol-matrix-endpoints: disabled shell xtrace to avoid printing private values" >&2
fi

umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
LIB_DIR="${SCRIPT_DIR}/lib/prepare-real-protocol-matrix-endpoints"
cd "$BASE_DIR"
EXPLICIT_BASE_URL="${BASE_URL:-}"
EXPLICIT_ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-}"
EXPLICIT_ADMIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-}"
EXPLICIT_REAL_ADMIN_ACCOUNT="${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-}"
EXPLICIT_REAL_ADMIN_PASSWORD="${XRAYC_REAL_E2E_ADMIN_PASSWORD:-}"

ENV_FILE="${XRAYC_REAL_PROTOCOL_MATRIX_ENV_FILE:-${XRAYC_REAL_RELEASE_ENV_FILE:-}}"
PROTOCOLS_ARG=""
DRY_RUN=0
CURL_TIMEOUT="${CURL_TIMEOUT:-30}"
RUN_ID="$(date +%Y%m%d%H%M%S)-$$"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage:
  bash scripts/prepare-real-protocol-matrix-endpoints.sh --env-file /private/real-protocol-matrix.env

Options:
  --env-file PATH       Private env file to read and update. Defaults to
                        XRAYC_REAL_PROTOCOL_MATRIX_ENV_FILE, then
                        XRAYC_REAL_RELEASE_ENV_FILE.
  --protocols LIST      Comma-separated protocol list. Default is
                        REAL_PROTOCOL_MATRIX_PROTOCOLS from the env file,
                        then socks,http,vless,trojan,shadowsocks,hy2.
  --dry-run             Validate pending protocol inputs and payload building
                        without calling the control plane or writing env.
  -h, --help            Show this help.

Required private env values for creation:
  BASE_URL plus ADMIN_ACCESS_TOKEN, or ADMIN_LOGIN_PASSWORD with
  ADMIN_LOGIN_ACCOUNT.

Per-protocol private inputs:
  SOCKS:        THIRD_PARTY_SOCKS_RAW_URL or host/port/auth variables.
  HTTP:         THIRD_PARTY_HTTP_RAW_URL or host/port/auth variables.
  VLESS:        THIRD_PARTY_VLESS_RAW_URL or host/port/uuid/security variables.
  Trojan:       THIRD_PARTY_TROJAN_RAW_URL or host/port/password variables.
  Shadowsocks:  THIRD_PARTY_SHADOWSOCKS_RAW_URL or host/port/method/password.
  HY2:          THIRD_PARTY_HY2_RAW_URL or host/port/password/auth variables.

The script never prints tokens, passwords, raw provider/proxy URLs, hosts, real
IPs, database URLs, curl stderr, or response bodies. It prints only protocol
names, env variable names, and coarse status.
USAGE
}

# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/common.sh
. "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/api.sh
. "${LIB_DIR}/api.sh"
# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/payload.sh
. "${LIB_DIR}/payload.sh"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file)
      ENV_FILE="${2:-}"
      shift 2
      ;;
    --protocols)
      PROTOCOLS_ARG="${2:-}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "real-protocol-matrix-endpoints: unknown argument" >&2
      usage >&2
      exit 2
      ;;
  esac
done

require_command python3
require_command curl

[[ -n "$ENV_FILE" ]] || die "XRAYC_REAL_PROTOCOL_MATRIX_ENV_FILE, XRAYC_REAL_RELEASE_ENV_FILE, or --env-file is required"
[[ -f "$ENV_FILE" ]] || die "private env file is missing"

set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a
[[ -n "$EXPLICIT_BASE_URL" ]] && BASE_URL="$EXPLICIT_BASE_URL"
[[ -n "$EXPLICIT_ADMIN_ACCOUNT" ]] && ADMIN_LOGIN_ACCOUNT="$EXPLICIT_ADMIN_ACCOUNT"
[[ -n "$EXPLICIT_ADMIN_PASSWORD" ]] && ADMIN_LOGIN_PASSWORD="$EXPLICIT_ADMIN_PASSWORD"
[[ -n "$EXPLICIT_REAL_ADMIN_ACCOUNT" ]] && XRAYC_REAL_E2E_ADMIN_ACCOUNT="$EXPLICIT_REAL_ADMIN_ACCOUNT"
[[ -n "$EXPLICIT_REAL_ADMIN_PASSWORD" ]] && XRAYC_REAL_E2E_ADMIN_PASSWORD="$EXPLICIT_REAL_ADMIN_PASSWORD"
if [[ -n "$EXPLICIT_ADMIN_PASSWORD" || -n "$EXPLICIT_REAL_ADMIN_PASSWORD" ]]; then
  ADMIN_ACCESS_TOKEN=""
fi

PROTOCOLS="${PROTOCOLS_ARG:-${REAL_PROTOCOL_MATRIX_PROTOCOLS:-socks,http,vless,trojan,shadowsocks,hy2}}"
IFS=',' read -r -a protocol_items <<< "$PROTOCOLS"
[[ "${#protocol_items[@]}" -gt 0 ]] || die "protocol list is empty"

pending_protocols=()
pending_suffixes=()
pending_payloads=()
pending_actions=()
pending_endpoint_ids=()
for item in "${protocol_items[@]}"; do
  protocol="$(normalize_protocol "$item")"
  [[ -n "$protocol" ]] || die "protocol list contains an empty item"
  suffix="$(protocol_suffix "$protocol")"
  [[ -n "$suffix" ]] || die "protocol list contains an unsupported protocol"
  endpoint_var="EXIT_ENDPOINT_ID_${suffix}"
  endpoint_value="${!endpoint_var:-}"
  payload_file="${TMP_DIR}/payload-${protocol}.json"
  build_endpoint_payload "$protocol" "$payload_file"
  action="create"
  existing_endpoint_id=""
  if value_ready "$endpoint_value"; then
    require_uuid_value "$endpoint_var" "$endpoint_value"
    if endpoint_exists_in_database "$endpoint_value"; then
      action="update"
      existing_endpoint_id="$endpoint_value"
      if endpoint_payload_matches_database "$endpoint_value" "$payload_file"; then
        status "will refresh ${endpoint_var} (already set and matches current non-secret input)"
      else
        status "will update ${endpoint_var} (existing value does not match current private input)"
      fi
    else
      status "will recreate ${endpoint_var} (existing value not found)"
    fi
  else
    status "will create ${endpoint_var} (value missing)"
  fi
  pending_protocols+=("$protocol")
  pending_suffixes+=("$suffix")
  pending_payloads+=("$payload_file")
  pending_actions+=("$action")
  pending_endpoint_ids+=("$existing_endpoint_id")
done

if [[ "${#pending_protocols[@]}" -eq 0 ]]; then
  status "completed; no endpoint variables needed updates"
  exit 0
fi

if [[ "$DRY_RUN" == "1" ]]; then
  for index in "${!pending_protocols[@]}"; do
    protocol="${pending_protocols[$index]}"
    suffix="${pending_suffixes[$index]}"
    status "dry-run validated EXIT_ENDPOINT_ID_${suffix}"
  done
  status "dry-run completed without writing private env"
  exit 0
fi

BASE_URL="${BASE_URL:-}"
require_ready_value BASE_URL
if [[ "$BASE_URL" != https://* && "$BASE_URL" != http://localhost* && "$BASE_URL" != http://127.0.0.1* && "${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-${ALLOW_INSECURE_HTTP_E2E:-0}}" != "1" ]]; then
  die "BASE_URL must use HTTPS unless explicitly allowing local/insecure real E2E"
fi

admin_token="$(resolve_admin_token)"

for index in "${!pending_protocols[@]}"; do
  protocol="${pending_protocols[$index]}"
  suffix="${pending_suffixes[$index]}"
  endpoint_var="EXIT_ENDPOINT_ID_${suffix}"
  aggregate_payload="${pending_payloads[$index]}"
  action="${pending_actions[$index]}"
  existing_endpoint_id="${pending_endpoint_ids[$index]}"
  resource_payload="${TMP_DIR}/resource-${protocol}.json"
  endpoint_payload="${TMP_DIR}/endpoint-${protocol}.json"
  resource_body="${TMP_DIR}/resource-body-${protocol}.json"
  endpoint_body="${TMP_DIR}/endpoint-body-${protocol}.json"
  resource_id=""

  if [[ "$action" == "update" ]]; then
    python3 - "$aggregate_payload" "$endpoint_payload" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
endpoint = {
    "name": payload.get("endpoint_name", ""),
    "outbound_type": payload["outbound_type"],
    "host": payload["host"],
    "port": payload["port"],
    "outbound_config": payload.get("outbound_config", {}),
    "stream_config": payload.get("stream_config", {}),
    "probe_config": payload.get("probe_config", {}),
    "enabled": payload.get("enabled", True),
}
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(endpoint, fh, ensure_ascii=False)
PY
    chmod 600 "$endpoint_payload"
    api_json PUT "/api/admin/exit-endpoints/${existing_endpoint_id}" "$endpoint_payload" "$endpoint_body" "$admin_token"
    refresh_existing_endpoint_health "$existing_endpoint_id"
    status "updated ${endpoint_var}"
    continue
  fi

  python3 - "$aggregate_payload" "$resource_payload" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
resource = {
    "name": payload["resource_name"],
    "region_code": payload.get("region_code", ""),
    "provider_name": payload.get("provider_name", ""),
    "ownership": payload.get("ownership", "third_party"),
    "enabled": payload.get("enabled", True),
}
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(resource, fh, ensure_ascii=False)
PY
  chmod 600 "$resource_payload"
  api_json POST "/api/admin/exit-resources" "$resource_payload" "$resource_body" "$admin_token"
  resource_id="$(json_value "$resource_body" id)" || die "control plane response missed resource id for ${endpoint_var}"
  RESOURCE_ID_VALUE="$resource_id" python3 - "$aggregate_payload" "$endpoint_payload" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
endpoint = {
    "exit_resource_id": os.environ["RESOURCE_ID_VALUE"],
    "name": payload.get("endpoint_name", ""),
    "outbound_type": payload["outbound_type"],
    "host": payload["host"],
    "port": payload["port"],
    "outbound_config": payload.get("outbound_config", {}),
    "stream_config": payload.get("stream_config", {}),
    "probe_config": payload.get("probe_config", {}),
    "enabled": payload.get("enabled", True),
}
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(endpoint, fh, ensure_ascii=False)
PY
  chmod 600 "$endpoint_payload"
  api_json POST "/api/admin/exit-endpoints" "$endpoint_payload" "$endpoint_body" "$admin_token"
  endpoint_id="$(json_value "$endpoint_body" id)" || die "control plane response missed ${endpoint_var}"
  require_uuid_value "$endpoint_var" "$endpoint_id"
  upsert_env_value "$endpoint_var" "$endpoint_id"
  status "wrote ${endpoint_var}"
done

status "completed without printing private values"
