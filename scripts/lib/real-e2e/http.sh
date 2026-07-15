#!/usr/bin/env bash
# 用途：提供 real e2e 中 curl 抓取、敏感 URL 保护和出口 IP 检查函数。
# 该文件只定义网络请求 helper，实际请求由调用方函数触发。
set -euo pipefail

xrayc_real_e2e_fetch_url_to_file() {
  local url="$1"
  local output_file="$2"
  local failure_message="$3"

  if ! curl --fail --silent --location --max-time "${CURL_TIMEOUT:-30}" "$url" >"$output_file" 2>/dev/null; then
    echo "${failure_message}; raw curl output redacted" >&2
    exit 1
  fi
}

xrayc_real_e2e_write_curl_url_config() {
  local url="$1"
  local config_file="$2"

  URL_VALUE="$url" python3 - "$config_file" <<'PY'
import os
import sys

config_file = sys.argv[1]
url = os.environ["URL_VALUE"]
escaped = url.replace("\\", "\\\\").replace('"', '\\"')
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{escaped}"\n')
PY
  chmod 600 "$config_file"
}

xrayc_real_e2e_fetch_sensitive_url_to_file() {
  local url="$1"
  local output_file="$2"
  local failure_message="$3"
  local tmp_dir config_file status curl_exit

  tmp_dir="$(mktemp -d)"
  config_file="$tmp_dir/curl-url.conf"
  xrayc_real_e2e_write_curl_url_config "$url" "$config_file"
  set +e
  status="$(curl --fail --silent --location --max-time "${CURL_TIMEOUT:-30}" \
    --output "$output_file" --write-out '%{http_code}' --config "$config_file" 2>/dev/null)"
  curl_exit=$?
  set -e
  if [[ "$curl_exit" -ne 0 || "$status" != 2* ]]; then
    rm -rf "$tmp_dir"
    echo "${failure_message}; status=${status:-000}; curl_exit=${curl_exit}; raw curl output redacted" >&2
    exit 1
  fi
  rm -rf "$tmp_dir"
}

xrayc_real_e2e_fetch_url_via_proxy_to_file() {
  local proxy_url="$1"
  local url="$2"
  local output_file="$3"
  local failure_message="$4"
  local tmp_dir config_file

  tmp_dir="$(mktemp -d)"
  config_file="$tmp_dir/curl-proxy-url.conf"
  URL_VALUE="$url" PROXY_VALUE="$proxy_url" python3 - "$config_file" <<'PY'
import os
import sys

def quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')

config_file = sys.argv[1]
with open(config_file, "w", encoding="utf-8") as fh:
    fh.write(f'url = "{quote(os.environ["URL_VALUE"])}"\n')
    fh.write(f'proxy = "{quote(os.environ["PROXY_VALUE"])}"\n')
PY
  chmod 600 "$config_file"
  if ! curl --fail --silent --location --max-time "${CURL_TIMEOUT:-30}" --config "$config_file" >"$output_file" 2>/dev/null; then
    rm -rf "$tmp_dir"
    echo "${failure_message}; raw curl output redacted" >&2
    exit 1
  fi
  rm -rf "$tmp_dir"
}

xrayc_real_e2e_assert_expected_egress_ip() {
  local client_proxy_url="$1"
  local public_ip_url="$2"
  local expected_exit_ip="$3"
  local tmp_dir="$4"
  local actual_ip_file="${tmp_dir}/public-ip.txt"
  local actual_ip=""

  xrayc_real_e2e_fetch_url_via_proxy_to_file "$client_proxy_url" "$public_ip_url" "$actual_ip_file" "public IP probe failed"
  actual_ip="$(tr -d '[:space:]' < "$actual_ip_file")"
  if [[ "$actual_ip" != "$expected_exit_ip" ]]; then
    echo "unexpected egress ip: actual value differs from expected value" >&2
    exit 1
  fi
}
