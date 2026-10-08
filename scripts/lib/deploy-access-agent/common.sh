#!/usr/bin/env bash
# 用途：提供 deploy-access-agent.sh 的通用输出、校验和转义函数。
# 该文件只定义函数，由主部署脚本 source 使用，不直接执行部署。

usage() {
  cat <<'USAGE'
Usage:
  XRAYC_CONTROL_PLANE_URL="http://example.com:8080" \
  XRAYC_DEPLOY_ARTIFACT_TOKEN="<set-in-env>" \
  bash scripts/deploy-access-agent.sh

Required environment variables:
  XRAYC_CONTROL_PLANE_URL       Center API origin, for example http://example.com:8080.
  XRAYC_DEPLOY_ARTIFACT_TOKEN   Short-lived token for /api/deploy/artifacts/{name}.
                                DEPLOY_ARTIFACT_TOKEN is also accepted.

Optional environment variables:
  XRAYC_NODE_ID                 Existing access node id. When omitted, the script
                                reuses the installed value or generates a new one.
  XRAYC_NODE_TOKEN              Existing access-agent bearer token. When omitted,
                                the script reuses the installed value or generates
                                a new one.
  XRAYC_DEPLOY_BASE_URL         Artifact API origin. Defaults to XRAYC_CONTROL_PLANE_URL.
  XRAYC_INSTALL_DIR             Work directory, default: /opt/xrayc/access-agent.
  XRAYC_COMPOSE_PROJECT_NAME    Docker Compose project name, default: xrayc-access.
  XRAYC_XRAY_API_SERVER         Stats API address, default: 127.0.0.1:10085.
  XRAYC_XRAY_API_LISTEN_HOST    Stats API listen host, default: 127.0.0.1.
  XRAYC_XRAY_API_LISTEN_PORT    Stats API listen port, default: derived from server or 10085.
  XRAYC_HEARTBEAT_INTERVAL_SECONDS  Default: 30.
  XRAYC_TRAFFIC_INTERVAL_SECONDS    Default: 60.
  XRAYC_SESSION_IDLE_SECONDS        Session idle window, default: 180.
  XRAYC_EXPECTED_LISTEN_PORTS   Space-separated inbound ports that must be listening.
  XRAYC_TLS_CERT_DOMAINS        Space-separated domains for automatic certbot
                                certificates used by TLS/Trojan inbounds.
  XRAYC_TLS_CERT_EMAIL          ACME account email. Required together with
                                XRAYC_TLS_CERT_DOMAINS for automatic certbot.
  XRAYC_DEPLOY_READY_TIMEOUT_SECONDS  Service readiness wait timeout, default: 120.
  XRAYC_DEPLOY_START            Start/restart Docker Compose services, default: true.
  XRAYC_DEPLOY_ROLLBACK_ON_FAILURE
                                Roll back containers and files created by a failed deploy, default: true.
  XRAYC_DEPLOY_ROLLBACK_REMOVE_EXISTING
                                Remove an existing install dir on failure, default: false.
  XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS  Disable old XrayC systemd units, default: false.
  XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS
                                Deprecated. Existing containers and logs are preserved; cross-project cleanup is disabled.
  XRAYC_XRAY_RECLAIM_COMMAND    Command the agent runs after a successful Xray reload to
                                reclaim stale Xray containers/processes that still hold the
                                Stats API port via SO_REUSEPORT. Defaults to a docker-socket
                                script that only stops non-current Xray containers in this release instance.
  CURL_TIMEOUT                  Curl timeout seconds, default: 120.

This script deploys the remote access node with Docker Compose. It downloads
access-agent-image.tar.gz, xray-image.tar.gz,
access-agent.sha256 and access-agent-manifest.json from the center, verifies
sha256, runs docker load, writes docker-compose.yml, runs docker compose up -d,
then waits for container readiness, agent state and optional expected ports. It
never stops SSH services or touches SSH processes.
USAGE
}

log() {
  printf '[xrayc-access-deploy] %s\n' "$*" >&2
}

die() {
  printf '[xrayc-access-deploy] ERROR: %s\n' "$*" >&2
  exit 1
}

log_redacted_tail() {
  local file="$1"
  [[ -s "$file" ]] || return 0
  tail -60 "$file" 2>/dev/null | sed -E \
    -e 's#https?://[^[:space:]]+#<URL>#g' \
    -e 's#[0-9]{1,3}(\.[0-9]{1,3}){3}#<IP>#g' \
    -e 's#(Bearer )[A-Za-z0-9._~+/-]+#\1<redacted>#g' \
    -e 's#(token|password|secret|credential)[=:][^[:space:]]+#\1=<redacted>#Ig' >&2 || true
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    die "${name} is required"
  fi
  case "${!name}" in
    *$'\n'*|*$'\r'*)
      die "${name} must not contain newlines"
      ;;
  esac
}

