# 真实 V2 中转池客户端启动辅助函数。
# 由 scripts/real-v2-relay-pool-e2e.sh source 使用。
# 负责把订阅解析出的连接参数写成远端客户端配置。
# 负责启动远端客户端容器并做 SOCKS 出口可用性探测。
# 失败时只输出脱敏后的容器状态和日志片段。
# 本文件不直接执行，依赖主脚本初始化的远端执行函数和全局变量。
# shellcheck shell=bash

setup_client_runtime() {
  local client_env_file="$TMP_DIR/client.env"
  local client_config_helper_b64
  client_config_helper_b64="$(base64 -w0 "$BASE_DIR/scripts/lib/real-v2-relay-pool/client-config.py")"
  BASE_URL="$BASE_URL" DEPLOY_TOKEN="$DEPLOY_TOKEN" CLIENT_SERVER="$CLIENT_SERVER" CLIENT_PORT="$CLIENT_PORT" \
    CLIENT_PROTOCOL="${CLIENT_PROTOCOL:-vless}" CLIENT_CIPHER="${CLIENT_CIPHER:-}" CLIENT_PASSWORD="${CLIENT_PASSWORD:-}" \
    CLIENT_UUID="$CLIENT_UUID" CLIENT_NETWORK="$CLIENT_NETWORK" CLIENT_SECURITY="$CLIENT_SECURITY" \
    CLIENT_SERVER_NAME="${CLIENT_SERVER_NAME:-}" CLIENT_FINGERPRINT="${CLIENT_FINGERPRINT:-}" \
    CLIENT_REALITY_PUBLIC_KEY="${CLIENT_REALITY_PUBLIC_KEY:-}" CLIENT_REALITY_SHORT_ID="${CLIENT_REALITY_SHORT_ID:-}" \
    CLIENT_FLOW="$CLIENT_FLOW" CLIENT_SOCKS_PORT="$CLIENT_SOCKS_PORT" CLIENT_UDP_PORT="$CLIENT_UDP_PORT" \
    UDP_TARGET_HOST="${EXIT_B_UDP_TARGET_HOST:-$EXIT_B_PUBLIC_HOST}" UDP_TARGET_PORT="$UDP_TARGET_PORT" CLIENT_CONFIG_HELPER_B64="$client_config_helper_b64" \
    write_shell_env "$client_env_file" BASE_URL DEPLOY_TOKEN CLIENT_PROTOCOL CLIENT_SERVER CLIENT_PORT CLIENT_UUID CLIENT_NETWORK CLIENT_SECURITY CLIENT_SERVER_NAME CLIENT_FINGERPRINT CLIENT_REALITY_PUBLIC_KEY CLIENT_REALITY_SHORT_ID CLIENT_FLOW CLIENT_CIPHER CLIENT_PASSWORD CLIENT_SOCKS_PORT CLIENT_UDP_PORT UDP_TARGET_HOST UDP_TARGET_PORT CLIENT_CONFIG_HELPER_B64
  if ! remote_bash_env 1 "$client_env_file" <<'REMOTE'
set -euo pipefail
install_dir="/opt/xrayc-real-client"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
mkdir -p "$install_dir"
printf 'header = "Authorization: Bearer %s"\n' "$DEPLOY_TOKEN" > "$tmp_dir/curl.conf"
curl --fail --silent --show-error --location --config "$tmp_dir/curl.conf" \
  --output "$tmp_dir/xray-image.tar.gz" \
  "${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz" >/dev/null
docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
python3 - "$tmp_dir/client-config.py" <<'PY'
import base64
import os
import sys

with open(sys.argv[1], "wb") as fh:
    fh.write(base64.b64decode(os.environ["CLIENT_CONFIG_HELPER_B64"]))
PY
python3 "$tmp_dir/client-config.py" "$install_dir/config.json"
docker rm -f xrayc-real-client >/dev/null 2>&1 || true
docker run -d --name xrayc-real-client --restart unless-stopped --network host \
  -v "$install_dir/config.json:/etc/xray/config.json:ro" \
  xrayc/xray:local run -config /etc/xray/config.json >/dev/null
for _ in $(seq 1 20); do
  if curl --fail --silent --show-error --max-time 8 \
    --socks5-hostname "127.0.0.1:${CLIENT_SOCKS_PORT}" https://api.ipify.org >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done
echo "client proxy readiness failed; container status redacted" >&2
docker ps --filter name=xrayc-real-client --format 'client_container={{.Status}}' >&2 || true
redact_sensitive() {
  python3 -c 'import os, sys
data = sys.stdin.read()
for key, replacement in (
    ("CLIENT_SERVER", "<transit>"),
    ("CLIENT_UUID", "<credential>"),
    ("CLIENT_PASSWORD", "<credential>"),
    ("CLIENT_SERVER_NAME", "<server-name>"),
    ("CLIENT_REALITY_PUBLIC_KEY", "<reality-key>"),
    ("CLIENT_REALITY_SHORT_ID", "<reality-short-id>"),
):
    value = os.environ.get(key, "")
    if value:
        data = data.replace(value, replacement)
sys.stdout.write(data)'
}
curl --verbose --max-time 8 --socks5-hostname "127.0.0.1:${CLIENT_SOCKS_PORT}" https://api.ipify.org 2>&1 \
  | redact_sensitive >&2 || true
docker logs --tail 20 xrayc-real-client 2>&1 \
  | redact_sensitive >&2 || true
exit 1
REMOTE
  then
    remote_exec 2 "docker ps --filter label=com.docker.compose.project=xrayc-real-relay --format 'relay_container={{.Names}} {{.Status}}'" >&2 || true
    remote_exec 2 "docker logs --tail 40 \$(docker ps -q --filter label=com.docker.compose.project=xrayc-real-relay --filter name=xrayc-xray | head -n 1) 2>&1 | sed 's/[0-9]\\{1,3\\}\\(\\.[0-9]\\{1,3\\}\\)\\{3\\}/<ip>/g' | tail -40" >&2 || true
    remote_exec 2 "docker logs --tail 40 \$(docker ps -q --filter label=com.docker.compose.project=xrayc-real-relay --filter name=access-agent | head -n 1) 2>&1 | sed 's/[0-9]\\{1,3\\}\\(\\.[0-9]\\{1,3\\}\\)\\{3\\}/<ip>/g' | tail -40" >&2 || true
    die "client proxy readiness failed"
  fi
}

