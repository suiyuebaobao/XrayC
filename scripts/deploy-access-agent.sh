#!/usr/bin/env bash
# 用途：部署远程 access-agent，并从 helper 目录加载实现细节。
# 本文件保留入口、环境变量装配和部署主流程，避免单文件过长。
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_ACCESS_AGENT_LIB_DIR="${SCRIPT_DIR}/lib/deploy-access-agent"

source "${DEPLOY_ACCESS_AGENT_LIB_DIR}/common.sh"
source "${DEPLOY_ACCESS_AGENT_LIB_DIR}/tooling.sh"
source "${DEPLOY_ACCESS_AGENT_LIB_DIR}/deploy.sh"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

CONTROL_PLANE_URL="${XRAYC_CONTROL_PLANE_URL:-}"
DEPLOY_BASE_URL="${XRAYC_DEPLOY_BASE_URL:-$CONTROL_PLANE_URL}"
DEPLOY_ARTIFACT_TOKEN="${XRAYC_DEPLOY_ARTIFACT_TOKEN:-${DEPLOY_ARTIFACT_TOKEN:-}}"
DEPLOY_TASK_ID="${XRAYC_DEPLOY_TASK_ID:-}"
DEPLOY_TASK_REPORT_TOKEN="${XRAYC_DEPLOY_TASK_REPORT_TOKEN:-}"
INSTALL_DIR="${XRAYC_INSTALL_DIR:-/opt/xrayc/access-agent}"
ARTIFACT_DIR="${INSTALL_DIR}/artifacts"
COMPOSE_PROJECT_NAME="${XRAYC_COMPOSE_PROJECT_NAME:-xrayc-access}"
CURL_TIMEOUT="${CURL_TIMEOUT:-120}"
DEPLOY_START="${XRAYC_DEPLOY_START:-true}"
DISABLE_LEGACY_SYSTEMD_UNITS="${XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS:-false}"
CLEAN_LEGACY_COMPOSE_PROJECTS="${XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS:-true}"
REUSE_EXISTING_IMAGES="${XRAYC_DEPLOY_REUSE_EXISTING_IMAGES:-false}"
OVERWRITE_RUNTIME_CONFIG="${XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG:-${XRAYC_DEPLOY_OVERWRITE_XRAY_CONFIG:-false}}"

require_env CONTROL_PLANE_URL
require_env DEPLOY_BASE_URL
require_env DEPLOY_ARTIFACT_TOKEN

CONTROL_PLANE_URL="${CONTROL_PLANE_URL%/}"
DEPLOY_BASE_URL="${DEPLOY_BASE_URL%/}"

report_deploy_progress() {
  local status="$1"
  local step="$2"
  local progress="$3"
  if [[ -z "$DEPLOY_TASK_ID" || -z "$DEPLOY_TASK_REPORT_TOKEN" ]]; then
    return 0
  fi
  local err_file="${tmp_dir:-/tmp}/deploy-task-report.err"
  curl --fail --silent --show-error \
    --connect-timeout 10 \
    --max-time 30 \
    -H 'Content-Type: application/json' \
    -X POST \
    --data "{\"report_token\":\"${DEPLOY_TASK_REPORT_TOKEN}\",\"status\":\"${status}\",\"step\":\"${step}\",\"progress_percent\":${progress}}" \
    "${CONTROL_PLANE_URL}/api/deployment-tasks/${DEPLOY_TASK_ID}/report" \
    >/dev/null 2>"$err_file" || log "deployment progress report skipped"
}

finish_deploy_trap() {
  local status="$?"
  if [[ "$status" -ne 0 ]]; then
    report_deploy_progress failed failed 100
  fi
  cleanup_all "$status"
  exit "$status"
}

