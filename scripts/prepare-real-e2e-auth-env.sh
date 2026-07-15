#!/usr/bin/env bash
# 用途：为真实 E2E 认证场景准备或校验私有认证环境变量。
# 范围：负责账号、令牌、订阅和访问链路前置条件，不运行完整 E2E。
# 输入：读取真实发布 env、登录账号密码、订阅和控制面 URL 等变量。
# 输出：写出或提示可供后续真实 E2E 使用的脱敏环境状态。
# 依赖：调用控制面 API、curl 和 real-e2e-lib.sh 的安全校验工具。
# 安全：不得打印密码、token、Cookie、订阅链接或完整 API 响应。
# 约束：只处理私有测试环境，生产环境必须显式防误用。
# 行为：按现有环境补齐认证前置条件，并校验基础访问可用性。
# 失败：缺少必需配置、认证失败或响应不符合预期时返回非零。
# 维护：认证流程字段变化时同步 usage、API 调用和脱敏输出。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
USER_LOGIN_ACCOUNT="${USER_LOGIN_ACCOUNT:-${SMOKE_LOGIN_ACCOUNT:-demo@example.test}}"
USER_LOGIN_PASSWORD="${USER_LOGIN_PASSWORD:-${SMOKE_LOGIN_PASSWORD:-demo123456}}"
ADMIN_LOGIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-admin}"
ADMIN_LOGIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-}"
RESET_SUBSCRIPTION_TOKEN="${RESET_SUBSCRIPTION_TOKEN:-0}"
CURL_TIMEOUT="${CURL_TIMEOUT:-15}"

# 认证材料是私有值，生成和更新过程默认只允许当前用户读取。
umask 077

usage() {
  cat <<'EOF'
usage: bash scripts/prepare-real-e2e-auth-env.sh

Login with private test accounts and write real E2E auth variables into the
private real-release env file. The script never prints access tokens,
subscription tokens, cookies, URLs, passwords, or raw response bodies.

Environment:
  XRAYC_REAL_RELEASE_ENV_FILE   target env file, default .env.real-release
  BASE_URL                      control plane base URL
  USER_LOGIN_ACCOUNT            user login account, defaults to demo account
  USER_LOGIN_PASSWORD           user login password, defaults to demo password
  ADMIN_LOGIN_ACCOUNT           admin login account, default admin
  ADMIN_LOGIN_PASSWORD          admin login password, optional
  RESET_SUBSCRIPTION_TOKEN=1    reset user subscription token before writing it
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

if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
  BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

json_eval() {
  local expression="$1"
  local file="$2"
  python3 - "$expression" "$file" <<'PY'
import json
import sys

expression = sys.argv[1]
path = sys.argv[2]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload) if isinstance(payload, dict) else {}

if expression == "access_token":
    keys = ("access_token", "accessToken", "token")
elif expression == "subscription_token":
    keys = ("token", "subscription_token", "subscriptionToken")
elif expression == "subscription_url":
    keys = ("subscription_url", "subscriptionUrl", "download_url", "downloadUrl", "clash_url", "clashUrl", "url")
else:
    raise SystemExit(2)

for key in keys:
    value = data.get(key) if isinstance(data, dict) else None
    if isinstance(value, str) and value.strip():
        print(value.strip())
        raise SystemExit(0)
raise SystemExit(1)
PY
}

upsert_env_value() {
  local name="$1"
  local value="${2:-}"
  [[ -n "$value" ]] || return 0
  python3 - "$REAL_RELEASE_ENV_FILE" "$name" "$value" <<'PY'
from pathlib import Path
import os
import re
import shlex
import sys

path = Path(sys.argv[1])
name = sys.argv[2]
value = sys.argv[3]
line = f"{name}={shlex.quote(value)}\n"
pattern = re.compile(rf"^{re.escape(name)}=")

lines = []
replaced = False
if path.exists():
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
for index, existing in enumerate(lines):
    if pattern.match(existing):
        lines[index] = line
        replaced = True
        break
if not replaced:
    if lines and not lines[-1].endswith("\n"):
        lines[-1] += "\n"
    lines.append(line)
path.write_text("".join(lines), encoding="utf-8")
os.chmod(path, 0o600)
PY
  echo "prepare-real-e2e-auth-env: wrote ${name}"
}

