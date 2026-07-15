#!/usr/bin/env bash
# 用途：对私有远端 access 节点执行真实 Agent 安装 E2E 验证。
# 范围：读取私有 inventory，在服务器侧运行安装脚本，并验证远端监听和控制面联通。
# 输入：需要 XRAYC_REAL_E2E_INVENTORY 和 XRAYC_REAL_E2E_TARGET 指定目标。
# 输出：只打印 alias 和粗粒度结果，不打印主机、密码、token 或 URL。
# 依赖：使用 ssh、python3、安装脚本和远端 Docker Compose 能力。
# 安全：inventory 必须在私有路径，临时文件清理时避免删除既有安装目录。
# 约束：默认只清理本次 E2E 创建的远端资源，既有资源需显式允许清理。
# 行为：解析目标、上传安装载荷、执行服务器本地安装、探测远端状态并回滚清理。
# 失败：inventory 无效、远端命令失败、安装或健康检查失败会返回非零。
# 维护：inventory schema 或清理策略变化时同步解析和保护断言。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  XRAYC_REAL_E2E_INVENTORY=/path/to/private-inventory.json \
  XRAYC_REAL_E2E_TARGET=access-node-1 \
  bash scripts/real-remote-access-deploy-e2e.sh

Inventory shape:
{
  "targets": [
    {
      "alias": "access-node-1",
      "ssh_host": "example.test",
      "ssh_user": "root",
      "ssh_port": 22,
      "ssh_password_file": "/private/ignored/password-file",
      "ssh_identity_file": "/private/ignored/key",
      "control_plane_url": "https://panel.example.test",
      "deploy_artifact_token": "<private>",
      "install_dir": "/opt/xrayc/access-agent",
      "compose_project": "xrayc-access",
      "xray_api_server": "127.0.0.1:10085",
      "xray_api_listen_host": "127.0.0.1",
      "xray_api_listen_port": 10085,
      "expected_listen_ports": [443]
    }
  ]
}

The inventory must live in an ignored private path. This script prints only
aliases and coarse results; it never prints hosts, credentials, tokens, DB URLs,
subscription links, or proxy credentials.

Optional environment variables:
  XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP  Set to 1 only for private HTTP E2E; default: 0.
  XRAYC_REMOTE_E2E_AUTH_CODE_OUT        Optional private local file to receive the generated auth code.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
TARGET_ALIAS="${XRAYC_REAL_E2E_TARGET:-}"
RUN_ID="$(date +%s)-$$"
TMP_DIR="$(mktemp -d)"
REMOTE_TMP=""
DEPLOY_ATTEMPTED=0
INSTALL_DIR_PREEXISTING=""

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ -z "$INVENTORY" ]]; then
  echo "XRAYC_REAL_E2E_INVENTORY is required and must point to a private JSON file." >&2
  exit 2
fi
if [[ ! -f "$INVENTORY" ]]; then
  echo "private inventory file was not found." >&2
  exit 2
fi

inventory_env="${TMP_DIR}/inventory.env"
python3 - "$INVENTORY" "$TARGET_ALIAS" > "$inventory_env" <<'PY'
import json
import os
import shlex
import sys

path = sys.argv[1]
target_alias = sys.argv[2]

with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)

targets = payload.get("targets")
if not isinstance(targets, list) or not targets:
    raise SystemExit("inventory.targets must be a non-empty array")

target = None
if target_alias:
    for item in targets:
        if str(item.get("alias", "")) == target_alias:
            target = item
            break
    if target is None:
        raise SystemExit("requested target alias was not found")
else:
    target = targets[0]

target = dict(target)
target["control_plane_url"] = (
    str(target.get("control_plane_url", "")).strip()
    or os.environ.get("BASE_URL", "").strip()
    or os.environ.get("XRAYC_CONTROL_PLANE_URL", "").strip()
)
target["deploy_artifact_token"] = (
    str(target.get("deploy_artifact_token", "")).strip()
    or os.environ.get("DEPLOY_ARTIFACT_TOKEN", "").strip()
    or os.environ.get("XRAYC_DEPLOY_ARTIFACT_TOKEN", "").strip()
)

required = [
    "alias",
    "ssh_host",
    "ssh_user",
    "control_plane_url",
    "deploy_artifact_token",
]
for key in required:
    if not str(target.get(key, "")).strip():
        raise SystemExit(f"inventory target missing {key}")

