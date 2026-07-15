#!/usr/bin/env bash
# 用途：回归测试真实协议矩阵 endpoint 准备脚本的管理员认证优先级。
# 约束：不访问网络、不打印 token 正文；只验证旧 ADMIN_ACCESS_TOKEN 不会覆盖可用登录凭据。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
LIB_DIR="${BASE_DIR}/scripts/lib/prepare-real-protocol-matrix-endpoints"
cd "$BASE_DIR"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/common.sh
. "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/api.sh
. "${LIB_DIR}/api.sh"

login_calls_file="${TMP_DIR}/login-calls"
printf '0\n' > "$login_calls_file"
post_admin_login() {
  local output="$1"
  local calls
  calls="$(cat "$login_calls_file")"
  printf '%s\n' "$((calls + 1))" > "$login_calls_file"
  printf '{"data":{"access_token":"fresh-login-token"}}\n' > "$output"
}

ADMIN_ACCESS_TOKEN="stale-env-token"
ADMIN_LOGIN_ACCOUNT="admin"
ADMIN_LOGIN_PASSWORD="valid-admin-password"

resolved="$(resolve_admin_token)"
if [[ "$resolved" != "fresh-login-token" ]]; then
  echo "expected login-derived admin token when login password is available" >&2
  exit 1
fi
if [[ "$(cat "$login_calls_file")" != "1" ]]; then
  echo "expected admin login to be called exactly once" >&2
  exit 1
fi

(
  # shellcheck source=scripts/lib/ops-mistake-recovery-uat/common.sh
  . "${BASE_DIR}/scripts/lib/ops-mistake-recovery-uat/common.sh"
  tmp_dir="${TMP_DIR}/ops"
  mkdir -p "$tmp_dir"
  ADMIN_ACCESS_TOKEN="stale-ops-token"
  ADMIN_LOGIN_ACCOUNT="admin"
  ADMIN_LOGIN_PASSWORD="valid-admin-password"
  ops_calls_file="${TMP_DIR}/ops-login-calls"
  printf '0\n' > "$ops_calls_file"
  api_json() {
    local _method="$1"
    local _path="$2"
    local _payload_file="$3"
    local body_file="$4"
    local status_file="$5"
    local calls
    calls="$(cat "$ops_calls_file")"
    printf '%s\n' "$((calls + 1))" > "$ops_calls_file"
    printf '{"data":{"access_token":"fresh-ops-token"}}\n' > "$body_file"
    printf '200' > "$status_file"
  }
  derive_admin_token_if_needed
  if [[ "$ADMIN_ACCESS_TOKEN" != "fresh-ops-token" ]]; then
    echo "expected ops UAT to refresh stale admin token when login password is available" >&2
    exit 1
  fi
  if [[ "$(cat "$ops_calls_file")" != "1" ]]; then
    echo "expected ops UAT admin login to be called exactly once" >&2
    exit 1
  fi
)

echo "real-protocol-matrix-endpoint-auth: passed"
