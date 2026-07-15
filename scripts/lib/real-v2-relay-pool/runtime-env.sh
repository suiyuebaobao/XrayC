#!/usr/bin/env bash

run_real_v2_runtime_env_if_requested() {
  if [[ -z "$runtime_env_out" ]]; then
    return
  fi
  if [[ ! "$local_tunnel_port" =~ ^[1-9][0-9]*$ ]]; then
    die "XRAYC_REAL_E2E_LOCAL_TUNNEL_PORT must be a positive integer"
  fi
  local tunnel_pattern="127.0.0.1:${local_tunnel_port}:127.0.0.1:${CLIENT_SOCKS_PORT}"
  sshpass -f "$PF1" ssh -f -N -p "$P1" \
    -o ExitOnForwardFailure=yes \
    -o ConnectTimeout=15 \
    -o ServerAliveInterval=15 \
    -o StrictHostKeyChecking=accept-new \
    -o BatchMode=no \
    -L "$tunnel_pattern" \
    "$U1@$H1" >/dev/null 2>&1
  ssh_tunnel_pid="$(ps -eo pid=,args= | awk -v pat="$tunnel_pattern" '$0 ~ pat && $0 ~ /[s]sh/ {print $1; exit}')"
  [[ -n "$ssh_tunnel_pid" ]] || die "local SSH tunnel process was not found"

  local client_proxy_url="socks5h://127.0.0.1:${local_tunnel_port}"
  local tunnel_egress=""
  local public_ip_url
  local candidate_egress
  for _ in $(seq 1 30); do
    for public_ip_url in https://api.ipify.org https://ifconfig.me/ip; do
      candidate_egress="$(curl --fail --silent --show-error --connect-timeout 8 --max-time 20 \
        --proxy "$client_proxy_url" "$public_ip_url" 2>/dev/null | tr -d '[:space:]' || true)"
      if [[ "$candidate_egress" == "$actual_egress" ]]; then
        tunnel_egress="$candidate_egress"
        break 2
      fi
    done
    sleep 2
  done
  if [[ -z "$tunnel_egress" ]]; then
    for public_ip_url in https://api.ipify.org https://ifconfig.me/ip; do
      if curl --fail --silent --show-error --connect-timeout 8 --max-time 20 \
        --proxy "$client_proxy_url" "$public_ip_url" >/dev/null 2>&1; then
        die "local tunnel egress differs from remote client egress"
      fi
    done
    die "local tunnel egress probe failed"
  fi
  for _ in $(seq 1 3); do
    if curl --fail --silent --show-error --connect-timeout 8 --max-time 20 \
      --proxy "$client_proxy_url" https://api.ipify.org >/dev/null 2>&1; then
      break
    fi
    sleep 2
  done

  mkdir -p "$(dirname "$runtime_env_out")"
  RUNTIME_ENV_OUT="$runtime_env_out" \
  BASE_URL_VALUE="$BASE_URL" DB_URL_VALUE="$DB_URL" SUB_TOKEN_VALUE="$sub_token" \
  CLIENT_PROXY_URL_VALUE="$client_proxy_url" EXPECTED_EXIT_IP_VALUE="$actual_egress" \
  ACCESS_NODE_ID_VALUE="$access_node_id" ACCESS_LINE_ID_VALUE="$access_line_id" \
  EXIT_ENDPOINT_ID_VALUE="$assigned_exit_endpoint_id" USER_ACCESS_TOKEN_VALUE="$user_token" \
  AGENT_TOKEN_VALUE="$agent_token" XRAY_USER_KEY_VALUE="$user_xray_key" SSH_TUNNEL_PID_VALUE="$ssh_tunnel_pid" \
    python3 - <<'PY'
import os
import shlex

path = os.environ["RUNTIME_ENV_OUT"]
values = {
    "BASE_URL": os.environ["BASE_URL_VALUE"],
    "DATABASE_URL": os.environ["DB_URL_VALUE"],
    "SUB_TOKEN": os.environ["SUB_TOKEN_VALUE"],
    "CLIENT_PROXY_URL": os.environ["CLIENT_PROXY_URL_VALUE"],
    "EXPECTED_EXIT_IP": os.environ["EXPECTED_EXIT_IP_VALUE"],
    "ACCESS_NODE_ID": os.environ["ACCESS_NODE_ID_VALUE"],
    "ACCESS_LINE_ID": os.environ["ACCESS_LINE_ID_VALUE"],
    "EXIT_ENDPOINT_ID": os.environ["EXIT_ENDPOINT_ID_VALUE"],
    "USER_ACCESS_TOKEN": os.environ["USER_ACCESS_TOKEN_VALUE"],
    "AGENT_TOKEN": os.environ["AGENT_TOKEN_VALUE"],
    "XRAY_USER_KEY": os.environ["XRAY_USER_KEY_VALUE"],
    "UAT_ACCESS_NODE_IDS": os.environ["ACCESS_NODE_ID_VALUE"],
    "UAT_ACCESS_LINE_IDS": os.environ["ACCESS_LINE_ID_VALUE"],
    "UAT_EXIT_ENDPOINT_IDS": os.environ["EXIT_ENDPOINT_ID_VALUE"],
    "UAT_CLIENT_PROXY_URLS": os.environ["CLIENT_PROXY_URL_VALUE"],
    "UAT_EXPECTED_EXIT_IPS": os.environ["EXPECTED_EXIT_IP_VALUE"],
    "UAT_USER_ACCESS_TOKENS": os.environ["USER_ACCESS_TOKEN_VALUE"],
    "UAT_AGENT_TOKENS": os.environ["AGENT_TOKEN_VALUE"],
    "XRAYC_RUNTIME_UAT_SSH_TUNNEL_PID": os.environ["SSH_TUNNEL_PID_VALUE"],
}
with open(path, "w", encoding="utf-8") as fh:
    fh.write("# 私有文件：真实中转长稳 UAT 环境，不要提交。\n")
    for key, value in values.items():
        fh.write(f"{key}={shlex.quote(value)}\n")
os.chmod(path, 0o600)
PY
  echo "real_v2_relay_pool_e2e: runtime_env_written"
}
