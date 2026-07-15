#!/usr/bin/env bash
# Purpose: offline regression checks for real protocol matrix endpoint payloads.
# It uses fake provider values only and must not read private real-release env files.
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
RUN_ID="payload-test"
trap 'rm -rf "$TMP_DIR"' EXIT

# shellcheck source=scripts/lib/prepare-real-protocol-matrix-endpoints/payload.sh
. scripts/lib/prepare-real-protocol-matrix-endpoints/payload.sh

fail() {
  printf 'real-protocol-matrix-endpoint-payload: %s\n' "$1" >&2
  exit 1
}

reset_matrix_env() {
  local name
  while IFS='=' read -r name _; do
    case "$name" in
      THIRD_PARTY_*|REAL_PROTOCOL_MATRIX_*|XRAYC_REAL_PROTOCOL_MATRIX_*)
        unset "$name"
        ;;
    esac
  done < <(env)
}

payload_for() {
  local protocol="$1"
  local output="$TMP_DIR/${protocol}.json"
  build_endpoint_payload "$protocol" "$output"
  printf '%s' "$output"
}

assert_json_value() {
  local file="$1"
  local expression="$2"
  local expected="$3"
  python3 - "$file" "$expression" "$expected" <<'PY' || fail "${expression} did not match expected fake value"
import json
import sys

path, expression, expected = sys.argv[1:4]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
cursor = payload
for part in expression.split("."):
    if not isinstance(cursor, dict) or part not in cursor:
        raise SystemExit(1)
    cursor = cursor[part]
if str(cursor) != expected:
    raise SystemExit(1)
PY
}

assert_json_absent() {
  local file="$1"
  local expression="$2"
  python3 - "$file" "$expression" <<'PY' || fail "${expression} should be absent"
import json
import sys

path, expression = sys.argv[1:3]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
cursor = payload
for part in expression.split("."):
    if not isinstance(cursor, dict) or part not in cursor:
        raise SystemExit(0)
    cursor = cursor[part]
raise SystemExit(1)
PY
}

reset_matrix_env
export THIRD_PARTY_SOCKS_RAW_URL="socks5h://raw-socks.example.test:1080"
export THIRD_PARTY_SOCKS_HOST="stale-socks.example.test"
export THIRD_PARTY_SOCKS_PORT="2080"
export THIRD_PARTY_SOCKS_USERNAME="stale-user"
export THIRD_PARTY_SOCKS_PASSWORD="stale-pass"
socks_payload="$(payload_for socks)"
assert_json_value "$socks_payload" "host" "raw-socks.example.test"
assert_json_value "$socks_payload" "port" "1080"
assert_json_absent "$socks_payload" "outbound_config.username"
assert_json_absent "$socks_payload" "outbound_config.password"

reset_matrix_env
export THIRD_PARTY_HTTP_RAW_URL="http://raw-http.example.test:8080"
export THIRD_PARTY_HTTP_HOST="stale-http.example.test"
export THIRD_PARTY_HTTP_PORT="28080"
export THIRD_PARTY_HTTP_USERNAME="stale-user"
export THIRD_PARTY_HTTP_KEY="stale-key"
http_payload="$(payload_for http)"
assert_json_value "$http_payload" "host" "raw-http.example.test"
assert_json_value "$http_payload" "port" "8080"
assert_json_absent "$http_payload" "outbound_config.username"
assert_json_absent "$http_payload" "outbound_config.password"

