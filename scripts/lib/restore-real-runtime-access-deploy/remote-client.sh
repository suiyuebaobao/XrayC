#!/usr/bin/env bash
# shellcheck shell=bash

restore_client_alloc_local_port() {
  python3 - <<'PY'
import socket

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
}

restore_client_init() {
  client_mode="${XRAYC_RUNTIME_RESTORE_CLIENT_MODE:-remote}"
  client_socks_port="${XRAYC_RUNTIME_RESTORE_CLIENT_SOCKS_PORT:-31080}"
  case "$client_mode" in
    local|remote) ;;
    *) die "XRAYC_RUNTIME_RESTORE_CLIENT_MODE must be local or remote" ;;
  esac
  [[ "$client_socks_port" =~ ^[1-9][0-9]*$ && "$client_socks_port" -le 65535 ]] \
    || die "XRAYC_RUNTIME_RESTORE_CLIENT_SOCKS_PORT must be a TCP port"

  if [[ "$client_mode" != "remote" ]]; then
    return 0
  fi

  RESTORE_CLIENT_TARGET_INDEX="${XRAYC_RUNTIME_RESTORE_CLIENT_TARGET_INDEX:-0}" \
  python3 - "$XRAYC_REAL_E2E_INVENTORY" >"$TMP_DIR/client-target.env" <<'PY'
import json
import os
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
if not targets:
    raise SystemExit("inventory requires at least one client target")
try:
    index = int(os.environ.get("RESTORE_CLIENT_TARGET_INDEX", "0"))
except ValueError as exc:
    raise SystemExit("client target index is invalid") from exc
if index < 0 or index >= len(targets):
    raise SystemExit("client target index is outside inventory")
target = targets[index]
values = {
    "CLIENT_SSH_HOST": target.get("ssh_host", ""),
    "CLIENT_SSH_USER": target.get("ssh_user", "root"),
    "CLIENT_SSH_PORT": str(target.get("ssh_port", 22)),
    "CLIENT_SSH_PASSWORD_FILE": target.get("ssh_password_file", ""),
    "CLIENT_TARGET_INDEX": str(index + 1),
}
for key, value in values.items():
    if not str(value).strip():
        raise SystemExit(f"inventory client target missing {key}")
    print(f"{key}={shlex.quote(str(value))}")
PY
  # shellcheck disable=SC1090
  . "$TMP_DIR/client-target.env"
  [[ -f "$CLIENT_SSH_PASSWORD_FILE" ]] || die "client SSH password file is missing"
  upsert_env_value REAL_RUNTIME_CLIENT_TARGET_INDEX "$CLIENT_TARGET_INDEX"
}

restore_client_config_port() {
  if [[ "$client_mode" == "remote" ]]; then
    printf '%s\n' "$client_socks_port"
  else
    restore_client_alloc_local_port
  fi
}