XRAY_API_SERVER="${XRAYC_XRAY_API_SERVER:-127.0.0.1:10085}"
XRAY_API_LISTEN_HOST="${XRAYC_XRAY_API_LISTEN_HOST:-127.0.0.1}"
XRAY_API_LISTEN_PORT="${XRAYC_XRAY_API_LISTEN_PORT:-$(derive_port "$XRAY_API_SERVER")}"
HEARTBEAT_INTERVAL="${XRAYC_HEARTBEAT_INTERVAL_SECONDS:-30}"
TRAFFIC_INTERVAL="${XRAYC_TRAFFIC_INTERVAL_SECONDS:-60}"
SESSION_IDLE_SECONDS="${XRAYC_SESSION_IDLE_SECONDS:-180}"
EXPECTED_LISTEN_PORTS="${XRAYC_EXPECTED_LISTEN_PORTS:-}"
TLS_CERT_DOMAINS="${XRAYC_TLS_CERT_DOMAINS:-}"
TLS_CERT_EMAIL="${XRAYC_TLS_CERT_EMAIL:-}"
# CF 橙云域名 DNS-01 证书参数:cf_cert_mode=dns01 时给 cf_domain 用 CF Token 签自己的证书。
# CLOUDFLARE_API_TOKEN 仅安装/续期用、写节点本机 0600 ini,绝不回显/不进日志。
CF_CERT_MODE="${XRAYC_CF_CERT_MODE:-reuse_direct}"
CF_DOMAIN="${XRAYC_CF_DOMAIN:-}"
CLOUDFLARE_API_TOKEN="${XRAYC_CLOUDFLARE_API_TOKEN:-}"
DEPLOY_READY_TIMEOUT="${XRAYC_DEPLOY_READY_TIMEOUT_SECONDS:-300}"
RUST_LOG_VALUE="${RUST_LOG:-info,xrayc_access_agent=debug}"
XRAY_RELOAD_COMMAND="${XRAYC_XRAY_RELOAD_COMMAND:-}"
DEPLOY_ROLLBACK_ON_FAILURE="${XRAYC_DEPLOY_ROLLBACK_ON_FAILURE:-true}"
DEPLOY_ROLLBACK_REMOVE_EXISTING="${XRAYC_DEPLOY_ROLLBACK_REMOVE_EXISTING:-$OVERWRITE_RUNTIME_CONFIG}"
RATE_LIMITER_ENABLED="${XRAYC_RATE_LIMITER_ENABLED:-true}"
RATE_LIMITER_DRY_RUN="${XRAYC_RATE_LIMITER_DRY_RUN:-false}"
RATE_LIMITER_INTERFACE="${XRAYC_RATE_LIMITER_INTERFACE:-}"
RATE_LIMITER_IFB_INTERFACE="${XRAYC_RATE_LIMITER_IFB_INTERFACE:-ifb-xrayc}"
RATE_LIMITER_ROOT_RATE_BPS="${XRAYC_RATE_LIMITER_ROOT_RATE_BPS:-10000000000}"
DEPLOY_START_EPOCH="$(date +%s)"

XRAY_CONFIG_CONTAINER_DIR="/etc/xray"
XRAY_LOG_CONTAINER_DIR="/var/log/xray"
XRAY_HOST_CONFIG_DIR="${INSTALL_DIR}/xray"
XRAY_HOST_LOG_DIR="${INSTALL_DIR}/logs/xray"
XRAY_HOST_BINARY_PATH="${INSTALL_DIR}/bin/xrayc-xray"
XRAY_CONFIG_CONTAINER_PATH="${XRAY_CONFIG_CONTAINER_DIR}/config.json"
XRAY_ACCESS_LOG_CONTAINER_PATH="${XRAY_LOG_CONTAINER_DIR}/access.log"

ensure_node_credentials
EXISTING_TLS_CERT_DOMAINS=""
if [[ -f "${INSTALL_DIR}/access-agent.env" ]]; then
  EXISTING_TLS_CERT_DOMAINS="$(read_existing_env_value "${INSTALL_DIR}/access-agent.env" XRAYC_TLS_CERT_DOMAINS)"
