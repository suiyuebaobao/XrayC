#!/usr/bin/env bash
# 此 helper 提供真实三节点完整性 E2E 的远端清单读取、SSH 建连和 relay 配置检查。
# 它由主脚本 source，不直接执行，并避免打印 SSH 主机、密码文件或远端敏感内容。

read_inventory_target() {
  local output_file="$1"
  [[ -n "$INVENTORY" ]] || die_usage "XRAYC_REAL_E2E_INVENTORY is required"
  [[ -f "$INVENTORY" ]] || die_usage "private inventory file was not found"
  [[ -n "$ACCESS_TARGET" ]] || die_usage "XRAYC_REAL_E2E_TARGET is required"

  python3 - "$INVENTORY" "$ACCESS_TARGET" >"$output_file" <<'PY'
import json
import shlex
import sys

path, alias = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = payload.get("targets")
if not isinstance(targets, list) or not targets:
    raise SystemExit("inventory.targets must be a non-empty array")
target = next((item for item in targets if str(item.get("alias", "")) == alias), None)
if target is None:
    raise SystemExit("requested target alias was not found")
for key in ("alias", "ssh_host", "ssh_user", "node_id"):
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
emit("REMOTE_NODE_ID", target["node_id"])
emit("REMOTE_INSTALL_DIR", target.get("install_dir", "/opt/xrayc/access-agent"))
emit("REMOTE_COMPOSE_PROJECT", target.get("compose_project", "xrayc-access"))
ports = target.get("expected_listen_ports", [])
if not isinstance(ports, list):
    raise SystemExit("expected_listen_ports must be an array")
emit("REMOTE_EXPECTED_LISTEN_PORTS", " ".join(str(int(port)) for port in ports))
PY
}

SSH_HOST=""
SSH_USER=""
SSH_PORT=""
SSH_PASSWORD_FILE=""
SSH_IDENTITY_FILE=""
REMOTE_NODE_ID=""
REMOTE_INSTALL_DIR=""
REMOTE_COMPOSE_PROJECT=""
REMOTE_EXPECTED_LISTEN_PORTS=""
ssh_base=()
ssh_target=""

setup_ssh() {
  local target_env="$tmp_dir/remote-target.env"
  if [[ -n "$INVENTORY" ]]; then
    read_inventory_target "$target_env"
    # shellcheck disable=SC1090
    . "$target_env"
  else
    SSH_HOST="$DIRECT_ACCESS_SSH_HOST"
    SSH_USER="$DIRECT_ACCESS_SSH_USER"
    SSH_PORT="$DIRECT_ACCESS_SSH_PORT"
    SSH_PASSWORD_FILE="$DIRECT_ACCESS_SSH_PASSWORD_FILE"
    SSH_IDENTITY_FILE="$DIRECT_ACCESS_SSH_IDENTITY_FILE"
    REMOTE_NODE_ID="$TARGET_ACCESS_NODE_ID"
    REMOTE_INSTALL_DIR="$DIRECT_ACCESS_INSTALL_DIR"
    REMOTE_COMPOSE_PROJECT="$DIRECT_ACCESS_COMPOSE_PROJECT"
    REMOTE_EXPECTED_LISTEN_PORTS="$DIRECT_ACCESS_EXPECTED_LISTEN_PORTS"
  fi
  require_real_value SSH_HOST "$SSH_HOST"
  require_real_value SSH_USER "$SSH_USER"
  require_real_value REMOTE_NODE_ID "$REMOTE_NODE_ID"
  SSH_HOST="$SSH_HOST" python3 - <<'PY'
import os

host = os.environ["SSH_HOST"].strip().strip("[]").lower()
allow_test = os.environ.get("XRAYC_REAL_3NODE_ALLOW_TEST_DOMAINS") == "1"
if host in {"example.com", "example.org", "example.net", "example.test"} or (
    host.endswith(".example") or (host.endswith(".test") and not allow_test)
):
    raise SystemExit("inventory SSH host looks like documentation or test data")
PY
  [[ "$REMOTE_NODE_ID" == "$TARGET_ACCESS_NODE_ID" ]] || fail "inventory node_id does not match database access_node_id"

  ssh_base=(ssh -p "$SSH_PORT" -o ConnectTimeout=15 -o ServerAliveInterval=15 -o StrictHostKeyChecking=accept-new)
  if [[ -n "$SSH_IDENTITY_FILE" ]]; then
    require_real_value SSH_IDENTITY_FILE "$SSH_IDENTITY_FILE"
    ssh_base+=(-o BatchMode=yes -i "$SSH_IDENTITY_FILE")
  fi
  if [[ -n "$SSH_PASSWORD_FILE" ]]; then
    require_real_value SSH_PASSWORD_FILE "$SSH_PASSWORD_FILE"
    [[ -f "$SSH_PASSWORD_FILE" ]] || die_usage "ssh password file was not found"
    require_command sshpass
    ssh_base+=(-o BatchMode=no)
    ssh_base=(sshpass -f "$SSH_PASSWORD_FILE" "${ssh_base[@]}")
  else
    ssh_base+=(-o BatchMode=yes)
  fi
  ssh_target="${SSH_USER}@${SSH_HOST}"
}

ssh_remote() {
  local err_file="$tmp_dir/ssh.err"
  if ! "${ssh_base[@]}" "$ssh_target" "$@" 2>"$err_file"; then
    return 1
  fi
}

