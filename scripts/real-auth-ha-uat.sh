#!/usr/bin/env bash
# 该脚本执行真实认证高可用 UAT，验证多副本下认证安全行为。
# 主文件只负责参数说明、环境加载、全局状态初始化和流程编排。
# Compose、HTTP、payload、数据库清理和认证流程实现拆到 helper 中。
# 输出必须避免泄露密码、令牌、Cookie、响应体、URL、IP 和数据库连接串。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=lib/real-auth-ha-uat/runtime.sh
source "${SCRIPT_DIR}/lib/real-auth-ha-uat/runtime.sh"
# shellcheck source=lib/real-auth-ha-uat/auth-flow.sh
source "${SCRIPT_DIR}/lib/real-auth-ha-uat/auth-flow.sh"

usage() {
  cat <<'USAGE'
usage: bash scripts/real-auth-ha-uat.sh

Runs real multi-replica auth-security UAT without printing passwords, tokens,
cookies, response bodies, URLs, IPs, or database connection strings.

The script loads .env.real-release or XRAYC_REAL_RELEASE_ENV_FILE when present.

Required:
  ADMIN_ACCESS_TOKEN or ADMIN_LOGIN_ACCOUNT + ADMIN_LOGIN_PASSWORD.
  E2E_ADMIN_ACCOUNT/E2E_ADMIN_PASSWORD are used as fallback login credentials.
  BASE_URL, unless UAT_COMPOSE_START=1 is used.

Replica gate, choose one:
  UAT_COMPOSE_START=1
    Start an isolated Docker Compose project with --scale api=2, then test it.
    Optional: UAT_HTTP_PORT, UAT_COMPOSE_PROJECT_NAME, UAT_COMPOSE_NO_BUILD=1.
  default
    Verify the current Docker Compose project has at least two api containers.
    Optional: UAT_COMPOSE_FILES, UAT_COMPOSE_PROJECT_NAME.

Optional:
  DATABASE_URL                  Required for email verification one-time UAT and cleanup.
  UAT_PSQL_DATABASE_URL         Optional host-reachable PostgreSQL URL when BASE_URL is remote.
  UAT_TEST_EMAIL_DOMAIN         Default example.test.
  UAT_RATE_LIMIT_ATTEMPTS       Default 35.
  UAT_MIN_API_REPLICAS          Default 2.
  UAT_VALIDATE_ONLY=1           Validate required variables and exit.
  CURL_TIMEOUT                  Default 15.
USAGE
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
if [[ -n "${XRAYC_AUTH_HA_FORCE_COMPOSE_START:-}" ]]; then
  UAT_COMPOSE_START="$XRAYC_AUTH_HA_FORCE_COMPOSE_START"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_COMPOSE_NO_BUILD:-}" ]]; then
  UAT_COMPOSE_NO_BUILD="$XRAYC_AUTH_HA_FORCE_COMPOSE_NO_BUILD"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_MIN_API_REPLICAS:-}" ]]; then
  UAT_MIN_API_REPLICAS="$XRAYC_AUTH_HA_FORCE_MIN_API_REPLICAS"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY:-}" ]]; then
  UAT_VALIDATE_ONLY="$XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY"
fi

BASE_URL="${BASE_URL:-}"
ADMIN_ACCESS_TOKEN="${ADMIN_ACCESS_TOKEN:-}"
ADMIN_LOGIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}"
ADMIN_LOGIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}"
DATABASE_URL="${DATABASE_URL:-}"
UAT_PSQL_DATABASE_URL="${UAT_PSQL_DATABASE_URL:-}"
CURL_TIMEOUT="${CURL_TIMEOUT:-15}"
UAT_TEST_EMAIL_DOMAIN="${UAT_TEST_EMAIL_DOMAIN:-example.test}"
UAT_RATE_LIMIT_ATTEMPTS="${UAT_RATE_LIMIT_ATTEMPTS:-35}"
UAT_MIN_API_REPLICAS="${UAT_MIN_API_REPLICAS:-2}"
UAT_COMPOSE_START="${UAT_COMPOSE_START:-0}"
UAT_COMPOSE_NO_BUILD="${UAT_COMPOSE_NO_BUILD:-0}"
UAT_VALIDATE_ONLY="${UAT_VALIDATE_ONLY:-0}"