fi

if [[ -z "$RATE_LIMITER_INTERFACE" ]]; then
  RATE_LIMITER_INTERFACE="$(ip route show default 2>/dev/null | awk '$1 == "default" { for (i = 1; i <= NF; i++) if ($i == "dev") { print $(i + 1); exit } }')"
  RATE_LIMITER_INTERFACE="${RATE_LIMITER_INTERFACE:-eth0}"
fi

INSTALL_DIR_PREEXISTING=0
if run_root test -e "$INSTALL_DIR" >/dev/null 2>&1; then
  INSTALL_DIR_PREEXISTING=1
fi

tmp_dir="$(mktemp -d)"
DEPLOY_CREATED_INSTALL_DIR=0
DEPLOY_COMPOSE_UP_ATTEMPTED=0
DEPLOY_CONTAINER_NAMES_TOUCHED=0

trap finish_deploy_trap EXIT
report_deploy_progress running server_started 10

CURL_CONFIG="${tmp_dir}/curl.conf"
umask 077
printf 'header = "Authorization: Bearer %s"\n' "$(curl_config_escape "$DEPLOY_ARTIFACT_TOKEN")" > "$CURL_CONFIG"
umask 022

ensure_tooling
cleanup_existing_compose_project_for_force_reinstall
cleanup_existing_install_files_for_force_reinstall
cleanup_legacy_compose_projects
cleanup_tls_certificates_for_force_reinstall
install_tls_certificates

log "preparing install directories"
run_root install -d -m 0755 \
  "$INSTALL_DIR" \
  "$ARTIFACT_DIR" \
  "${INSTALL_DIR}/bin" \
  "$XRAY_HOST_CONFIG_DIR" \
  "${INSTALL_DIR}/certs"
if [[ "$INSTALL_DIR_PREEXISTING" -eq 0 ]]; then
  DEPLOY_CREATED_INSTALL_DIR=1
fi
run_root install -d -m 0755 "$XRAY_HOST_LOG_DIR"
run_root install -d -m 0700 "${INSTALL_DIR}/state"
run_root touch "${XRAY_HOST_LOG_DIR}/access.log"
run_root chmod 0644 "${XRAY_HOST_LOG_DIR}/access.log"

download_artifact access-agent-manifest.json

ACCESS_AGENT_IMAGE="$(json_value access_agent_image "${ARTIFACT_DIR}/access-agent-manifest.json")"
XRAY_IMAGE="$(json_value xray_image "${ARTIFACT_DIR}/access-agent-manifest.json")"
[[ -n "$ACCESS_AGENT_IMAGE" ]] || die "manifest missing access_agent_image"
[[ -n "$XRAY_IMAGE" ]] || die "manifest missing xray_image"

reuse_existing_images=0
case "$REUSE_EXISTING_IMAGES" in
  1|true|TRUE|yes|YES) reuse_existing_images=1 ;;
esac

downloaded_access_agent_image=0
downloaded_xray_image=0
if [[ "$reuse_existing_images" == "1" ]] && docker_run image inspect "$ACCESS_AGENT_IMAGE" >/dev/null 2> "${tmp_dir}/docker-reuse-inspect-access-agent.err"; then
  log "reusing existing access-agent Docker image"
else
  download_artifact access-agent-image.tar.gz
  downloaded_access_agent_image=1
fi
if [[ "$reuse_existing_images" == "1" ]] && docker_run image inspect "$XRAY_IMAGE" >/dev/null 2> "${tmp_dir}/docker-reuse-inspect-xray.err"; then
  log "reusing existing xray Docker image"
else
  download_artifact xray-image.tar.gz
  downloaded_xray_image=1
fi
download_artifact access-agent.sha256

log "verifying downloaded artifact checksums"
checksum_names_regex='^(access-agent-manifest\.json'
if [[ "$downloaded_access_agent_image" == "1" ]]; then
  checksum_names_regex+='|access-agent-image\.tar\.gz'