check_remote_docker_deployment() {
  local suffix
  local xray_container
  local agent_container
  suffix="$(printf '%s' "$REMOTE_NODE_ID" | sha256sum | cut -c1-12)"
  xray_container="xrayc-xray-${suffix}"
  agent_container="xrayc-access-agent-${suffix}"

  echo "Checking relay Docker deployment"
  ssh_remote "if docker version >/dev/null 2>&1; then dcmd='docker'; else dcmd='sudo -n docker'; fi; test \"\$(\$dcmd inspect -f '{{.State.Running}}' $(shell_quote "$xray_container") 2>/dev/null)\" = true && test \"\$(\$dcmd inspect -f '{{.State.Running}}' $(shell_quote "$agent_container") 2>/dev/null)\" = true" \
    || fail "relay Docker containers are not running; remote stderr redacted"

  ssh_remote "test -s $(shell_quote "${REMOTE_INSTALL_DIR}/state/state.json") && grep -Eq '\"config_version\"[[:space:]]*:[[:space:]]*\"[^\"]+\"' $(shell_quote "${REMOTE_INSTALL_DIR}/state/state.json")" \
    || fail "relay access-agent state is not persisted; remote stderr redacted"

  if [[ -n "$REMOTE_EXPECTED_LISTEN_PORTS" ]]; then
    local port=""
    for port in $REMOTE_EXPECTED_LISTEN_PORTS; do
      ssh_remote "ss -ltn | awk '{print \$4}' | grep -Eq '(^|:)${port}$'" \
        || fail "relay expected listen port is not active; remote stderr redacted"
    done
  fi
  echo "relay Docker deployment check passed"
}

remote_state_line_user_present() {
  local command
  command="python3 - $(shell_quote "${REMOTE_INSTALL_DIR}/state/state.json") $(shell_quote "$TARGET_ACCESS_LINE_ID") $(shell_quote "$XRAY_USER_KEY") <<'PY'
import json
import sys

path, line_id, user_key = sys.argv[1:4]
with open(path, 'r', encoding='utf-8') as fh:
    payload = json.load(fh)
config = payload.get('config') if isinstance(payload, dict) else None
if not isinstance(config, dict):
    raise SystemExit(1)
for line in config.get('access_lines') or []:
    if str(line.get('id')) != line_id:
        continue
    for user in line.get('users') or []:
        if str(user.get('xray_user_key')) == user_key:
            raise SystemExit(0)
    raise SystemExit(1)
raise SystemExit(1)
PY"
  ssh_remote "$command"
}

remote_state_line_user_absent() {
  local command
  command="python3 - $(shell_quote "${REMOTE_INSTALL_DIR}/state/state.json") $(shell_quote "$TARGET_ACCESS_LINE_ID") $(shell_quote "$XRAY_USER_KEY") <<'PY'
import json
import sys

path, line_id, user_key = sys.argv[1:4]
try:
    with open(path, 'r', encoding='utf-8') as fh:
        payload = json.load(fh)
except FileNotFoundError:
    raise SystemExit(0)
config = payload.get('config') if isinstance(payload, dict) else None
if not isinstance(config, dict):
    raise SystemExit(0)
for line in config.get('access_lines') or []:
    if str(line.get('id')) != line_id:
        continue
    for user in line.get('users') or []:
        if str(user.get('xray_user_key')) == user_key:
            raise SystemExit(1)
    raise SystemExit(0)
raise SystemExit(0)
PY"
  ssh_remote "$command"
}

remote_user_present() {
  remote_state_line_user_present \
    && ssh_remote "grep -Fq -- $(shell_quote "$TARGET_STATS_EMAIL") $(shell_quote "${REMOTE_INSTALL_DIR}/xray/config.json")"
}

remote_user_absent() {
  remote_state_line_user_absent \
    && ssh_remote "! grep -Fq -- $(shell_quote "$TARGET_STATS_EMAIL") $(shell_quote "${REMOTE_INSTALL_DIR}/xray/config.json")"
}

wait_remote_user_present() {
  local elapsed=0
  echo "Waiting for relay config to include restored user"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    if remote_user_present; then
      echo "relay config includes restored user"
      return
    fi
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "relay config did not include restored user before timeout"
}

wait_remote_user_absent() {
  local reason="$1"
  local elapsed=0
  echo "Waiting for relay config eviction (${reason})"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    if remote_user_absent; then
      echo "relay config eviction observed (${reason})"
      return
    fi
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "relay config still contains target user after timeout (${reason})"
}

run_remote_deploy_if_requested() {
  if [[ "$RUN_REMOTE_DEPLOY" != "1" ]]; then
    return
  fi
  [[ -n "$INVENTORY" ]] || die_usage "XRAYC_REAL_3NODE_RUN_DEPLOY=1 requires XRAYC_REAL_E2E_INVENTORY"
  echo "Running relay remote deploy verification"
  XRAYC_REAL_E2E_INVENTORY="$INVENTORY" \
  XRAYC_REAL_E2E_TARGET="$ACCESS_TARGET" \
  XRAYC_REMOTE_E2E_CHECK_ARTIFACTS="${XRAYC_REMOTE_E2E_CHECK_ARTIFACTS:-1}" \
  XRAYC_REMOTE_E2E_ARTIFACT_FULL="${XRAYC_REMOTE_E2E_ARTIFACT_FULL:-1}" \
  XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE="${XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE:-1}" \
  bash scripts/real-remote-access-deploy-e2e.sh
}