def emit(name, value):
    print(f"{name}={shlex.quote(str(value))}")

emit("TARGET_ALIAS", target["alias"])
emit("SSH_HOST", target["ssh_host"])
emit("SSH_USER", target["ssh_user"])
emit("SSH_PORT", target.get("ssh_port", 22))
emit("SSH_PASSWORD_FILE", target.get("ssh_password_file", ""))
emit("SSH_IDENTITY_FILE", target.get("ssh_identity_file", ""))
emit("CONTROL_PLANE_URL", target["control_plane_url"])
emit("DEPLOY_ARTIFACT_TOKEN_VALUE", target["deploy_artifact_token"])
emit("NODE_ID", target.get("node_id", ""))
emit("NODE_TOKEN", target.get("node_token", ""))
emit("INSTALL_DIR", target.get("install_dir", "/opt/xrayc/access-agent"))
emit("COMPOSE_PROJECT", target.get("compose_project", "xrayc-access"))
emit("XRAY_API_SERVER", target.get("xray_api_server", ""))
emit("XRAY_API_LISTEN_HOST", target.get("xray_api_listen_host", ""))
emit("XRAY_API_LISTEN_PORT", target.get("xray_api_listen_port", ""))
emit("HEARTBEAT_INTERVAL_SECONDS", target.get("heartbeat_interval_seconds", ""))
emit("TRAFFIC_INTERVAL_SECONDS", target.get("traffic_interval_seconds", ""))
emit("SESSION_IDLE_SECONDS", target.get("session_idle_seconds", ""))
tls_domains = target.get("tls_cert_domains", [])
if isinstance(tls_domains, str):
    tls_domains = [item for item in tls_domains.split() if item]
if not isinstance(tls_domains, list):
    raise SystemExit("tls_cert_domains must be an array or a space-separated string")
emit("TLS_CERT_DOMAINS", " ".join(str(item).strip() for item in tls_domains if str(item).strip()))
emit("TLS_CERT_EMAIL", target.get("tls_cert_email", ""))
ports = target.get("expected_listen_ports", [])
if not isinstance(ports, list):
    raise SystemExit("expected_listen_ports must be an array")
emit("EXPECTED_LISTEN_PORTS", " ".join(str(int(port)) for port in ports))
PY

# shellcheck disable=SC1090
. "$inventory_env"
if [[ "${XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS:-0}" != "1" ]]; then
  NODE_ID=""
  NODE_TOKEN=""
fi
if [[ -n "${XRAYC_REMOTE_E2E_EXPECTED_LISTEN_PORTS_OVERRIDE+x}" ]]; then
  EXPECTED_LISTEN_PORTS="$XRAYC_REMOTE_E2E_EXPECTED_LISTEN_PORTS_OVERRIDE"
fi

ALLOW_INSECURE_HTTP="${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-0}"
if [[ "$ALLOW_INSECURE_HTTP" != "0" && "$ALLOW_INSECURE_HTTP" != "1" ]]; then
  echo "XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP must be unset, 0, or 1." >&2
  exit 2
fi
case "$CONTROL_PLANE_URL" in
  https://*)
    ;;
  http://*)
    if [[ "$ALLOW_INSECURE_HTTP" != "1" ]]; then
      echo "remote agent install e2e requires HTTPS control plane; set XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP=1 only for private HTTP E2E." >&2
      exit 2
    fi
    ;;
  *)
    echo "remote agent install e2e control plane URL must use HTTPS, or HTTP only with the explicit private E2E override." >&2
    exit 2
    ;;
esac

echo "remote agent install e2e target: ${TARGET_ALIAS}"

ssh_base=(
  ssh
  -p "$SSH_PORT"
  -o ConnectTimeout=15
  -o ServerAliveInterval=15
  -o StrictHostKeyChecking=accept-new
)
if [[ -n "$SSH_IDENTITY_FILE" ]]; then
  ssh_base+=(-o BatchMode=yes)
  ssh_base+=(-i "$SSH_IDENTITY_FILE")
fi
if [[ -n "$SSH_PASSWORD_FILE" ]]; then
  if ! command -v sshpass >/dev/null 2>&1; then
    echo "sshpass is required when ssh_password_file is configured." >&2
    exit 2
  fi
  ssh_base+=(-o BatchMode=no)
  ssh_base=(sshpass -f "$SSH_PASSWORD_FILE" "${ssh_base[@]}")