reset_matrix_env
export THIRD_PARTY_VLESS_RAW_URL="vless://11111111-1111-1111-1111-111111111111@raw-vless.example.test:443?security=reality&sni=raw-vless-sni.example.test&pbk=raw-public-key&sid=rawsid&fp=chrome"
export THIRD_PARTY_VLESS_HOST="stale-vless.example.test"
export THIRD_PARTY_VLESS_PORT="2443"
export THIRD_PARTY_VLESS_UUID="22222222-2222-2222-2222-222222222222"
export THIRD_PARTY_VLESS_SECURITY="tls"
export THIRD_PARTY_VLESS_SERVER_NAME="stale-vless-sni.example.test"
export THIRD_PARTY_VLESS_PUBLIC_KEY="stale-public-key"
export THIRD_PARTY_VLESS_SHORT_ID="stalesid"
export THIRD_PARTY_VLESS_FINGERPRINT="firefox"
vless_payload="$(payload_for vless)"
assert_json_value "$vless_payload" "host" "raw-vless.example.test"
assert_json_value "$vless_payload" "port" "443"
assert_json_value "$vless_payload" "outbound_config.uuid" "11111111-1111-1111-1111-111111111111"
assert_json_value "$vless_payload" "outbound_config.security" "reality"
assert_json_value "$vless_payload" "outbound_config.server_name" "raw-vless-sni.example.test"
assert_json_value "$vless_payload" "outbound_config.public_key" "raw-public-key"
assert_json_value "$vless_payload" "outbound_config.short_id" "rawsid"
assert_json_value "$vless_payload" "outbound_config.fingerprint" "chrome"

reset_matrix_env
export THIRD_PARTY_VLESS_RAW_URL="vless://11111111-1111-1111-1111-111111111111@raw-vless.example.test:443?security=reality&sni=raw-vless-sni.example.test"
export THIRD_PARTY_VLESS_PUBLIC_KEY="split-public-key"
vless_split_key_payload="$(payload_for vless)"
assert_json_value "$vless_split_key_payload" "outbound_config.public_key" "split-public-key"

reset_matrix_env
export THIRD_PARTY_TROJAN_RAW_URL="trojan://raw-pass@raw-trojan.example.test:443?sni=raw-trojan-sni.example.test&security=tls"
export THIRD_PARTY_TROJAN_HOST="stale-trojan.example.test"
export THIRD_PARTY_TROJAN_PORT="2443"
export THIRD_PARTY_TROJAN_PASSWORD="stale-pass"
export THIRD_PARTY_TROJAN_SNI="stale-trojan-sni.example.test"
trojan_payload="$(payload_for trojan)"
assert_json_value "$trojan_payload" "host" "raw-trojan.example.test"
assert_json_value "$trojan_payload" "port" "443"
assert_json_value "$trojan_payload" "outbound_config.password" "raw-pass"
assert_json_value "$trojan_payload" "outbound_config.server_name" "raw-trojan-sni.example.test"

reset_matrix_env
export THIRD_PARTY_SHADOWSOCKS_RAW_URL="ss://aes-256-gcm:raw-pass@raw-ss.example.test:8388"
export THIRD_PARTY_SHADOWSOCKS_HOST="stale-ss.example.test"
export THIRD_PARTY_SHADOWSOCKS_PORT="28388"
export THIRD_PARTY_SHADOWSOCKS_METHOD="chacha20-ietf-poly1305"
export THIRD_PARTY_SHADOWSOCKS_PASSWORD="stale-pass"
ss_payload="$(payload_for shadowsocks)"
assert_json_value "$ss_payload" "host" "raw-ss.example.test"
assert_json_value "$ss_payload" "port" "8388"
assert_json_value "$ss_payload" "outbound_config.method" "aes-256-gcm"
assert_json_value "$ss_payload" "outbound_config.password" "raw-pass"

reset_matrix_env
export THIRD_PARTY_HY2_RAW_URL="hy2://raw-hy2-pass@raw-hy2.example.test:443?sni=raw-hy2-sni.example.test"
export THIRD_PARTY_HY2_HOST="stale-hy2.example.test"
export THIRD_PARTY_HY2_PORT="2443"
export THIRD_PARTY_HY2_PASSWORD="stale-pass"
export THIRD_PARTY_HY2_SNI="stale-hy2-sni.example.test"
export THIRD_PARTY_HY2_ALLOW_INSECURE="true"
hy2_payload="$(payload_for hy2)"
assert_json_value "$hy2_payload" "host" "raw-hy2.example.test"
assert_json_value "$hy2_payload" "port" "443"
assert_json_value "$hy2_payload" "outbound_config.password" "raw-hy2-pass"
assert_json_value "$hy2_payload" "outbound_config.server_name" "raw-hy2-sni.example.test"
assert_json_absent "$hy2_payload" "outbound_config.allow_insecure"

printf 'real-protocol-matrix-endpoint-payload: passed\n'