if [[ -z "$ADMIN_ACCESS_TOKEN" && ( -z "$ADMIN_LOGIN_ACCOUNT" || -z "$ADMIN_LOGIN_PASSWORD" ) ]]; then
  echo "ADMIN_ACCESS_TOKEN or ADMIN_LOGIN_ACCOUNT + ADMIN_LOGIN_PASSWORD is required" >&2
  exit 2
fi
if ! bool_is_true "$UAT_COMPOSE_START"; then
  if [[ -z "$BASE_URL" || ! "$BASE_URL" =~ ^https?:// ]]; then
    echo "BASE_URL must be an http(s) URL unless UAT_COMPOSE_START=1 is used" >&2
    exit 2
  fi
fi
if [[ ! "$CURL_TIMEOUT" =~ ^[1-9][0-9]*$ ]]; then
  echo "CURL_TIMEOUT must be a positive integer" >&2
  exit 2
fi
if [[ ! "$UAT_RATE_LIMIT_ATTEMPTS" =~ ^[1-9][0-9]*$ ]]; then
  echo "UAT_RATE_LIMIT_ATTEMPTS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$UAT_MIN_API_REPLICAS" =~ ^[1-9][0-9]*$ ]]; then
  echo "UAT_MIN_API_REPLICAS must be a positive integer" >&2
  exit 2
fi
if [[ "$UAT_MIN_API_REPLICAS" -lt 2 ]]; then
  echo "UAT_MIN_API_REPLICAS must be at least 2" >&2
  exit 2
fi
if [[ "$UAT_RATE_LIMIT_ATTEMPTS" -lt 31 ]]; then
  echo "UAT_RATE_LIMIT_ATTEMPTS must be at least 31" >&2
  exit 2
fi
if bool_is_true "$UAT_VALIDATE_ONLY"; then
  echo "auth HA UAT validation passed"
  exit 0
fi
if [[ -z "${UAT_PSQL_DATABASE_URL:-${DATABASE_URL:-}}" ]] && ! bool_is_true "$UAT_COMPOSE_START"; then
  echo "DATABASE_URL or UAT_PSQL_DATABASE_URL is required for auth HA UAT email verification coverage" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
run_id="$(date -u +%Y%m%d%H%M%S)-$$"
test_email="auth-ha-uat-${run_id}@${UAT_TEST_EMAIL_DOMAIN}"
test_password="AuthHaUat-${run_id}-Passw0rd!"
wrong_password="AuthHaUat-${run_id}-Wrong!"
original_security_file="$tmp_dir/original-auth-security.json"
test_security_file="$tmp_dir/test-auth-security.json"
restore_needed=0
compose_started=0
compose_override_file=""
uat_email_code="739251"
uat_email_code_id=""

cleanup() {
  local status=$?
  if [[ "$restore_needed" -eq 1 && -s "$original_security_file" ]]; then
    put_admin_json "restore auth security" "/api/admin/auth-security" "$original_security_file" \
      "$tmp_dir/restore.body" "$tmp_dir/restore.status" >/dev/null || true
  fi
  cleanup_test_data || true
  if [[ "$compose_started" -eq 1 ]]; then
    compose_cmd down --volumes --remove-orphans >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
  exit "$status"
}
trap cleanup EXIT

require_tool curl
require_tool python3
require_tool psql

if bool_is_true "$UAT_COMPOSE_START"; then
  start_compose_replicas
else
  verify_compose_replicas
fi

wait_for_health
derive_admin_token_if_needed
configure_auth_security 0
run_email_verification_uat
run_auth_uat
run_shared_rate_limit_uat

echo "real auth HA UAT passed"