fi
if [[ "$downloaded_xray_image" == "1" ]]; then
  checksum_names_regex+='|xray-image\.tar\.gz'
fi
checksum_names_regex+=')$'
(cd "$ARTIFACT_DIR" && awk -v names="$checksum_names_regex" '$2 ~ names {print}' access-agent.sha256 | sha256sum -c -)
report_deploy_progress running artifacts_downloaded 45

log "loading Docker images"
if [[ "$downloaded_access_agent_image" == "1" ]]; then
  if ! docker_run load --input "${ARTIFACT_DIR}/access-agent-image.tar.gz" >/dev/null 2> "${tmp_dir}/docker-load-access-agent.err"; then
    log "access-agent docker load failed; trying filesystem image import fallback"
    if ! docker_run import "${ARTIFACT_DIR}/access-agent-image.tar.gz" "$ACCESS_AGENT_IMAGE" >/dev/null 2> "${tmp_dir}/docker-import-access-agent.err"; then
      die "Docker image load/import failed for access-agent artifact; stderr redacted"
    fi
  fi
fi
if [[ "$downloaded_xray_image" == "1" ]]; then
  if ! docker_run load --input "${ARTIFACT_DIR}/xray-image.tar.gz" >/dev/null 2> "${tmp_dir}/docker-load-xray.err"; then
    die "Docker image load failed for xray artifact; stderr redacted"
  fi
fi
if ! docker_run image inspect "$ACCESS_AGENT_IMAGE" >/dev/null 2> "${tmp_dir}/docker-inspect-access-agent.err"; then
  die "loaded access-agent Docker image is unavailable; stderr redacted"
fi
if ! docker_run image inspect "$XRAY_IMAGE" >/dev/null 2> "${tmp_dir}/docker-inspect-xray.err"; then
  die "loaded xray Docker image is unavailable; stderr redacted"
fi

xray_bin="${tmp_dir}/xrayc-xray"
extract_binary "$XRAY_IMAGE" "$xray_bin" /usr/local/bin/xray /usr/bin/xray /xray /app/xray

log "installing runtime config test binaries"
write_file_root 0755 root root "$xray_bin" "$XRAY_HOST_BINARY_PATH"

initial_xray_config="${tmp_dir}/xray-config.json"
cat > "$initial_xray_config" <<'JSON'
{
  "log": {
    "loglevel": "warning"
  },
  "inbounds": [],
  "outbounds": [
    {
      "tag": "direct",
      "protocol": "freedom"
    }
  ]
}
JSON

if [[ ! -f "${XRAY_HOST_CONFIG_DIR}/config.json" || "$OVERWRITE_RUNTIME_CONFIG" == "true" ]]; then
  log "installing initial Xray config"
  write_file_root 0644 root root "$initial_xray_config" "${XRAY_HOST_CONFIG_DIR}/config.json"
else
  log "keeping existing Xray config"
fi

if [[ "$OVERWRITE_RUNTIME_CONFIG" == "true" ]]; then
  log "clearing stale access-agent state for forced reinstall"
  run_root rm -f "${INSTALL_DIR}/state/state.json" "${INSTALL_DIR}/state/traffic-backlog.json"
fi

container_suffix="$(printf '%s' "$XRAYC_NODE_ID" | sha256sum | cut -c1-12)"
[[ -n "$container_suffix" ]] || container_suffix="default"
xray_container="xrayc-xray-${container_suffix}"
agent_container="xrayc-access-agent-${container_suffix}"

if [[ -z "$XRAY_RELOAD_COMMAND" ]]; then
  XRAY_RELOAD_COMMAND="curl --fail --silent --show-error --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/restart?t=0' >/dev/null"
