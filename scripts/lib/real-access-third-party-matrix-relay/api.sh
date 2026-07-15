#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的控制平面 API 调用函数。
# 负责 JSON 请求、鉴权 GET 和登录 token 提取。
# 所有失败信息只输出状态摘要，避免把响应体中的私有材料写入日志。
# 本文件依赖 common.sh 中的 die、json_value 和 write_json。

api_json() {
  local method="$1"
  local path="$2"
  local payload="$3"
  local output="$4"
  local token="$5"
  local config_file="$TMP_DIR/curl-api-${RANDOM}.conf"
  local status=""
  API_URL="${BASE_URL%/}${path}" API_TOKEN="$token" API_METHOD="$method" API_PAYLOAD="$payload" \
    python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'request = "{quote(os.environ["API_METHOD"])}"\n')
    fh.write('header = "Content-Type: application/json"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
    fh.write(f'data-binary = "@{quote(os.environ["API_PAYLOAD"])}"\n')
PY
  chmod 600 "$config_file"
  status="$(curl -sS --location --max-time 180 -o "$output" -w '%{http_code}' --config "$config_file" 2>/dev/null || true)"
  case "$status" in
    2*) ;;
    *) die "api request failed path=${path} status=${status}; response redacted" ;;
  esac
}

api_auth_get() {
  local path="$1"
  local output="$2"
  local token="$3"
  local config_file="$TMP_DIR/curl-get-${RANDOM}.conf"
  local status=""
  API_URL="${BASE_URL%/}${path}" API_TOKEN="$token" python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["API_URL"])}"\n')
    fh.write(f'header = "Authorization: Bearer {quote(os.environ["API_TOKEN"])}"\n')
PY
  chmod 600 "$config_file"
  status="$(curl -sS --location --max-time 30 -o "$output" -w '%{http_code}' --config "$config_file" 2>/dev/null || true)"
  case "$status" in
    2*) ;;
    *) die "api get failed with status ${status}; response redacted" ;;
  esac
}

login_token() {
  local account="$1"
  local password="$2"
  local label="$3"
  local payload="$TMP_DIR/${label}-login-payload.json"
  local body="$TMP_DIR/${label}-login-body.json"
  local status=""
  ACCOUNT_VALUE="$account" PASSWORD_VALUE="$password" \
    write_json "$payload" 'print(json.dumps({"account": os.environ["ACCOUNT_VALUE"], "password": os.environ["PASSWORD_VALUE"]}, ensure_ascii=False))'
  status="$(curl -sS --max-time 30 -o "$body" -w '%{http_code}' \
    -H "Content-Type: application/json" \
    --data-binary "@$payload" "${BASE_URL%/}/api/auth/login" 2>/dev/null || true)"
  case "$status" in
    2*) ;;
    *) die "${label} login failed; response redacted" ;;
  esac
  json_value "$body" access_token || json_value "$body" accessToken || json_value "$body" token
}
