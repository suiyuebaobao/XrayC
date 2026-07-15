#!/usr/bin/env bash
# 这个 helper 负责真实入站矩阵 E2E 的远程客户端执行。
# 这个 helper 负责远程 xray 镜像准备和协议配置上传。
# 这个 helper 负责逐协议启动容器并校验出口结果。
# 这个 helper 负责失败时的脱敏远程调试输出。
# 这个 helper 只定义函数，由主脚本 source 后调用。
# 这个 helper 不打印真实 URL、IP、令牌或代理地址。
# 这个 helper 属于 real-access-inbound-matrix-e2e 的拆分实现。
# 这个 helper 依赖主脚本提供 TMP_DIR、DOCKER_IMAGE 等变量。
# 这个 helper 依赖 inventory-ssh helper 提供远程执行函数。
# 这个 helper 修改时需保持 bash -n 通过。

write_remote_runner() {
  cat > "$TMP_DIR/remote-runner.sh" <<'REMOTE'
set -euo pipefail
set +x
umask 077

base_dir="/opt/xrayc-inbound-matrix"
mkdir -p "$base_dir/configs"
if ! docker image inspect "$DOCKER_IMAGE" >/dev/null 2>&1; then
  if [[ -z "${DEPLOY_ARTIFACT_TOKEN:-}" || -z "${BASE_URL:-}" ]]; then
    exit 17
  fi
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT
  printf 'url = "%s/api/deploy/artifacts/xray-image.tar.gz"\nheader = "Authorization: Bearer %s"\n' "${BASE_URL%/}" "$DEPLOY_ARTIFACT_TOKEN" > "$tmp_dir/curl.conf"
  chmod 600 "$tmp_dir/curl.conf"
  curl --fail --silent --location --max-time 180 --config "$tmp_dir/curl.conf" --output "$tmp_dir/xray-image.tar.gz" 2>/dev/null
  docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
fi
REMOTE
}

retry_remote_exec_stdin() {
  local stdin_file="$1"
  local command="$2"
  local attempt max_attempts

  max_attempts="$(remote_retry_attempts)"
  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
    if remote_exec "$command" < "$stdin_file"; then
      return 0
    fi
    sleep "$((attempt * 2))"
  done
  return 1
}

retry_remote_bash_file() {
  local script_file="$1"
  local output_file="${2:-}"
  local attempt max_attempts

  max_attempts="$(remote_retry_attempts)"
  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
    if [[ -n "$output_file" ]]; then
      if remote_bash_file "$script_file" > "$output_file" 2>/dev/null; then
        return 0
      fi
    else
      if remote_bash_file "$script_file"; then
        return 0
      fi
    fi
    sleep "$((attempt * 2))"
  done
  return 1
}