else
  ssh_base+=(-o BatchMode=yes)
fi
ssh_target="${SSH_USER}@${SSH_HOST}"

ssh_remote() {
  local err_file="${TMP_DIR}/ssh.err"
  if ! "${ssh_base[@]}" "$ssh_target" "$@" 2> "$err_file"; then
    echo "remote ssh command failed; stderr redacted to avoid leaking host or credentials." >&2
    return 1
  fi
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

cleanup_all() {
  local status=$?
  local cleanup_requested="${XRAYC_REMOTE_E2E_CLEANUP:-0}"
  local cleanup_existing="${XRAYC_REMOTE_E2E_CLEANUP_EXISTING_INSTALL_DIR:-0}"
  if [[ -n "${REMOTE_TMP:-}" ]]; then
    ssh_remote "rm -f $(shell_quote "${REMOTE_TMP}/node.env")" >/dev/null 2>&1 || true
  fi
  if [[ "$status" != "0" ]]; then
    if [[ "${XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE:-0}" == "1" ]]; then
      cleanup_requested="1"
    else
      cleanup_requested="0"
    fi
  fi
  if [[ "$cleanup_requested" == "1" && "$DEPLOY_ATTEMPTED" == "1" && -n "${INSTALL_DIR:-}" && -n "${COMPOSE_PROJECT:-}" ]]; then
    if [[ "${INSTALL_DIR_PREEXISTING:-1}" == "0" || "$cleanup_existing" == "1" ]]; then
      ssh_remote "install_dir=$(shell_quote "$INSTALL_DIR"); compose_project=$(shell_quote "$COMPOSE_PROJECT"); if [ -f \"\$install_dir/docker-compose.yml\" ]; then if docker version >/dev/null 2>&1; then dcmd='docker'; else dcmd='sudo -n docker'; fi; if \$dcmd compose version >/dev/null 2>&1; then \$dcmd compose -f \"\$install_dir/docker-compose.yml\" -p \"\$compose_project\" down --remove-orphans >/dev/null 2>&1 || true; elif command -v docker-compose >/dev/null 2>&1; then docker-compose -f \"\$install_dir/docker-compose.yml\" -p \"\$compose_project\" down --remove-orphans >/dev/null 2>&1 || true; else sudo -n docker-compose -f \"\$install_dir/docker-compose.yml\" -p \"\$compose_project\" down --remove-orphans >/dev/null 2>&1 || true; fi; fi; rm -rf \"\$install_dir\"" >/dev/null 2>&1 || true
    fi
  fi
  if [[ -n "${REMOTE_TMP:-}" ]]; then
    ssh_remote "rm -rf $(shell_quote "$REMOTE_TMP") $(shell_quote "/tmp/xrayc-agent-install-${RUN_ID}.log")" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup_all EXIT

echo "remote agent install e2e: local script syntax check"
bash -n scripts/deploy-access-agent.sh

if [[ "${XRAYC_REMOTE_E2E_CHECK_ARTIFACTS:-1}" == "1" ]]; then
  echo "remote agent install e2e: center artifact smoke"
  BASE_URL="$CONTROL_PLANE_URL" \
    ALLOW_INSECURE_HTTP_E2E="$ALLOW_INSECURE_HTTP" \
    DEPLOY_ARTIFACT_TOKEN="$DEPLOY_ARTIFACT_TOKEN_VALUE" \
    ACCESS_NODE_ID="" \
    AGENT_TOKEN="" \
    ACCESS_LINE_ID="" \
    EXIT_ENDPOINT_ID="" \
    SUB_TOKEN="" \
    SUBSCRIPTION_URL="" \
    SKIP_SUBSCRIPTION_DOWNLOAD=1 \
    DEPLOY_ARTIFACT_FULL="${XRAYC_REMOTE_E2E_ARTIFACT_FULL:-0}" \
    bash scripts/real-smoke.sh >/dev/null
  echo "remote agent install e2e: center artifact smoke passed"
fi

node_env="${TMP_DIR}/node.env"
write_remote_env_line() {
  local key="$1"
  local value="$2"
  printf '%s=%s\n' "$key" "$(shell_quote "$value")"
}
{
  write_remote_env_line XRAYC_CONTROL_PLANE_URL "$CONTROL_PLANE_URL"
  write_remote_env_line XRAYC_DEPLOY_ARTIFACT_TOKEN "$DEPLOY_ARTIFACT_TOKEN_VALUE"
  if [[ -n "$NODE_ID" ]]; then
    write_remote_env_line XRAYC_NODE_ID "$NODE_ID"
  fi
  if [[ -n "$NODE_TOKEN" ]]; then
    write_remote_env_line XRAYC_NODE_TOKEN "$NODE_TOKEN"
  fi
  write_remote_env_line XRAYC_INSTALL_DIR "$INSTALL_DIR"
  write_remote_env_line XRAYC_COMPOSE_PROJECT_NAME "$COMPOSE_PROJECT"
  if [[ -n "$XRAY_API_SERVER" ]]; then
    write_remote_env_line XRAYC_XRAY_API_SERVER "$XRAY_API_SERVER"
  fi
  if [[ -n "$XRAY_API_LISTEN_HOST" ]]; then
    write_remote_env_line XRAYC_XRAY_API_LISTEN_HOST "$XRAY_API_LISTEN_HOST"
  fi
  if [[ -n "$XRAY_API_LISTEN_PORT" ]]; then
    write_remote_env_line XRAYC_XRAY_API_LISTEN_PORT "$XRAY_API_LISTEN_PORT"
  fi
  if [[ -n "$HEARTBEAT_INTERVAL_SECONDS" ]]; then
    write_remote_env_line XRAYC_HEARTBEAT_INTERVAL_SECONDS "$HEARTBEAT_INTERVAL_SECONDS"
  fi
  if [[ -n "$TRAFFIC_INTERVAL_SECONDS" ]]; then
    write_remote_env_line XRAYC_TRAFFIC_INTERVAL_SECONDS "$TRAFFIC_INTERVAL_SECONDS"
  fi
  if [[ -n "$SESSION_IDLE_SECONDS" ]]; then
    write_remote_env_line XRAYC_SESSION_IDLE_SECONDS "$SESSION_IDLE_SECONDS"
  fi
  if [[ -n "$TLS_CERT_DOMAINS" ]]; then
    write_remote_env_line XRAYC_TLS_CERT_DOMAINS "$TLS_CERT_DOMAINS"
  fi
  if [[ -n "$TLS_CERT_EMAIL" ]]; then
    write_remote_env_line XRAYC_TLS_CERT_EMAIL "$TLS_CERT_EMAIL"
  fi
  write_remote_env_line XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS "${XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS:-false}"
  write_remote_env_line XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS "${XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS:-true}"
  write_remote_env_line XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG "${XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG:-${XRAYC_DEPLOY_OVERWRITE_XRAY_CONFIG:-true}}"
  write_remote_env_line XRAYC_DEPLOY_REUSE_EXISTING_IMAGES "${XRAYC_DEPLOY_REUSE_EXISTING_IMAGES:-false}"
  write_remote_env_line XRAYC_DEPLOY_START "true"
} > "$node_env"
chmod 600 "$node_env"

payload_dir="${TMP_DIR}/payload"
mkdir -p "$payload_dir"
cp scripts/deploy-access-agent.sh "$payload_dir/deploy-access-agent.sh"
mkdir -p "$payload_dir/lib"
cp -R scripts/lib/deploy-access-agent "$payload_dir/lib/deploy-access-agent"
cp "$node_env" "$payload_dir/node.env"
tarball="${TMP_DIR}/payload.tar.gz"
tar -C "$payload_dir" -czf "$tarball" .

remote_tmp="/tmp/xrayc-agent-install-${RUN_ID}"
REMOTE_TMP="$remote_tmp"
echo "remote agent install e2e: uploading install payload"
ssh_remote "umask 077; rm -rf '${remote_tmp}'; mkdir -p '${remote_tmp}'; tar -xzf - -C '${remote_tmp}'" < "$tarball"

echo "remote agent install e2e: running install script on server"
INSTALL_DIR_PREEXISTING="$(ssh_remote "if test -e $(shell_quote "$INSTALL_DIR"); then printf 1; else printf 0; fi")"
DEPLOY_ATTEMPTED=1
if ! ssh_remote "cd '${remote_tmp}' && set -a && . ./node.env && set +a && rm -f ./node.env && bash ./deploy-access-agent.sh >/tmp/xrayc-agent-install-${RUN_ID}.log 2>&1"; then
  echo "remote agent install e2e: install script failed; redacted install log tail follows" >&2
  ssh_remote "tail -80 /tmp/xrayc-agent-install-${RUN_ID}.log 2>/dev/null | sed -E 's#https?://[^[:space:]]+#<URL>#g; s#[0-9]{1,3}(\\.[0-9]{1,3}){3}#<IP>#g; s#xrayc-agent-v1:[^[:space:]]+#xrayc-agent-v1:<redacted>#g; s#(Bearer )[A-Za-z0-9._~+/-]+#\\1<redacted>#g; s#(token|password|secret|credential)[=:][^[:space:]]+#\\1=<redacted>#Ig'" >&2 || true
  exit 1
fi

remote_env_value() {
  local key="$1"
  ssh_remote "awk -v key='$key' 'index(\$0, key \"=\") == 1 { value = substr(\$0, length(key) + 2); if (value ~ /^\".*\"$/) { value = substr(value, 2, length(value) - 2); gsub(/\\\\\"/, \"\\\"\", value); gsub(/\\\\\\\\/, \"\\\\\", value) } print value; exit }' $(shell_quote "${INSTALL_DIR}/access-agent.env")"
}

INSTALLED_NODE_ID="$(remote_env_value XRAYC_NODE_ID)"
INSTALLED_NODE_TOKEN="$(remote_env_value XRAYC_NODE_TOKEN)"
if [[ -z "$INSTALLED_NODE_ID" || -z "$INSTALLED_NODE_TOKEN" ]]; then
  echo "remote agent install e2e: installed agent credentials were not persisted." >&2
  exit 1
fi
INSTALLED_AGENT_AUTH_CODE="xrayc-agent-v1:${INSTALLED_NODE_ID}:${INSTALLED_NODE_TOKEN}"
printf '%s\n' "$INSTALLED_AGENT_AUTH_CODE" > "${TMP_DIR}/installed-agent-auth-code"
chmod 600 "${TMP_DIR}/installed-agent-auth-code"
if [[ -n "${XRAYC_REMOTE_E2E_AUTH_CODE_OUT:-}" ]]; then
  install -m 0600 "${TMP_DIR}/installed-agent-auth-code" "$XRAYC_REMOTE_E2E_AUTH_CODE_OUT"
fi
echo "remote agent install e2e: installed agent credentials captured"

echo "remote agent install e2e: checking compose services"
container_suffix="$(printf '%s' "$INSTALLED_NODE_ID" | sha256sum | cut -c1-12)"
remote_xray_container="xrayc-xray-${container_suffix}"
remote_agent_container="xrayc-access-agent-${container_suffix}"
ssh_remote "if docker version >/dev/null 2>&1; then dcmd='docker'; else dcmd='sudo -n docker'; fi; deadline=\$((\$(date +%s)+${XRAYC_REMOTE_E2E_SERVICE_WAIT_SECONDS:-60})); until test \"\$(\$dcmd inspect -f '{{.State.Running}}' '${remote_xray_container}' 2>/dev/null)\" = true && test \"\$(\$dcmd inspect -f '{{.State.Running}}' '${remote_agent_container}' 2>/dev/null)\" = true; do if [ \$(date +%s) -ge \$deadline ]; then exit 1; fi; sleep 2; done"

echo "remote agent install e2e: waiting for heartbeat"
sleep "${XRAYC_REMOTE_E2E_HEARTBEAT_WAIT_SECONDS:-35}"

if [[ -n "$EXPECTED_LISTEN_PORTS" ]]; then
  echo "remote agent install e2e: checking remote state hash"
  ssh_remote "test -s '${INSTALL_DIR}/state/state.json' && grep -Eq '\"config_version\"[[:space:]]*:[[:space:]]*\"[^\"]+\"' '${INSTALL_DIR}/state/state.json'"
  echo "remote agent install e2e: checking expected listen ports"
  for port in $EXPECTED_LISTEN_PORTS; do
    ssh_remote "deadline=\$((\$(date +%s)+${XRAYC_REMOTE_E2E_PORT_WAIT_SECONDS:-30})); until ss -ltn | awk '{print \$4}' | grep -Eq '(^|:)${port}$'; do if [ \$(date +%s) -ge \$deadline ]; then exit 1; fi; sleep 2; done"
  done
else
  echo "remote agent install e2e: empty access node has no expected listen ports; skipping remote state hash check"
fi

echo "remote agent install e2e completed"