run_root() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  else
    sudo -n "$@"
  fi
}

docker_run() {
  if [[ "$(id -u)" -eq 0 ]]; then
    docker "$@"
  else
    sudo -n docker "$@"
  fi
}

compose_run() {
  if docker_run compose version >/dev/null 2>&1; then
    docker_run compose "$@"
  elif [[ "$(id -u)" -eq 0 ]] && command -v docker-compose >/dev/null 2>&1; then
    docker-compose "$@"
  elif [[ "$(id -u)" -ne 0 ]] && sudo -n sh -c 'command -v docker-compose >/dev/null 2>&1'; then
    sudo -n docker-compose "$@"
  else
    return 127
  fi
}

json_value() {
  local key="$1"
  local file="$2"
  sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" "$file" | head -n 1
}

write_file_root() {
  local mode="$1"
  local owner="$2"
  local group="$3"
  local src="$4"
  local dest="$5"
  run_root install -D -m "$mode" -o "$owner" -g "$group" "$src" "$dest"
}

env_escape() {
  # 这些值写进 access-agent.env,经 compose `env_file:` 字面注入容器(env_file 不做 $$→$ 反转义,
  # 与 compose `environment:` 内联插值不同)。agent 直接 env::var 读出后交给 `sh -c` 执行,
  # 因此 `$XRAYC_XRAY_CONFIG` 这类必须保留**单个 $** 让 agent 侧 shell 展开。
  # 旧实现 `s/\$/$$/g` 把 $→$$,env_file 不反转义 → 容器内字面含 $$ → agent `sh -c` 把 $$ 展开成
  # shell PID → xray -test 的 -config 路径变垃圾(open <PID>XRAYC_XRAY_CONFIG: no such file,EXIT 23)
  # → 每心跳校验必败 → record_config_result 抹 desired_config_hash=NULL → 节点永久不收敛(BUG-C 根因)。
  # 只转义双引号值结构必需的 \ 与 ",不再 double $。
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

write_env_line() {
  local key="$1"
  local value="$2"
  case "$value" in
    *$'\n'*|*$'\r'*)
      die "${key} must not contain newlines"
      ;;
  esac
  printf '%s="%s"\n' "$key" "$(env_escape "$value")"
}

yaml_escape() {
  printf "%s" "$1" | sed "s/'/''/g"
}

curl_config_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

derive_port() {
  local server="$1"
  if [[ "$server" =~ :([0-9]+)$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
  else
    printf '10085\n'
  fi
}

read_existing_env_value() {
  local file="$1"
  local key="$2"
  run_root awk -v key="$key" '
    index($0, key "=") == 1 {
      value = substr($0, length(key) + 2)
      if (value ~ /^".*"$/) {
        value = substr(value, 2, length(value) - 2)
        gsub(/\\"/, "\"", value)
        gsub(/\\\\/, "\\", value)
      }
      print value
      exit
    }
  ' "$file" 2>/dev/null || true
}

generate_node_uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  elif [[ -r /proc/sys/kernel/random/uuid ]]; then
    tr '[:upper:]' '[:lower:]' </proc/sys/kernel/random/uuid
  elif command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
  else
    die "cannot generate XRAYC_NODE_ID; install uuidgen or python3"
  fi
}

generate_node_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
  elif command -v base64 >/dev/null 2>&1; then
    head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '='
  else
    die "cannot generate XRAYC_NODE_TOKEN; install openssl or base64"
  fi
}

ensure_node_credentials() {
  local existing_env="${INSTALL_DIR}/access-agent.env"
  if [[ -z "${XRAYC_NODE_ID:-}" && -f "$existing_env" ]]; then
    XRAYC_NODE_ID="$(read_existing_env_value "$existing_env" XRAYC_NODE_ID)"
  fi
  if [[ -z "${XRAYC_NODE_TOKEN:-}" && -f "$existing_env" ]]; then
    XRAYC_NODE_TOKEN="$(read_existing_env_value "$existing_env" XRAYC_NODE_TOKEN)"
  fi
  if [[ -z "${XRAYC_NODE_ID:-}" ]]; then
    XRAYC_NODE_ID="$(generate_node_uuid)"
  fi
  if [[ -z "${XRAYC_NODE_TOKEN:-}" ]]; then
    XRAYC_NODE_TOKEN="$(generate_node_token)"
  fi
  if ! [[ "$XRAYC_NODE_ID" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
    die "XRAYC_NODE_ID must be a UUID"
  fi
  require_env XRAYC_NODE_TOKEN
  XRAYC_AGENT_AUTH_CODE="xrayc-agent-v1:${XRAYC_NODE_ID}:${XRAYC_NODE_TOKEN}"
}
