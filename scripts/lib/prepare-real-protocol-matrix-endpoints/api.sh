#!/usr/bin/env bash
# 用途：封装真实协议矩阵 endpoint 准备脚本使用的控制面 API 调用。
# 范围：仅供主脚本 source，依赖 common.sh 中的 die/json/value 校验函数。
# 安全：curl 配置、stderr 和响应正文都写入临时目录，不在终端输出私密内容。
# 约束：这里不解析参数、不写 env，只负责登录和通用 GET/POST。

api_json() {
  local method="$1"
  local path="$2"
  local payload="$3"
  local output="$4"
  local token="$5"
  local config_file="${TMP_DIR}/curl-api-${RANDOM}.conf"
  local err_file="${TMP_DIR}/curl-api-${RANDOM}.err"
  local status_code=""

  API_URL="${BASE_URL%/}${path}" API_TOKEN="$token" API_METHOD="$method" API_PAYLOAD="$payload" \
    python3 - "$config_file" <<'PY'
import os
import sys

def quote(value):
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'request = "{quote(os.environ["API_METHOD"])}"\n')
    fh.write('header = "Content-Type: application/json"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
    fh.write(f'data-binary = "@{quote(os.environ["API_PAYLOAD"])}"\n')
PY
  chmod 600 "$config_file"
  status_code="$(curl --silent --show-error --location --max-time "$CURL_TIMEOUT" \
    --output "$output" --write-out "%{http_code}" --config "$config_file" 2>"$err_file" || true)"
  case "$status_code" in
    2*) ;;
    *) die "control plane API request failed with status ${status_code}; response redacted" ;;
  esac
}

api_get() {
  local path="$1"
  local output="$2"
  local token="$3"
  local config_file="${TMP_DIR}/curl-get-${RANDOM}.conf"
  local err_file="${TMP_DIR}/curl-get-${RANDOM}.err"
  local status_code=""

  API_URL="${BASE_URL%/}${path}" API_TOKEN="$token" python3 - "$config_file" <<'PY'
import os
import sys

def quote(value):
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
PY
  chmod 600 "$config_file"
  status_code="$(curl --silent --show-error --location --max-time "$CURL_TIMEOUT" \
    --output "$output" --write-out "%{http_code}" --config "$config_file" 2>"$err_file" || true)"
  case "$status_code" in
    2*) ;;
    *) die "control plane API request failed with status ${status_code}; response redacted" ;;
  esac
}

post_admin_login() {
  local output="$1"
  local status_file="${TMP_DIR}/admin-login.status"
  local payload_file="${TMP_DIR}/admin-login.json"
  local err_file="${TMP_DIR}/admin-login.err"
  local account_file="${TMP_DIR}/admin-login-account.txt"
  local password_file="${TMP_DIR}/admin-login-password.txt"

  printf '%s' "$ADMIN_LOGIN_ACCOUNT_VALUE" > "$account_file"
  printf '%s' "$ADMIN_LOGIN_PASSWORD_VALUE" > "$password_file"
  python3 - "$account_file" "$password_file" > "$payload_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    account = fh.read()
with open(sys.argv[2], "r", encoding="utf-8") as fh:
    password = fh.read()
print(json.dumps({"account": account, "password": password}, ensure_ascii=False))
PY

  if ! curl --silent --show-error --location --max-time "$CURL_TIMEOUT" \
    --header "Content-Type: application/json" \
    --data-binary "@$payload_file" \
    --output "$output" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}/api/auth/login" > "$status_file" 2>"$err_file"; then
    die "admin login request failed; stderr redacted"
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *) die "admin login was rejected; response redacted" ;;
  esac
}

resolve_admin_token() {
  ADMIN_LOGIN_ACCOUNT_VALUE="${ADMIN_LOGIN_ACCOUNT:-${SMOKE_ADMIN_ACCOUNT:-${CONTRACT_ADMIN_ACCOUNT:-${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-admin}}}}"
  ADMIN_LOGIN_PASSWORD_VALUE="${ADMIN_LOGIN_PASSWORD:-${SMOKE_ADMIN_PASSWORD:-${CONTRACT_ADMIN_PASSWORD:-${XRAYC_REAL_E2E_ADMIN_PASSWORD:-}}}}"
  if value_ready "$ADMIN_LOGIN_PASSWORD_VALUE"; then
    local body_file="${TMP_DIR}/admin-login-body.json"
    local token=""
    post_admin_login "$body_file"
    token="$(json_value "$body_file" access_token || json_value "$body_file" accessToken || json_value "$body_file" token)" \
      || die "admin login response did not contain an access token"
    printf '%s' "$token"
    return 0
  fi

  if value_ready "${ADMIN_ACCESS_TOKEN:-}"; then
    printf '%s' "$ADMIN_ACCESS_TOKEN"
    return 0
  fi

  die "ADMIN_ACCESS_TOKEN or ADMIN_LOGIN_PASSWORD is required"
}