fi
# Xray 旧实例回收命令：reload/redeploy 切换 Xray 时，旧容器可能已被删但进程未退，
# 通过 SO_REUSEPORT 继续占用 Stats API 端口（如 10085），导致 agent statsquery
# 被内核分流到旧进程读不到当前用户流量 → 计费失效。回收脚本通过 docker socket
# 枚举所有 xrayc-xray-* 容器，删除非当前实例（含其残留进程），确保同一 Stats 端口
# 只有当前 Xray 在监听。回收命令为独立脚本文件，避免 env 转义破坏内联 shell。
RECLAIM_SCRIPT_CONTAINER_PATH="/var/lib/xrayc/access-agent/reclaim-xray.sh"
XRAY_RECLAIM_COMMAND="${XRAYC_XRAY_RECLAIM_COMMAND:-sh ${RECLAIM_SCRIPT_CONTAINER_PATH}}"
XRAY_START_COMMAND="${XRAYC_XRAY_START_COMMAND:-curl --fail --silent --show-error --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/start' >/dev/null}"
XRAY_STOP_COMMAND="${XRAYC_XRAY_STOP_COMMAND:-curl --fail --silent --show-error --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/stop?t=10' >/dev/null}"
# BUG-E：xray-test 命令含运行时 $XRAYC_XRAY_CONFIG，必须留到 agent 侧由 sh -c 展开。
# 但若经 env_file 注入，compose v1 不插值（保留单 $，正确）、compose v2 会插值
# （此刻 $XRAYC_XRAY_CONFIG 未定义）→ -config "" → xray-test 报 flag needs an argument
# → agent 退回空配置 → 永不收敛、443 不监听；任何单一转义都无法 v1/v2 两边都对。
# 根除依赖：默认不把该命令写进 env_file，改由 agent config.rs 自带等价默认、由 agent
# 自己 sh -c 展开（不经 compose 插值）。仅当运维显式覆盖时才透传（其责任自负）。
# 其余命令型值（reload/start/stop/reclaim）在脚本期已用 ${xray_container} 等展开，
# 字面不含运行时 $VAR，故经 env_file 在 v1/v2 下均安全。

