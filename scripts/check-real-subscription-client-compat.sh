#!/usr/bin/env bash
# 中文说明：真实订阅客户端兼容门禁，校验 Clash/mihomo YAML 可被真实客户端解析。
# 中文说明：脚本下载订阅但不打印订阅 URL、token、节点地址或代理凭据。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/inventory-ssh.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/inventory-ssh.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/subscription-prepare.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/subscription-prepare.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/subscription-ports.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/subscription-ports.sh"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

bool_enabled() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
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
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *mock* ]] && return 0
  [[ "$value" == *fake* ]] && return 0
  return 1
}

require_value() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "check-real-subscription-client-compat: ${name} is required" >&2
    exit 1
  fi
}

die_usage() {
  echo "check-real-subscription-client-compat: $*" >&2
  exit 2
}

die() {
  echo "check-real-subscription-client-compat: $*" >&2
  exit 1
}

status() {
  echo "check-real-subscription-client-compat: $*"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die_usage "local ${1} command is missing"
}

subscription_url() {
  if [[ -n "${SUBSCRIPTION_URL:-}" ]]; then
    if [[ "$SUBSCRIPTION_URL" == /* ]]; then
      require_value BASE_URL
      printf '%s%s\n' "${BASE_URL%/}" "$SUBSCRIPTION_URL"
    else
      printf '%s\n' "$SUBSCRIPTION_URL"
    fi
    return
  fi
  require_value BASE_URL
  require_value SUB_TOKEN
  printf '%s/sub/%s\n' "${BASE_URL%/}" "$SUB_TOKEN"
}

validate_subscription_yaml() {
  python3 - "$1" <<'PY'
import sys
import yaml

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    profile = yaml.safe_load(fh)
if not isinstance(profile, dict):
    raise SystemExit("subscription profile must be a YAML mapping")
proxies = profile.get("proxies")
groups = profile.get("proxy-groups")
rules = profile.get("rules")
if not isinstance(proxies, list) or not proxies:
    raise SystemExit("subscription profile must contain at least one proxy")
if not isinstance(groups, list) or not groups:
    raise SystemExit("subscription profile must contain at least one proxy group")
if not isinstance(rules, list):
    raise SystemExit("subscription profile rules must be a list")
for index, proxy in enumerate(proxies, 1):
    if not isinstance(proxy, dict):
        raise SystemExit(f"proxy #{index} must be a mapping")
    for key in ("name", "type", "server", "port"):
        value = proxy.get(key)
        if value is None or str(value).strip() == "":
            raise SystemExit(f"proxy #{index} missing {key}")
    try:
        port = int(proxy["port"])
    except (TypeError, ValueError):
        raise SystemExit(f"proxy #{index} has invalid port")
    if port <= 0 or port > 65535:
        raise SystemExit(f"proxy #{index} has invalid port")
print(f"subscription YAML structure ok: proxies={len(proxies)} groups={len(groups)} rules={len(rules)}")
PY
}

run_mihomo_or_clash_check() {
  local profile="$1"
  if command -v mihomo >/dev/null 2>&1; then
    mihomo -t -f "$profile" >/dev/null 2>&1 || {
      echo "mihomo subscription check failed" >&2
      exit 1
    }
    echo "mihomo subscription check ok"
    return
  fi
  if command -v clash >/dev/null 2>&1; then
    clash -t -f "$profile" >/dev/null 2>&1 || {
      echo "clash subscription check failed" >&2
      exit 1
    }
    echo "clash subscription check ok"
    return
  fi
  if command -v docker >/dev/null 2>&1; then
    local image="${XRAYC_MIHOMO_IMAGE:-metacubex/mihomo:latest}"
    docker run --rm -v "$(dirname "$profile"):/work:ro" "$image" -t -f /work/subscription.yaml >/dev/null 2>&1 || {
      echo "mihomo Docker subscription check failed" >&2
      exit 1
    }
    echo "mihomo Docker subscription check ok"
    return
  fi
  echo "mihomo/clash checker is missing and Docker is unavailable" >&2
  exit 1
}

# shellcheck source=scripts/lib/real-subscription-client-compat/traffic.sh
source "${SCRIPT_DIR}/lib/real-subscription-client-compat/traffic.sh"

if ! bool_enabled "${RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT:-1}"; then
  echo "check-real-subscription-client-compat: skipped"
  exit 0
fi

tmp_dir="$(mktemp -d)"
compat_prepare_manifest="${REAL_SUBSCRIPTION_CLIENT_COMPAT_PREPARE_MANIFEST:-}"

cleanup_compat_subscription_prepare() {
  TMP_DIR="$tmp_dir"
  cleanup_prepared_access_ports >/dev/null 2>&1 || true
  if bool_enabled "${REAL_SUBSCRIPTION_CLIENT_COMPAT_KEEP_PREPARE:-0}"; then
    rm -rf "$tmp_dir"
    return 0
  fi
  if [[ -n "$compat_prepare_manifest" && -f "$compat_prepare_manifest" ]]; then
    bash scripts/real-access-inbound-matrix-db-prepare.sh cleanup "$compat_prepare_manifest" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}

trap cleanup_compat_subscription_prepare EXIT
profile="$tmp_dir/subscription.yaml"
mihomo_runtime_profile="$tmp_dir/mihomo-run.yaml"

FETCH_STATUS=""
FETCH_CURL_EXIT=0

fetch_subscription_profile() {
  local url="$1"
  local output_file="$2"
  local config_file="$tmp_dir/subscription-url.curl.conf"

  xrayc_real_e2e_write_curl_url_config "$url" "$config_file"
  set +e
  FETCH_STATUS="$(curl --fail --silent --location --max-time "${CURL_TIMEOUT:-30}" \
    --output "$output_file" --write-out '%{http_code}' --config "$config_file" 2>/dev/null)"
  FETCH_CURL_EXIT=$?
  set -e
  [[ "$FETCH_CURL_EXIT" -eq 0 && "$FETCH_STATUS" == 2* ]]
}

extract_sub_token_from_url_if_needed() {
  [[ -z "${SUB_TOKEN:-}" && -n "${SUBSCRIPTION_URL:-}" && "$SUBSCRIPTION_URL" == *"/sub/"* ]] || return 0
  SUB_TOKEN="${SUBSCRIPTION_URL##*/sub/}"
  SUB_TOKEN="${SUB_TOKEN%%\?*}"
  export SUB_TOKEN
}

prepare_compat_subscription_if_needed() {
  local protocols

  bool_enabled "${REAL_SUBSCRIPTION_CLIENT_COMPAT_AUTO_PREPARE:-1}" || return 1
  extract_sub_token_from_url_if_needed
  [[ -n "${DATABASE_URL:-}" && -n "${SUB_TOKEN:-}" ]] || return 1
  [[ -f scripts/real-access-inbound-matrix-db-prepare.sh ]] || return 1
  protocols="${REAL_SUBSCRIPTION_CLIENT_COMPAT_PREPARE_PROTOCOLS:-trojan}"
  [[ -n "$compat_prepare_manifest" ]] || compat_prepare_manifest="$tmp_dir/compat-db-prepare.env"
  bash scripts/real-access-inbound-matrix-db-prepare.sh prepare "$protocols" "$compat_prepare_manifest" >/dev/null \
    || return 1
}

prepare_compat_subscription_for_traffic_if_needed() {
  local previous_tmp_dir="${TMP_DIR:-}"
  local previous_inventory="${INVENTORY:-}"
  local previous_access_target="${ACCESS_TARGET:-}"
  local previous_temp_prep_manifest="${TEMP_PREP_MANIFEST:-}"

  bool_enabled "${RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC:-1}" || return 0
  prepare_compat_subscription_if_needed || return 0
  [[ -n "${XRAYC_REAL_E2E_INVENTORY:-}" && -f "${XRAYC_REAL_E2E_INVENTORY:-}" ]] || return 0
  TMP_DIR="$tmp_dir"
  INVENTORY="$XRAYC_REAL_E2E_INVENTORY"
  TEMP_PREP_MANIFEST="$compat_prepare_manifest"
  ACCESS_TARGET="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET:-}"
  open_prepared_access_ports
  wait_prepared_access_ports_listening
  wait_prepared_access_udp_ports_listening
  TMP_DIR="$previous_tmp_dir"
  INVENTORY="$previous_inventory"
  ACCESS_TARGET="$previous_access_target"
  TEMP_PREP_MANIFEST="$previous_temp_prep_manifest"
}

prepare_compat_subscription_for_traffic_if_needed
subscription_download_url="$(subscription_url)"
if ! fetch_subscription_profile "$subscription_download_url" "$profile"; then
  if [[ "$FETCH_STATUS" == "422" ]] && prepare_compat_subscription_if_needed; then
    fetch_subscription_profile "$subscription_download_url" "$profile" || {
      echo "subscription client compatibility download failed after temporary prepare; status=${FETCH_STATUS:-000}; curl_exit=${FETCH_CURL_EXIT}; raw curl output redacted" >&2
      exit 1
    }
  else
    echo "subscription client compatibility download failed; status=${FETCH_STATUS:-000}; curl_exit=${FETCH_CURL_EXIT}; raw curl output redacted" >&2
    exit 1
  fi
fi

validate_subscription_yaml "$profile"
run_mihomo_or_clash_check "$profile"
run_subscription_client_traffic_checks