client_egress_ip() {
  local actual
  actual="$(remote_exec 1 "curl --fail --silent --show-error --max-time 12 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org" 2>/dev/null | tr -d '[:space:]')" || return 1
  printf '%s\n' "$actual"
}

client_egress_matches_expected() {
  local expected_ip="${1:-${expected_egress_ip:-$exit_a_ip}}"
  local actual
  actual="$(client_egress_ip)" || return 1
  [[ "$actual" == "$expected_ip" ]]
}

client_egress_matches_configured_exit() {
  local actual
  actual="$(client_egress_ip)" || return 1
  [[ "$actual" == "$exit_a_ip" || "$actual" == "$exit_b_ip" ]]
}

wait_client_expected_egress() {
  local expected_ip="${1:-${expected_egress_ip:-$exit_a_ip}}"
  local label="${2:-matched_configured_exit}"
  local candidate
  actual_egress=""
  for _ in $(seq 1 36); do
    candidate="$(client_egress_ip)" || {
      sleep 5
      continue
    }
    if [[ "$candidate" == "$expected_ip" ]]; then
      actual_egress="$candidate"
      expected_egress_ip="$expected_ip"
      echo "real_v2_relay_pool_e2e: egress_ok ${label}"
      return
    fi
    sleep 5
  done
  die "egress IP did not match bound exit"
}

wait_client_blocked() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if ! client_egress_matches_configured_exit; then
      echo "real_v2_relay_pool_e2e: client_eviction_ok reason=${reason}"
      return
    fi
    sleep 5
  done
  die "client still reaches expected egress after ${reason}"
}

wait_client_restored() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if actual_egress="$(client_egress_ip)" && [[ "$actual_egress" == "$exit_a_ip" || "$actual_egress" == "$exit_b_ip" ]]; then
      expected_egress_ip="$actual_egress"
      echo "real_v2_relay_pool_e2e: client_restore_ok reason=${reason}"
      return
    fi
    sleep 5
  done
  die "client egress did not restore after ${reason}"
}

restart_client_runtime() {
  remote_exec 1 "docker restart xrayc-real-client >/dev/null 2>&1 || true" >/dev/null 2>&1 || true
}

wait_initial_client_exit_assignment() {
assigned_exit_endpoint_id=""
expected_egress_ip=""
for _ in $(seq 1 36); do
  remote_exec 1 "curl --fail --silent --show-error --max-time 12 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org >/dev/null" >/dev/null 2>&1 || true
  assigned_exit_endpoint_id="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" -v line_id="$access_line_id" <<'SQL'
SELECT exit_endpoint_id
FROM user_exit_assignments
WHERE user_id = :'user_id'::uuid
  AND access_line_id = :'line_id'::uuid
LIMIT 1;
SQL
)"
  case "$assigned_exit_endpoint_id" in
    "$exit_endpoint_a")
      expected_egress_ip="$exit_a_ip"
      break
      ;;
    "$exit_endpoint_b")
      expected_egress_ip="$exit_b_ip"
      break
      ;;
  esac
  sleep 5
done
[[ -n "$expected_egress_ip" ]] || die "assigned exit endpoint is not one of the bound line endpoints"
wait_client_expected_egress "$expected_egress_ip" "matched_configured_exit"
}