post_login() {
  local label="$1"
  local account="$2"
  local password="$3"
  local body_file="$4"
  local status_file="$5"
  local account_file="$tmp_dir/${label}-account.txt"
  local password_file="$tmp_dir/${label}-password.txt"
  local payload_file="$tmp_dir/${label}-login-payload.json"

  printf '%s' "$account" > "$account_file"
  printf '%s' "$password" > "$password_file"
  python3 - "$account_file" "$password_file" > "$payload_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    account = fh.read()
with open(sys.argv[2], "r", encoding="utf-8") as fh:
    password = fh.read()
print(json.dumps({"account": account, "password": password}, ensure_ascii=False))
PY
  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --header "Content-Type: application/json" \
    --data-binary "@$payload_file" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}/api/auth/login" > "$status_file" 2>/dev/null; then
    echo "prepare-real-e2e-auth-env: ${label} login request failed" >&2
    return 1
  fi
  case "$(cat "$status_file")" in
    2*) return 0 ;;
    *)
      echo "prepare-real-e2e-auth-env: ${label} login was rejected" >&2
      return 1
      ;;
  esac
}

fetch_user_subscription() {
  local token="$1"
  local method="$2"
  local path="$3"
  local body_file="$4"
  local status_file="$5"
  local auth_config="$tmp_dir/subscription-auth.conf"
  local escaped="${token//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$auth_config"
  chmod 600 "$auth_config"
  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --request "$method" \
    --config "$auth_config" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" > "$status_file" 2>/dev/null; then
    echo "prepare-real-e2e-auth-env: subscription request failed" >&2
    return 1
  fi
  case "$(cat "$status_file")" in
    2*) return 0 ;;
    *)
      echo "prepare-real-e2e-auth-env: subscription request was rejected" >&2
      return 1
      ;;
  esac
}

user_login_body="$tmp_dir/user-login.json"
user_login_status="$tmp_dir/user-login.status"
post_login "user" "$USER_LOGIN_ACCOUNT" "$USER_LOGIN_PASSWORD" "$user_login_body" "$user_login_status"
user_access_token="$(json_eval access_token "$user_login_body")"
upsert_env_value USER_ACCESS_TOKEN "$user_access_token"

subscription_body="$tmp_dir/subscription.json"
subscription_status="$tmp_dir/subscription.status"
if [[ "$RESET_SUBSCRIPTION_TOKEN" == "1" ]]; then
  fetch_user_subscription "$user_access_token" "POST" "/api/user/subscription/token/reset" "$subscription_body" "$subscription_status"
else
  fetch_user_subscription "$user_access_token" "GET" "/api/user/subscription" "$subscription_body" "$subscription_status"
fi

subscription_token="$(json_eval subscription_token "$subscription_body" || true)"
subscription_url="$(json_eval subscription_url "$subscription_body" || true)"
if [[ -z "$subscription_token" && "$subscription_url" == *"/sub/"* ]]; then
  subscription_token="${subscription_url##*/sub/}"
  subscription_token="${subscription_token%%[?#]*}"
fi
if [[ -n "$subscription_token" ]]; then
  upsert_env_value SUB_TOKEN "$subscription_token"
else
  upsert_env_value SUBSCRIPTION_URL "$subscription_url"
fi

if [[ -n "$ADMIN_LOGIN_PASSWORD" ]]; then
  admin_login_body="$tmp_dir/admin-login.json"
  admin_login_status="$tmp_dir/admin-login.status"
  post_login "admin" "$ADMIN_LOGIN_ACCOUNT" "$ADMIN_LOGIN_PASSWORD" "$admin_login_body" "$admin_login_status"
  admin_access_token="$(json_eval access_token "$admin_login_body")"
  upsert_env_value ADMIN_ACCESS_TOKEN "$admin_access_token"
else
  echo "prepare-real-e2e-auth-env: skipped ADMIN_ACCESS_TOKEN because ADMIN_LOGIN_PASSWORD is not set"
fi

echo "prepare-real-e2e-auth-env: completed without printing private values"