env_file="${tmp_dir}/access-agent.env"
{
  write_env_line XRAYC_NODE_ID "$XRAYC_NODE_ID"
  write_env_line XRAYC_CONTROL_PLANE_URL "$CONTROL_PLANE_URL"
  write_env_line XRAYC_NODE_TOKEN "$XRAYC_NODE_TOKEN"
  write_env_line XRAYC_HEARTBEAT_INTERVAL_SECONDS "$HEARTBEAT_INTERVAL"
  write_env_line XRAYC_TRAFFIC_INTERVAL_SECONDS "$TRAFFIC_INTERVAL"
  write_env_line XRAYC_SESSION_IDLE_SECONDS "$SESSION_IDLE_SECONDS"
  write_env_line XRAYC_XRAY_CONFIG_PATH "/etc/xray/config.json"
  write_env_line XRAYC_AGENT_STATE_PATH "/var/lib/xrayc/access-agent/state.json"
  write_env_line XRAYC_TRAFFIC_BACKLOG_PATH "/var/lib/xrayc/access-agent/traffic-backlog.json"
  write_env_line XRAYC_XRAY_ACCESS_LOG_PATH "$XRAY_ACCESS_LOG_CONTAINER_PATH"
  write_env_line XRAYC_XRAY_BINARY "/usr/local/bin/xrayc-xray"
  # 默认不注入 XRAYC_XRAY_TEST_COMMAND（见上方 BUG-E 注释）：agent 自带等价默认并自行展开。
  # 仅当运维显式覆盖时才透传该自定义命令（若仍含运行时 $VAR，由覆盖方对 v1/v2 语义自负）。
  if [[ -n "${XRAYC_XRAY_TEST_COMMAND:-}" ]]; then
    write_env_line XRAYC_XRAY_TEST_COMMAND "$XRAYC_XRAY_TEST_COMMAND"
  fi
  write_env_line XRAYC_XRAY_RELOAD_COMMAND "$XRAY_RELOAD_COMMAND"
  write_env_line XRAYC_XRAY_RECLAIM_COMMAND "$XRAY_RECLAIM_COMMAND"
  write_env_line XRAYC_XRAY_START_COMMAND "$XRAY_START_COMMAND"
  write_env_line XRAYC_XRAY_STOP_COMMAND "$XRAY_STOP_COMMAND"
  write_env_line XRAYC_XRAY_API_SERVER "$XRAY_API_SERVER"
  write_env_line XRAYC_XRAY_API_LISTEN_HOST "$XRAY_API_LISTEN_HOST"
  write_env_line XRAYC_XRAY_API_LISTEN_PORT "$XRAY_API_LISTEN_PORT"
  write_env_line XRAYC_TLS_CERT_DOMAINS "$TLS_CERT_DOMAINS"
  write_env_line XRAYC_TLS_CERT_EMAIL "$TLS_CERT_EMAIL"
  # CF DNS-01 证书:cf_cert_mode/cf_domain 落 agent env 供续期判定;
  # CF Token 也写入 agent env(仅节点本机,不回控制面/不入日志),agent 据此在续期前
  # 重写 0600 ini 给 certbot-dns-cloudflare 用。write_env_line 只写文件、不回显。
  write_env_line XRAYC_CF_CERT_MODE "$CF_CERT_MODE"
  write_env_line XRAYC_CF_DOMAIN "$CF_DOMAIN"
  write_env_line XRAYC_CLOUDFLARE_API_TOKEN "$CLOUDFLARE_API_TOKEN"
  write_env_line XRAYC_XRAY_CONTAINER_SUFFIX "$container_suffix"
  write_env_line XRAYC_XRAY_CONTAINER_NAME "$xray_container"
  write_env_line XRAYC_XRAY_RELOAD_MATCH "$XRAY_CONFIG_CONTAINER_PATH"
  write_env_line XRAYC_RATE_LIMITER_ENABLED "$RATE_LIMITER_ENABLED"
  write_env_line XRAYC_RATE_LIMITER_DRY_RUN "$RATE_LIMITER_DRY_RUN"
  write_env_line XRAYC_RATE_LIMITER_INTERFACE "$RATE_LIMITER_INTERFACE"
  write_env_line XRAYC_RATE_LIMITER_IFB_INTERFACE "$RATE_LIMITER_IFB_INTERFACE"
  write_env_line XRAYC_RATE_LIMITER_ROOT_RATE_BPS "$RATE_LIMITER_ROOT_RATE_BPS"
  write_env_line XRAYC_RATE_LIMITER_PLAN_PATH "/var/lib/xrayc/access-agent/limiter-plan.sh"
  # 监控中心宿主磁盘采集挂载点(阶段B):与 compose 里 `- /:/hostfs:ro` 对齐,
  # agent 对 /hostfs 跑 statvfs 读宿主磁盘用量;挂载缺失时 agent 自身回退 "/"。
  write_env_line XRAYC_HOST_FS_ROOT "/hostfs"
  write_env_line RUST_LOG "$RUST_LOG_VALUE"
} > "$env_file"
log "installing access-agent environment file"
write_file_root 0600 root root "$env_file" "${INSTALL_DIR}/access-agent.env"

