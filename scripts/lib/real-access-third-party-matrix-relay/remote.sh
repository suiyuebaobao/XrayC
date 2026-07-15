#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的远端 SSH 执行与中继调试函数。
# 负责构造 ssh/sshpass 命令、注入临时环境并执行远端 bash。
# 远端调试输出会统一脱敏，避免泄露 URL、用户信息、IP、UUID、token 或密码。
# 本文件依赖 common.sh 的 status/die，并由 client/assertions 复用。

remote_exec() {
  local index="$1"
  shift
  local ssh_cmd=()
  build_ssh_command "$index" ssh_cmd
  "${ssh_cmd[@]}" "$@"
}

remote_bash_env() {
  local index="$1"
  local env_file="$2"
  shift 2
  local script_file ssh_cmd=()
  script_file="$TMP_DIR/remote-${index}-$$-${RANDOM}.sh"
  cat > "$script_file"
  build_ssh_command "$index" ssh_cmd
  {
    printf 'set -euo pipefail\n'
    cat "$env_file"
    cat "$script_file"
  } | "${ssh_cmd[@]}" "$@" bash -s
}

build_ssh_command() {
  local index="$1"
  local -n out_ref="$2"
  local host user port passfile identity_file
  eval "host=\$H${index}" "user=\$U${index}" "port=\$P${index}" \
    "passfile=\${PF${index}:-}" "identity_file=\${IF${index}:-}"

  out_ref=(
    ssh
    -p "$port"
    -o ConnectTimeout=15
    -o ServerAliveInterval=15
    -o StrictHostKeyChecking=accept-new
  )
  if [[ -n "$identity_file" ]]; then
    out_ref+=(-o BatchMode=yes -i "$identity_file")
  fi
  if [[ -n "$passfile" ]]; then
    out_ref+=(-o BatchMode=no)
    out_ref=(sshpass -f "$passfile" "${out_ref[@]}")
  else
    out_ref+=(-o BatchMode=yes)
  fi
  out_ref+=("${user}@${host}")
}

dump_remote_relay_debug() {
  if ! declare -F remote_exec >/dev/null 2>&1; then
    return 0
  fi
  remote_exec 2 '
    echo "relay_debug: containers"
    docker ps -a --filter label=com.docker.compose.project=xrayc-real-matrix-relay --format "{{.Names}} {{.Status}}" || true
    echo "relay_debug: listeners"
    ss -ltnp | grep -E ":(${LISTEN_PORT}|${XRAY_API_PORT}) " || true
    agent=$(docker ps -q --filter label=com.docker.compose.project=xrayc-real-matrix-relay --filter label=com.docker.compose.service=access-agent | head -n1)
    xray=$(docker ps -q --filter label=com.docker.compose.project=xrayc-real-matrix-relay --filter label=com.docker.compose.service=xray | head -n1)
    echo "relay_debug: agent_logs"
    [ -n "$agent" ] && docker logs --tail 60 "$agent" 2>&1 || true
    echo "relay_debug: xray_logs"
    [ -n "$xray" ] && docker logs --tail 60 "$xray" 2>&1 || true
  ' 2>/dev/null \
    | sed -E 's#(https?|socks5?|vless|trojan|ss|hysteria2?)://[^[:space:]]+#<proxy-url>#Ig; s#//[^/@[:space:]]+:[^/@[:space:]]+@#//<userinfo>@#g; s/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g; s/[0-9a-fA-F-]{36}/<uuid>/g; s/(token|password|secret|key|uuid)[=:][^ ]+/\1=<redacted>/Ig' \
    || true
}
