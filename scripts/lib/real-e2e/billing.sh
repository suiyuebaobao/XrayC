#!/usr/bin/env bash
# 用途：提供 real e2e 用户计费读数、基线捕获和轮询增长检查函数。
# 该文件只定义 billing helper，敏感响应和 token 信息保持不输出。
set -euo pipefail

xrayc_real_e2e_read_billed_bytes() {
  local base_url="$1"
  local user_access_token="$2"
  local tmp_dir="$3"
  local body_file="${tmp_dir}/user-usage.body"
  local status_file="${tmp_dir}/user-usage.status"
  local status="" config_file="${tmp_dir}/user-usage.curl.conf"

  BASE_URL_VALUE="${base_url%/}/api/user/usage" USER_ACCESS_TOKEN_VALUE="$user_access_token" python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
token = os.environ["USER_ACCESS_TOKEN_VALUE"].strip()
if "\n" in token or "\r" in token:
    raise SystemExit("authorization token must not contain newlines")
if token.lower().startswith("authorization:"):
    raise SystemExit("authorization token must be a raw token or Bearer token, not a full header")
if token.lower().startswith("bearer "):
    token = token.split(None, 1)[1].strip()
if not token:
    raise SystemExit("authorization token must not be empty")
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["BASE_URL_VALUE"])}"\n')
    fh.write(f'header = "Authorization: Bearer {quote(token)}"\n')
PY
  chmod 600 "$config_file"

  if ! curl \
    --silent \
    --location \
    --max-time "${CURL_TIMEOUT:-15}" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    --config "$config_file" >"$status_file" 2>/dev/null; then
    echo "user usage request failed; raw curl output redacted" >&2
    exit 1
  fi

  status="$(cat "$status_file")"
  if [[ "$status" != 2* ]]; then
    echo "user usage request returned non-success status; response body redacted" >&2
    exit 1
  fi

  python3 - "$body_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)

data = payload.get("data", payload)
value = data.get("billed_bytes", data.get("billedBytes"))
if value is None:
    raise SystemExit("user usage response missing billed bytes")
print(int(value))
PY
}

xrayc_real_e2e_capture_billing_baseline() {
  local base_url="$1"
  local user_access_token="${2:-}"
  local tmp_dir="$3"

  if [[ -n "$user_access_token" ]]; then
    xrayc_real_e2e_read_billed_bytes "$base_url" "$user_access_token" "$tmp_dir"
    return
  fi

  if xrayc_real_e2e_bool_is_true "${REQUIRE_BILLING_CHECK:-false}"; then
    echo "USER_ACCESS_TOKEN is required when REQUIRE_BILLING_CHECK=true" >&2
    exit 2
  fi

  printf '\n'
}

xrayc_real_e2e_wait_billing_increase() {
  local base_url="$1"
  local user_access_token="${2:-}"
  local before="${3:-}"
  local tmp_dir="$4"
  local timeout_seconds="${BILLING_POLL_SECONDS:-90}"
  local interval_seconds="${BILLING_POLL_INTERVAL_SECONDS:-5}"
  local elapsed=0
  local current=""

  if [[ -z "$before" ]]; then
    echo "Skipping billing usage check: USER_ACCESS_TOKEN not provided."
    return
  fi

  echo "Waiting for billed traffic increase"
  while [[ "$elapsed" -le "$timeout_seconds" ]]; do
    current="$(xrayc_real_e2e_read_billed_bytes "$base_url" "$user_access_token" "$tmp_dir")"
    if [[ "$current" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ && "$current" -gt "$before" ]]; then
      echo "billing usage increased"
      return
    fi
    sleep "$interval_seconds"
    elapsed=$((elapsed + interval_seconds))
  done

  echo "billing usage did not increase before timeout; usage values redacted" >&2
  exit 1
}