# 写入 Xray 旧实例回收脚本到状态目录（已挂载进 agent 容器为
# /var/lib/xrayc/access-agent/reclaim-xray.sh）。脚本只在 reload 成功后由 agent 执行。
# 通过 docker socket 枚举所有 xrayc-xray-* 容器，删除非当前实例及其残留进程，
# 保证同一 Stats API 端口只剩当前 Xray 监听，避免 statsquery 读到旧进程导致计费失效。
reclaim_script="${tmp_dir}/reclaim-xray.sh"
cat > "$reclaim_script" <<RECLAIM
#!/bin/sh
# 由 deploy-access-agent.sh 生成，回收占用 Stats 端口的陈旧 Xray 容器。
# 当前 Xray 容器名在部署时固定写入，回收时据此跳过当前实例。
set -u
sock=/var/run/docker.sock
current="${xray_container}"
list="\$(curl --fail --silent --show-error --unix-socket "\$sock" 'http://localhost/containers/json?all=1' 2>/dev/null)" || exit 0
# 提取所有 xrayc-xray-* 容器名（Names 形如 ["/xrayc-xray-<suffix>"]），排除 agent。
names="\$(printf '%s' "\$list" | grep -oE '"/xrayc-xray-[A-Za-z0-9_.-]+"' | tr -d '"/' | sort -u)"
status=0
for name in \$names; do
  [ "\$name" = "\$current" ] && continue
  # 先停后强删旧容器；强删会向其主进程发 SIGKILL，连同残留进程一起回收。
  curl --fail --silent --show-error --unix-socket "\$sock" -X POST "http://localhost/containers/\$name/stop?t=5" >/dev/null 2>&1 || true
  if ! curl --fail --silent --show-error --unix-socket "\$sock" -X DELETE "http://localhost/containers/\$name?force=true" >/dev/null 2>&1; then
    status=1
  fi
done
exit \$status
RECLAIM
write_file_root 0755 root root "$reclaim_script" "${INSTALL_DIR}/state/reclaim-xray.sh"

compose_file="${tmp_dir}/docker-compose.yml"
cat > "$compose_file" <<YAML
services:
  xray:
    image: '$(yaml_escape "$XRAY_IMAGE")'
    container_name: '$(yaml_escape "$xray_container")'
    restart: unless-stopped
    user: "0:0"
    network_mode: host
    cap_add:
      - NET_ADMIN
    command: ["run", "-config", "$(yaml_escape "$XRAY_CONFIG_CONTAINER_PATH")"]
    environment:
      XRAYC_XRAY_CONTAINER_SUFFIX: '$(yaml_escape "$container_suffix")'
    volumes:
      - '$(yaml_escape "$XRAY_HOST_CONFIG_DIR"):$(yaml_escape "$XRAY_CONFIG_CONTAINER_DIR"):rw'
      - '$(yaml_escape "$XRAY_HOST_LOG_DIR"):$(yaml_escape "$XRAY_LOG_CONTAINER_DIR"):rw'
      - /etc/letsencrypt:/etc/letsencrypt:ro
      - '$(yaml_escape "${INSTALL_DIR}/certs"):$(yaml_escape "$XRAY_CONFIG_CONTAINER_DIR")/certs:ro'

  access-agent:
    image: '$(yaml_escape "$ACCESS_AGENT_IMAGE")'
    container_name: '$(yaml_escape "$agent_container")'
    restart: unless-stopped
    user: "0:0"
    network_mode: host
    cap_add:
      - NET_ADMIN
      - SYS_MODULE
    depends_on:
      - xray
    env_file:
      - '$(yaml_escape "${INSTALL_DIR}/access-agent.env")'
    command: ["/usr/local/bin/xrayc-access-agent"]
    volumes:
      - '$(yaml_escape "$XRAY_HOST_CONFIG_DIR"):$(yaml_escape "$XRAY_CONFIG_CONTAINER_DIR"):rw'
      - '$(yaml_escape "${INSTALL_DIR}/state"):/var/lib/xrayc/access-agent:rw'
      - '$(yaml_escape "$XRAY_HOST_LOG_DIR"):$(yaml_escape "$XRAY_LOG_CONTAINER_DIR"):rw'
      - '$(yaml_escape "$XRAY_HOST_BINARY_PATH"):/usr/local/bin/xrayc-xray:ro'
      # access-agent 自签节点全部域名证书,需对 /etc/letsencrypt 可写(agent 写、xray 只读)。
      - /etc/letsencrypt:/etc/letsencrypt:rw
      - '$(yaml_escape "${INSTALL_DIR}/certs"):$(yaml_escape "$XRAY_CONFIG_CONTAINER_DIR")/certs:ro'
      - /var/run/docker.sock:/var/run/docker.sock:rw
      - /lib/modules:/lib/modules:ro
      # 监控中心宿主磁盘采集(阶段B):把宿主根 / 只读挂到容器 /hostfs,agent 对它跑 statvfs
      # 读宿主磁盘用量。/proc/stat、/proc/meminfo 因 network_mode: host + 容器默认即宿主视图,
      # 无需额外挂载;只有磁盘 statvfs 需要这条 host 根挂载。只读、绝不写宿主根。
      - /:/hostfs:ro
