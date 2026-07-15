#!/usr/bin/env bash
# 用途：真实发布门禁前校验 E2E 用户和管理员账号可登录。
# 范围：只调用控制面登录接口和用户订阅接口，不创建或修改账号。
# 输入：读取 .env.real-release 或 XRAYC_REAL_RELEASE_ENV_FILE。
# 输出：只打印阶段状态，不打印账号、密码、Token、Cookie 或响应体。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

BASE_URL="${BASE_URL:-}"
USER_ACCOUNT="${E2E_USER_ACCOUNT:-${SMOKE_LOGIN_ACCOUNT:-}}"
USER_PASSWORD="${E2E_USER_PASSWORD:-${SMOKE_LOGIN_PASSWORD:-}}"
ADMIN_ACCOUNT="${E2E_ADMIN_ACCOUNT:-${ADMIN_LOGIN_ACCOUNT:-}}"
ADMIN_PASSWORD="${E2E_ADMIN_PASSWORD:-${ADMIN_LOGIN_PASSWORD:-}}"
CURL_TIMEOUT="${CURL_TIMEOUT:-15}"

[[ -n "$BASE_URL" ]] || { echo "real-e2e-auth-accounts: BASE_URL is required" >&2; exit 2; }
[[ -n "$USER_ACCOUNT" && -n "$USER_PASSWORD" ]] || { echo "real-e2e-auth-accounts: user credentials are required" >&2; exit 2; }
[[ -n "$ADMIN_ACCOUNT" && -n "$ADMIN_PASSWORD" ]] || { echo "real-e2e-auth-accounts: admin credentials are required" >&2; exit 2; }

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

login() {
  local label="$1"
  local account="$2"
  local password="$3"
  local body_file="$tmp_dir/${label}.json"
  local status_file="$tmp_dir/${label}.status"
  local payload_file="$tmp_dir/${label}-payload.json"

  ACCOUNT_VALUE="$account" PASSWORD_VALUE="$password" python3 - >"$payload_file" <<'PY'
import json
import os

print(json.dumps({
    "account": os.environ["ACCOUNT_VALUE"],
    "password": os.environ["PASSWORD_VALUE"],
}, ensure_ascii=False))
PY
  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --header "Content-Type: application/json" \
    --data-binary "@$payload_file" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}/api/auth/login" >"$status_file" 2>/dev/null; then
    echo "real-e2e-auth-accounts: ${label} login request failed" >&2
    return 1
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *)
      echo "real-e2e-auth-accounts: ${label} login rejected" >&2
      return 1
      ;;
  esac
  python3 - "$body_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
token = data.get("access_token") or data.get("accessToken") or data.get("token")
if not isinstance(token, str) or not token.strip():
    raise SystemExit("login response token missing")
PY
}

login "user" "$USER_ACCOUNT" "$USER_PASSWORD"
login "admin" "$ADMIN_ACCOUNT" "$ADMIN_PASSWORD"

echo "real-e2e-auth-accounts: passed"