remote_retry_attempts() {
  local value="${REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES:-3}"
  [[ "$value" =~ ^[0-9]+$ && "$value" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES must be a positive integer"
  printf '%s' "$value"
}

traffic_attempts() {
  local value="${REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_ATTEMPTS:-20}"
  [[ "$value" =~ ^[0-9]+$ && "$value" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_ATTEMPTS must be a positive integer"
  printf '%s' "$value"
}

remote_prepare_image() {
  write_remote_runner
  BASE_URL_VALUE="$BASE_URL" DEPLOY_TOKEN_VALUE="${DEPLOY_ARTIFACT_TOKEN:-}" DOCKER_IMAGE_VALUE="$DOCKER_IMAGE" \
    python3 - "$TMP_DIR/remote-env.sh" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"export BASE_URL={shlex.quote(os.environ['BASE_URL_VALUE'])}\n")
    fh.write(f"export DEPLOY_ARTIFACT_TOKEN={shlex.quote(os.environ.get('DEPLOY_TOKEN_VALUE', ''))}\n")
    fh.write(f"export DOCKER_IMAGE={shlex.quote(os.environ['DOCKER_IMAGE_VALUE'])}\n")
PY
  cat "$TMP_DIR/remote-env.sh" "$TMP_DIR/remote-runner.sh" > "$TMP_DIR/remote-prepare.sh"
  retry_remote_bash_file "$TMP_DIR/remote-prepare.sh" || die "remote client image preparation failed; details redacted"
}

remote_write_config() {
  local protocol="$1"
  local config_file="$TMP_DIR/configs/${protocol}.json"
  [[ -f "$config_file" ]] || die "client config build failed"
  retry_remote_exec_stdin "$config_file" "mkdir -p /opt/xrayc-inbound-matrix/configs && cat > /opt/xrayc-inbound-matrix/configs/${protocol}.json" \
    || die "remote client config upload failed; details redacted"
}

expected_exit_ip_for() {
  local suffix="$1"
  local name
  name="REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_${suffix}"
  if [[ -n "${!name:-}" ]]; then
    printf '%s' "${!name}"
    return 0
  fi
}

remote_run_protocol() {
  local protocol="$1"
  local suffix="$2"
  local port="$3"
  local expected_ip actual_file actual_ip container attempt_count
  expected_ip="$(expected_exit_ip_for "$suffix")"
  attempt_count="$(traffic_attempts)"
  require_real_value "REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_${suffix}" "${expected_ip:-set}"
  container="xrayc-inbound-matrix-${protocol}"
  remote_write_config "$protocol"
  actual_file="$TMP_DIR/${protocol}.egress"
  PROTOCOL_VALUE="$protocol" CONTAINER_VALUE="$container" PORT_VALUE="$port" \
  TRAFFIC_URL_VALUE="$TRAFFIC_URL" CURL_TIMEOUT_VALUE="$CURL_TIMEOUT" DOCKER_IMAGE_VALUE="$DOCKER_IMAGE" \
  DEBUG_VALUE="$DEBUG_OUTPUT" TRAFFIC_ATTEMPTS_VALUE="$attempt_count" \
    python3 - "$TMP_DIR/run-${protocol}.sh" <<'PY'
import os
import shlex
import sys

values = {name: os.environ[name] for name in (
    "PROTOCOL_VALUE",
    "CONTAINER_VALUE",
    "PORT_VALUE",
    "TRAFFIC_URL_VALUE",
    "CURL_TIMEOUT_VALUE",
    "DOCKER_IMAGE_VALUE",
    "DEBUG_VALUE",
    "TRAFFIC_ATTEMPTS_VALUE",
)}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    for key, value in values.items():
        fh.write(f"export {key}={shlex.quote(value)}\n")
    fh.write(r'''
set -euo pipefail
set +x
docker rm -f "$CONTAINER_VALUE" >/dev/null 2>&1 || true
docker run -d --name "$CONTAINER_VALUE" --network host \
  -v "/opt/xrayc-inbound-matrix/configs/${PROTOCOL_VALUE}.json:/etc/xray/config.json:ro" \
  "$DOCKER_IMAGE_VALUE" run -config /etc/xray/config.json >/dev/null
for _ in $(seq 1 "$TRAFFIC_ATTEMPTS_VALUE"); do
  if curl --fail --silent --location --max-time "$CURL_TIMEOUT_VALUE" \
    --socks5-hostname "127.0.0.1:${PORT_VALUE}" "$TRAFFIC_URL_VALUE" >/tmp/xrayc-inbound-matrix-egress 2>/dev/null; then
    tr -d '\r\n' </tmp/xrayc-inbound-matrix-egress
    docker rm -f "$CONTAINER_VALUE" >/dev/null 2>&1 || true
    exit 0
  fi
  sleep 1
done
if [[ "$DEBUG_VALUE" != "1" ]]; then
  docker logs "$CONTAINER_VALUE" >/dev/null 2>&1 || true
  docker rm -f "$CONTAINER_VALUE" >/dev/null 2>&1 || true
fi
exit 1
''')
PY
  if ! retry_remote_bash_file "$TMP_DIR/run-${protocol}.sh" "$actual_file"; then
    dump_remote_protocol_debug "$container" "$port" "$protocol"
    die "${protocol} inbound traffic failed; remote output redacted"
  fi
  actual_ip="$(cat "$actual_file")"
  if [[ -n "$expected_ip" && "$actual_ip" != "$expected_ip" ]]; then
    die "${protocol} inbound egress did not match expected value; IPs redacted"
  fi
  status "${protocol}_inbound_ok"
}

dump_remote_protocol_debug() {
  local container="$1"
  local port="$2"
  local protocol="$3"
  local debug_file="$TMP_DIR/${protocol}.remote-debug"
  local debug_script="$TMP_DIR/${protocol}.remote-debug.sh"

  [[ "$DEBUG_OUTPUT" == "1" ]] || return 0
  CONTAINER_VALUE="$container" PORT_VALUE="$port" python3 - "$debug_script" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"export CONTAINER_VALUE={shlex.quote(os.environ['CONTAINER_VALUE'])}\n")
    fh.write(f"export PORT_VALUE={shlex.quote(os.environ['PORT_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
echo "container_state_begin"
docker ps -a --filter "name=${CONTAINER_VALUE}" --format '{{.Names}} {{.Status}}' || true
echo "listen_state_begin"
(ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null || true) | awk -v port=":${PORT_VALUE}" '$0 ~ port {print $0}'
echo "upstream_tcp_begin"
python3 - "/opt/xrayc-inbound-matrix/configs/${CONTAINER_VALUE#xrayc-inbound-matrix-}.json" <<'PYREMOTE' >/tmp/xrayc-inbound-upstream.env 2>/dev/null || true
import json
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    config = json.load(fh)
outbound = config.get("outbounds", [{}])[0]
settings = outbound.get("settings", {})
host = ""
port = 0
vnext = settings.get("vnext")
servers = settings.get("servers")
if isinstance(vnext, list) and vnext:
    host = str(vnext[0].get("address") or "")
    port = int(vnext[0].get("port") or 0)
elif isinstance(servers, list) and servers:
    host = str(servers[0].get("address") or "")
    port = int(servers[0].get("port") or 0)
if host and port:
    print(f"UPSTREAM_HOST={shlex.quote(host)}")
    print(f"UPSTREAM_PORT={port}")
PYREMOTE
if [ -s /tmp/xrayc-inbound-upstream.env ]; then
  . /tmp/xrayc-inbound-upstream.env
  if timeout 6 bash -c ":</dev/tcp/${UPSTREAM_HOST}/${UPSTREAM_PORT}" >/dev/null 2>&1; then
    echo "upstream_tcp=open"
  else
    echo "upstream_tcp=closed"
  fi
else
  echo "upstream_tcp=unknown"
fi
echo "container_logs_begin"
docker logs --tail 80 "$CONTAINER_VALUE" 2>&1 || true
''')
PY
  remote_bash_file "$debug_script" >"$debug_file" 2>/dev/null || true
  sed -E \
    -e 's#(https?|socks5?|vless|trojan|ss|hysteria2?)://[^[:space:]]+#<url>#Ig' \
    -e 's#//[^/@[:space:]]+:[^/@[:space:]]+@#//<userinfo>@#g' \
    -e 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g' \
    -e 's/[A-Za-z0-9._-]+\.(com|net|org|io|fun|dev|app|cloud|cn|xyz|site|test)/<domain>/g' \
    -e 's/[0-9a-fA-F-]{36}/<uuid>/g' \
    -e 's/(token|password|secret|key|uuid|id)[=:][^ ]+/\1=<redacted>/Ig' \
    "$debug_file" >&2 || true
  dump_access_target_debug
}

dump_access_target_debug() {
  local debug_file="$TMP_DIR/access-target.remote-debug"
  local debug_script="$TMP_DIR/access-target.remote-debug.sh"

  [[ "$DEBUG_OUTPUT" == "1" ]] || return 0
  [[ -n "${TEMP_OPENED_PORTS:-}" ]] || return 0
  declare -p ACCESS_SSH_CMD >/dev/null 2>&1 || return 0
  [[ -n "${ACCESS_SSH_DEST:-}" ]] || return 0
  PORTS_VALUE="$TEMP_OPENED_PORTS" python3 - "$debug_script" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"PORTS={shlex.quote(os.environ['PORTS_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
echo "access_target_listen_begin"
for port in $PORTS; do
  (ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null || true) | awk -v port=":${port}" '$0 ~ port {print $0}'
done
echo "access_target_containers_begin"
docker ps --format '{{.Names}} {{.Status}}' | grep -Ei 'xray|agent|relay' || true
echo "access_target_xray_logs_begin"
for name in $(docker ps --format '{{.Names}}' | grep -Ei 'xray' | head -3); do
  echo "container=${name}"
  docker logs --tail 80 "$name" 2>&1 || true
done
''')
PY
  access_remote_bash_file "$debug_script" >"$debug_file" 2>/dev/null || true
  sed -E \
    -e 's#(https?|socks5?|vless|trojan|ss|hysteria2?)://[^[:space:]]+#<url>#Ig' \
    -e 's#//[^/@[:space:]]+:[^/@[:space:]]+@#//<userinfo>@#g' \
    -e 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g' \
    -e 's/[A-Za-z0-9._-]+\.(com|net|org|io|fun|dev|app|cloud|cn|xyz|site|test)/<domain>/g' \
    -e 's/[0-9a-fA-F-]{36}/<uuid>/g' \
    -e 's/(token|password|secret|key|uuid|id)[=:][^ ]+/\1=<redacted>/Ig' \
    "$debug_file" >&2 || true
}