YAML

log "installing Docker Compose file"
write_file_root 0644 root root "$compose_file" "${INSTALL_DIR}/docker-compose.yml"
if ! compose_run -f "${INSTALL_DIR}/docker-compose.yml" -p "$COMPOSE_PROJECT_NAME" config >/dev/null 2> "${tmp_dir}/compose-config.err"; then
  log_redacted_tail "${tmp_dir}/compose-config.err"
  die "Docker Compose config validation failed; stderr redacted"
fi

if [[ "$DISABLE_LEGACY_SYSTEMD_UNITS" == "true" ]]; then
  disable_legacy_systemd_units
else
  log "legacy systemd cleanup skipped; set XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS=true to remove old project units"
fi

if [[ "$DEPLOY_START" == "true" ]]; then
  log "starting xrayc Docker Compose services"
  if [[ "$OVERWRITE_RUNTIME_CONFIG" == "true" ]]; then
    log "stopping existing project containers before forced reinstall"
    DEPLOY_COMPOSE_UP_ATTEMPTED=1
    compose_run -f "${INSTALL_DIR}/docker-compose.yml" -p "$COMPOSE_PROJECT_NAME" down --remove-orphans >/dev/null 2>&1 || true
  fi
  DEPLOY_CONTAINER_NAMES_TOUCHED=1
  docker_run rm -f "$agent_container" "$xray_container" >/dev/null 2>&1 || true
  DEPLOY_COMPOSE_UP_ATTEMPTED=1
  if ! compose_run -f "${INSTALL_DIR}/docker-compose.yml" -p "$COMPOSE_PROJECT_NAME" up -d --remove-orphans >/dev/null 2> "${tmp_dir}/compose-up.err"; then
    log_redacted_tail "${tmp_dir}/compose-up.err"
    die "Docker Compose service start failed; stderr redacted"
  fi
  wait_container_running "$xray_container"
  wait_container_running "$agent_container"
  report_deploy_progress running containers_started 85
  # 容器已 running 即 agent 进程已起、会主动回连控制面收敛：此刻先把鉴权码打到 stdout。
  # 这样即使下面 wait_agent_state / 端口等待因空节点或瞬时抖动超时(降级为告警),
  # 控制面也已拿到鉴权码可登记节点 + 凭心跳判定上线,不会因"进度回报跳过/就绪等待超时"误判失败。
  log "节点鉴权码：${XRAYC_AGENT_AUTH_CODE}"
  # 就绪等待降级为尽力而为信号：超时只告警不退出、不回滚,部署不据此判失败。
  wait_agent_state || log "access-agent state readiness wait skipped; agent will converge via heartbeat"
  wait_expected_listen_ports
else
  log "XRAYC_DEPLOY_START is not true; compose file was installed but not started"
  # 未启动也输出鉴权码,保持安装说明/一键安装的鉴权码契约一致。
  log "节点鉴权码：${XRAYC_AGENT_AUTH_CODE}"
fi

log "deployment completed"
report_deploy_progress succeeded agent_ready 100
log "status check: use docker ps for xrayc-xray and xrayc-access-agent containers; install directory redacted"
log "把上面的节点鉴权码复制到管理平台新增中转节点；平台添加成功后 agent 会自动通过心跳并拉取配置。"