restore_client_start_remote() {
  local client_config_file="$1"
  local client_port="$2"
  local remote_client_tmp="/tmp/xrayc-real-runtime-client-${RANDOM}-$$"
  local remote_client_install_dir="/opt/xrayc-real-runtime-client"
  local remote_client_env="$TMP_DIR/remote-client.env"
  local remote_client_script="$TMP_DIR/remote-client-install.sh"

  docker rm -f xrayc-real-runtime-client >/dev/null 2>&1 || true
  BASE_URL_VALUE="$BASE_URL" DEPLOY_TOKEN_VALUE="$DEPLOY_ARTIFACT_TOKEN" \
  CLIENT_PORT_VALUE="$client_port" INSTALL_DIR_VALUE="$remote_client_install_dir" \
  PUBLIC_IP_URL_VALUE="${PUBLIC_IP_URL:-https://api.ipify.org}" \
    python3 - "$remote_client_env" <<'PY'
import os
import shlex
import sys

values = {
    "BASE_URL": os.environ["BASE_URL_VALUE"],
    "DEPLOY_TOKEN": os.environ["DEPLOY_TOKEN_VALUE"],
    "CLIENT_PORT": os.environ["CLIENT_PORT_VALUE"],
    "INSTALL_DIR": os.environ["INSTALL_DIR_VALUE"],
    "PUBLIC_IP_URL": os.environ["PUBLIC_IP_URL_VALUE"],
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    for key, value in values.items():
        fh.write(f"{key}={shlex.quote(value)}\n")
os.chmod(sys.argv[1], 0o600)
PY
  cat >"$remote_client_script" <<'REMOTE'
#!/usr/bin/env bash
set -euo pipefail
remote_tmp="$1"
# shellcheck disable=SC1090
. "${remote_tmp}/client.env"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
mkdir -p "$INSTALL_DIR"
cp "${remote_tmp}/config.json" "${INSTALL_DIR}/config.json"
printf 'header = "Authorization: Bearer %s"\n' "$DEPLOY_TOKEN" > "$tmp_dir/curl.conf"
curl --fail --silent --show-error --location --config "$tmp_dir/curl.conf" \
  --output "$tmp_dir/xray-image.tar.gz" \
  "${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz" >/dev/null
docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
docker rm -f xrayc-real-runtime-client >/dev/null 2>&1 || true
docker run -d --name xrayc-real-runtime-client --restart unless-stopped --network host \
  -v "${INSTALL_DIR}/config.json:/etc/xray/config.json:ro" \
  xrayc/xray:local run -config /etc/xray/config.json >/dev/null
for _ in $(seq 1 20); do
  if curl --fail --silent --show-error --max-time 12 \
    --socks5-hostname "127.0.0.1:${CLIENT_PORT}" "$PUBLIC_IP_URL" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done
echo "remote client readiness failed; details redacted" >&2
docker ps --filter name=xrayc-real-runtime-client --format 'client_container={{.Status}}' >&2 || true
exit 1
REMOTE
  chmod 700 "$remote_client_script"
  sshpass -f "$CLIENT_SSH_PASSWORD_FILE" ssh -p "$CLIENT_SSH_PORT" \
    -o ConnectTimeout=15 -o ServerAliveInterval=15 \
    -o StrictHostKeyChecking=accept-new -o BatchMode=no \
    "$CLIENT_SSH_USER@$CLIENT_SSH_HOST" \
    "rm -rf '$remote_client_tmp'; mkdir -p '$remote_client_tmp'" >/dev/null
  sshpass -f "$CLIENT_SSH_PASSWORD_FILE" scp -P "$CLIENT_SSH_PORT" \
    -o ConnectTimeout=15 -o StrictHostKeyChecking=accept-new -o BatchMode=no \
    "$client_config_file" "$remote_client_env" "$remote_client_script" \
    "$CLIENT_SSH_USER@$CLIENT_SSH_HOST:$remote_client_tmp/" >/dev/null
  sshpass -f "$CLIENT_SSH_PASSWORD_FILE" ssh -p "$CLIENT_SSH_PORT" \
    -o ConnectTimeout=15 -o ServerAliveInterval=15 \
    -o StrictHostKeyChecking=accept-new -o BatchMode=no \
    "$CLIENT_SSH_USER@$CLIENT_SSH_HOST" \
    "mv '$remote_client_tmp/$(basename "$client_config_file")' '$remote_client_tmp/config.json'; mv '$remote_client_tmp/$(basename "$remote_client_env")' '$remote_client_tmp/client.env'; bash '$remote_client_tmp/$(basename "$remote_client_script")' '$remote_client_tmp'; rm -rf '$remote_client_tmp'" >/dev/null

  local client_tunnel_port="${XRAYC_RUNTIME_RESTORE_LOCAL_TUNNEL_PORT:-}"
  if [[ -z "$client_tunnel_port" ]]; then
    client_tunnel_port="$(restore_client_alloc_local_port)"
  fi
  [[ "$client_tunnel_port" =~ ^[1-9][0-9]*$ && "$client_tunnel_port" -le 65535 ]] \
    || die "XRAYC_RUNTIME_RESTORE_LOCAL_TUNNEL_PORT must be a TCP port"
  local tunnel_pattern="127.0.0.1:${client_tunnel_port}:127.0.0.1:${client_port}"
  sshpass -f "$CLIENT_SSH_PASSWORD_FILE" ssh -f -N -p "$CLIENT_SSH_PORT" \
    -o ExitOnForwardFailure=yes \
    -o ConnectTimeout=15 \
    -o ServerAliveInterval=15 \
    -o StrictHostKeyChecking=accept-new \
    -o BatchMode=no \
    -L "$tunnel_pattern" \
    "$CLIENT_SSH_USER@$CLIENT_SSH_HOST" >/dev/null 2>&1
  local client_tunnel_pid
  client_tunnel_pid="$(ps -eo pid=,args= | awk -v pat="$tunnel_pattern" '$0 ~ pat && $0 ~ /[s]sh/ {print $1; exit}')"
  [[ -n "$client_tunnel_pid" ]] || die "client SSH tunnel process was not found"
  CLIENT_PROXY_URL="socks5h://127.0.0.1:${client_tunnel_port}"
  upsert_env_value XRAYC_RUNTIME_UAT_SSH_TUNNEL_PID "$client_tunnel_pid"
}

restore_client_start_local() {
  local client_config_file="$1"
  local client_port="$2"

  docker rm -f xrayc-real-runtime-client >/dev/null 2>&1 || true
  docker run -d --name xrayc-real-runtime-client --restart unless-stopped --network host \
    -v "$client_config_file:/etc/xray/config.json:ro" \
    xrayc/xray:local run -config /etc/xray/config.json >/dev/null
  CLIENT_PROXY_URL="socks5h://127.0.0.1:${client_port}"
}

restore_client_start() {
  local client_config_file="$1"
  local client_port="$2"

  if [[ "$client_mode" == "remote" ]]; then
    restore_client_start_remote "$client_config_file" "$client_port"
  else
    restore_client_start_local "$client_config_file" "$client_port"
  fi
  upsert_env_value CLIENT_PROXY_URL "$CLIENT_PROXY_URL"
  upsert_env_value EXPECTED_CLIENT_PROXY_ENDPOINTS "127.0.0.1:${CLIENT_PROXY_URL##*:}"
  upsert_env_value UAT_CLIENT_PROXY_URLS "$CLIENT_PROXY_URL"
}
