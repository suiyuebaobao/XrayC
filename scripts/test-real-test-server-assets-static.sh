#!/usr/bin/env bash
# 用途：静态验证真实测试服务器资产角色断言，不连接真实服务器。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
empty_env="$tmp_dir/empty.env"
touch "$empty_env" "$tmp_dir/p1" "$tmp_dir/p2" "$tmp_dir/p3" "$tmp_dir/p4"

write_inventory() {
  local path="$1"
  local server1_extra="$2"
  local server4_extra="$3"
  cat >"$path" <<JSON
{"targets":[
 {"alias":"server_1","ssh_host":"192.0.2.1","ssh_user":"root","ssh_password_file":"$tmp_dir/p1"$server1_extra},
 {"alias":"server_2","ssh_host":"192.0.2.2","ssh_user":"root","ssh_password_file":"$tmp_dir/p2"},
 {"alias":"server_3","ssh_host":"192.0.2.3","ssh_user":"root","ssh_password_file":"$tmp_dir/p3"},
 {"alias":"server_4","ssh_host":"192.0.2.4","ssh_user":"root","ssh_password_file":"$tmp_dir/p4"$server4_extra}
]}
JSON
}

assert_inventory_rejected() {
  local inventory="$1"
  local expected="$2"
  local output="$tmp_dir/reject.out"
  set +e
  XRAYC_REAL_RELEASE_ENV_FILE="$empty_env" \
    XRAYC_REAL_E2E_INVENTORY="$inventory" \
    XRAYC_REAL_TEST_SERVER_COUNT=4 \
    bash scripts/check-real-test-servers.sh >"$output" 2>&1
  local status=$?
  set -e
  [[ "$status" -ne 0 ]] || {
    echo "real-test-server-assets-static: inventory unexpectedly passed" >&2
    exit 1
  }
  grep -Fq "$expected" "$output"
}

bad_server1="$tmp_dir/bad-server1.json"
bad_server4="$tmp_dir/bad-server4.json"
write_inventory "$bad_server1" ',"role":"cloudflare edge"' ',"cdn_provider":"cloudflare"'
write_inventory "$bad_server4" "" ""

bad_alias_order="$tmp_dir/bad-alias-order.json"
cat >"$bad_alias_order" <<JSON
{"targets":[
 {"alias":"server_2","ssh_host":"192.0.2.1","ssh_user":"root","ssh_password_file":"$tmp_dir/p1"},
 {"alias":"server_1","ssh_host":"192.0.2.2","ssh_user":"root","ssh_password_file":"$tmp_dir/p2"},
 {"alias":"server_3","ssh_host":"192.0.2.3","ssh_user":"root","ssh_password_file":"$tmp_dir/p3"},
 {"alias":"server_4","ssh_host":"192.0.2.4","ssh_user":"root","ssh_password_file":"$tmp_dir/p4","cdn_provider":"cloudflare"}
]}
JSON

assert_inventory_rejected "$bad_server1" "server_1 must be direct/non-CF"
assert_inventory_rejected "$bad_server4" "server_4 must be marked Cloudflare/orange-cloud"
assert_inventory_rejected "$bad_alias_order" "target_1 alias must be server_1"

grep -Fq '"role"' scripts/check-real-test-servers.sh
grep -Fq '"public_host"' scripts/check-real-test-servers.sh
! grep -Fq 'alias == "server_4" or' scripts/check-real-test-servers.sh

echo "real-test-server-assets-static: passed"
