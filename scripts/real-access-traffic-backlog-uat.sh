#!/usr/bin/env bash
# 用途：执行真实 access-agent 流量积压 UAT，验证断连后的本地积压能力。
# 范围：阻断远端 agent 到控制面链路，产生真实客户端流量并恢复同步。
# 输入：需要控制面、数据库、客户端代理、节点线路 ID 和真实 inventory。
# 输出：仅输出阶段结果和断言状态，不打印 DB、token、代理或真实 IP。
# 依赖：source real-e2e-lib.sh，并使用 SSH、curl、psql 与远端 Docker。
# 安全：阻断和恢复操作必须限定在 inventory 指定的私有测试目标。
# 约束：默认会重启 agent 验证积压持久化，可用变量显式跳过。
# 行为：检查 backlog 文件生成、重启后保留、恢复后数据库账本增长。
# 失败：环境缺失、远端阻断恢复失败或账本未增长时返回非零。
# 维护：backlog 文件路径或账本字段变化时同步远端断言和 SQL。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

usage() {
  cat <<'USAGE'
usage: bash scripts/real-access-traffic-backlog-uat.sh

Real UAT for access-agent traffic backlog:
  1. Blocks the remote agent container from reaching the control plane.
  2. Generates real client traffic through CLIENT_PROXY_URL.
  3. Verifies traffic-backlog.json is written and survives agent restart.
  4. Restores control-plane connectivity and verifies DB ledger growth.

Required private env:
  BASE_URL, DATABASE_URL, CLIENT_PROXY_URL, ACCESS_NODE_ID, ACCESS_LINE_ID,
  SUB_TOKEN or SUBSCRIPTION_URL, XRAYC_REAL_E2E_INVENTORY.

Optional:
  BACKLOG_TARGET_INDEX=2              Inventory target index for the relay.
  BACKLOG_INSTALL_DIR=/opt/xrayc-real-relay
  BACKLOG_VALIDATE_ONLY=1             Validate env and remote access only.
  BACKLOG_SKIP_AGENT_RESTART=1        Do not restart the agent container.
  BACKLOG_WAIT_SECONDS=180            Wait timeout for each async phase.
  BACKLOG_TRAFFIC_ROUNDS=2            Number of client traffic requests.
  XRAY_USER_KEY=...                    Optional fallback expected user key.

The script never prints real IPs, passwords, proxy URLs, tokens, or DB URLs.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

BASE_URL="${BASE_URL:-}"
DATABASE_URL="${DATABASE_URL:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
SUB_TOKEN="${SUB_TOKEN:-}"
SUBSCRIPTION_URL="${SUBSCRIPTION_URL:-}"
ACCESS_NODE_ID="${ACCESS_NODE_ID:-}"
ACCESS_LINE_ID="${ACCESS_LINE_ID:-}"
XRAY_USER_KEY="${XRAY_USER_KEY:-}"
INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
TRAFFIC_URL="${UAT_CLIENT_TRAFFIC_URL:-https://speed.cloudflare.com/__down?bytes=1048576}"
BACKLOG_TARGET_INDEX="${BACKLOG_TARGET_INDEX:-2}"
BACKLOG_INSTALL_DIR="${BACKLOG_INSTALL_DIR:-/opt/xrayc-real-relay}"
BACKLOG_WAIT_SECONDS="${BACKLOG_WAIT_SECONDS:-180}"
BACKLOG_TRAFFIC_ROUNDS="${BACKLOG_TRAFFIC_ROUNDS:-2}"
BACKLOG_SKIP_AGENT_RESTART="${BACKLOG_SKIP_AGENT_RESTART:-0}"

xrayc_real_e2e_require_url_scheme BASE_URL "traffic backlog UAT"
xrayc_real_e2e_require_url_scheme DATABASE_URL "traffic backlog UAT"
xrayc_real_e2e_require_url_scheme CLIENT_PROXY_URL "traffic backlog UAT"
xrayc_real_e2e_require_url_scheme PUBLIC_IP_URL "traffic backlog UAT"
xrayc_real_e2e_require_url_scheme TRAFFIC_URL "traffic backlog UAT"
xrayc_real_e2e_require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" "traffic backlog UAT" SUB_TOKEN SUBSCRIPTION_URL
xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
xrayc_real_e2e_require_env ACCESS_NODE_ID "traffic backlog UAT"
xrayc_real_e2e_require_env ACCESS_LINE_ID "traffic backlog UAT"
xrayc_real_e2e_require_env INVENTORY "traffic backlog UAT"

for name in BACKLOG_TARGET_INDEX BACKLOG_WAIT_SECONDS BACKLOG_TRAFFIC_ROUNDS; do
  if [[ ! "${!name}" =~ ^[1-9][0-9]*$ ]]; then
    echo "${name} must be a positive integer" >&2
    exit 2
  fi
done
if [[ ! -f "$INVENTORY" ]]; then
  echo "traffic backlog UAT inventory file is missing" >&2
  exit 2
fi
for command_name in python3 ssh curl psql sha256sum; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "${command_name} is required for traffic backlog UAT" >&2
    exit 2
  }
done

TMP_DIR="$(mktemp -d)"
TARGET_ENV="${TMP_DIR}/target.env"
KNOWN_HOSTS="${TMP_DIR}/known_hosts"
EXPECTED_USER_KEYS="${TMP_DIR}/expected-user-keys"
BLOCKED=0
SUBSCRIPTION_TOKEN=""
SUBSCRIPTION_TOKEN_HASH=""

cleanup() {
  local status=$?
  if [[ "$BLOCKED" == "1" ]]; then
    unblock_control_plane >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

shell_quote() {
  printf '%q' "$1"
}

python3 - "$INVENTORY" "$BACKLOG_TARGET_INDEX" "$TARGET_ENV" "$TMP_DIR" <<'PY'
import json
import os
import stat
import sys

inventory_path, target_index, output_path, tmp_dir = sys.argv[1:5]
with open(inventory_path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = payload.get("targets") or payload.get("servers") or []
index = int(target_index) - 1
if index < 0 or index >= len(targets):
    raise SystemExit("inventory target index is out of range")
target = targets[index]
host = target.get("ssh_host") or target.get("host")
user = target.get("ssh_user") or target.get("username") or "root"
port = str(target.get("ssh_port") or target.get("port") or 22)
password_file = target.get("ssh_password_file") or target.get("password_file")
identity_file = target.get("ssh_identity_file") or target.get("identity_file")
password = target.get("ssh_password") or target.get("password")
if not host:
    raise SystemExit("inventory target is missing host")
if not password_file and password:
    password_file = os.path.join(tmp_dir, "target.pass")
    with open(password_file, "w", encoding="utf-8") as fh:
        fh.write(password)
    os.chmod(password_file, stat.S_IRUSR | stat.S_IWUSR)
if not password_file and not identity_file:
    raise SystemExit("inventory target is missing SSH auth")

def emit(key: str, value: str) -> str:
    value = str(value or "").replace("'", "'\"'\"'")
    return f"{key}='{value}'"

lines = [
    emit("SSH_HOST", host),
    emit("SSH_USER", user),
    emit("SSH_PORT", port),
    emit("SSH_PASSWORD_FILE", password_file or ""),
    emit("SSH_IDENTITY_FILE", identity_file or ""),
]
with open(output_path, "w", encoding="utf-8") as fh:
    fh.write("\n".join(lines) + "\n")
os.chmod(output_path, stat.S_IRUSR | stat.S_IWUSR)
PY

# shellcheck disable=SC1090
. "$TARGET_ENV"

build_ssh_command() {
  SSH_CMD=(ssh -o BatchMode=no -o LogLevel=ERROR -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$KNOWN_HOSTS" -p "$SSH_PORT")
  if [[ -n "${SSH_IDENTITY_FILE:-}" ]]; then
    SSH_CMD+=(-i "$SSH_IDENTITY_FILE")
  fi
  SSH_CMD+=("${SSH_USER}@${SSH_HOST}")
  if [[ -n "${SSH_PASSWORD_FILE:-}" ]]; then
    command -v sshpass >/dev/null 2>&1 || {
      echo "sshpass is required for password based SSH" >&2
      exit 2
    }
    SSH_CMD=(sshpass -f "$SSH_PASSWORD_FILE" "${SSH_CMD[@]}")
  fi
}

remote_exec() {
  local command="$1"
  "${SSH_CMD[@]}" "$command"
}

psql_scalar() {
  local sql="$1"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 -c "$sql" | tr -d '\r'
}

extract_subscription_token() {
  if [[ -n "$SUB_TOKEN" ]]; then
    printf '%s' "$SUB_TOKEN"
    return
  fi
  SUBSCRIPTION_URL_VALUE="$SUBSCRIPTION_URL" python3 - <<'PY'
from urllib.parse import urlparse
import os

url = os.environ["SUBSCRIPTION_URL_VALUE"].strip()
path = urlparse(url).path if "://" in url else url
parts = [part for part in path.split("/") if part]
if "sub" in parts:
    index = parts.index("sub")
    print(parts[index + 1] if index + 1 < len(parts) else "")
elif parts:
    print(parts[-1])
PY
}

resolve_expected_user_keys() {
  local keys key_count
  keys="$(xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
    -v "access_line_id=$ACCESS_LINE_ID" \
    -v "subscription_token=$SUBSCRIPTION_TOKEN" \
    -v "subscription_token_hash=$SUBSCRIPTION_TOKEN_HASH" <<'SQL'
WITH selected_line AS (
  SELECT id, exit_pool_id
  FROM access_lines
  WHERE id = :'access_line_id'::uuid
),
token_user AS (
  SELECT user_id
  FROM subscription_tokens
  WHERE (token_hash = :'subscription_token_hash' OR token = :'subscription_token')
    AND revoked_at IS NULL
    AND (expires_at IS NULL OR expires_at > now())
  ORDER BY created_at DESC
  LIMIT 1
),
subscription_user AS (
  SELECT u.id, u.xray_user_key
  FROM token_user token
  JOIN users u ON u.id = token.user_id
  JOIN user_subscriptions subscription ON subscription.user_id = u.id
  WHERE u.disabled = FALSE
    AND subscription.active = TRUE
    AND (subscription.expires_at IS NULL OR subscription.expires_at > now())
    AND COALESCE(u.xray_user_key, '') <> ''
),
assigned_user AS (
  SELECT DISTINCT user_row.xray_user_key
  FROM subscription_user user_row
  JOIN user_access_line_assignments line_assignment
    ON line_assignment.user_id = user_row.id
  JOIN selected_line line
    ON line.id = line_assignment.access_line_id
  JOIN user_exit_assignments exit_assignment
    ON exit_assignment.user_id = user_row.id
   AND exit_assignment.access_line_id = line.id
   AND exit_assignment.exit_pool_id = line.exit_pool_id
)
SELECT xray_user_key
FROM assigned_user
LIMIT 2;
SQL
)"
  printf '%s\n' "$keys" | sed '/^[[:space:]]*$/d' > "$EXPECTED_USER_KEYS"
  key_count="$(wc -l < "$EXPECTED_USER_KEYS" | tr -d ' ')"
  if [[ "$key_count" != "1" ]]; then
    echo "traffic backlog UAT cannot resolve one active assigned subscription user key" >&2
    exit 2
  fi
}

build_ssh_command
SUBSCRIPTION_TOKEN="$(extract_subscription_token)"
if [[ -z "$SUBSCRIPTION_TOKEN" ]]; then
  echo "traffic backlog UAT cannot extract subscription token" >&2
  exit 2
fi
SUBSCRIPTION_TOKEN_HASH="$(printf '%s' "$SUBSCRIPTION_TOKEN" | sha256sum | awk '{print $1}')"
resolve_expected_user_keys

container_suffix="$(printf '%s' "$ACCESS_NODE_ID" | sha256sum | cut -c1-12)"
agent_container="xrayc-access-agent-${container_suffix}"
backlog_path="${BACKLOG_INSTALL_DIR}/state/traffic-backlog.json"
state_path="${BACKLOG_INSTALL_DIR}/state/state.json"
agent_env_path="${BACKLOG_INSTALL_DIR}/access-agent.env"

read_remote_control_plane_url() {
  local quoted_env
  quoted_env="$(shell_quote "$agent_env_path")"
  remote_exec "set -eu; test -s ${quoted_env}; set -a; . ${quoted_env}; set +a; printf '%s' \"\${XRAYC_CONTROL_PLANE_URL}\""
}

control_plane_url="$(read_remote_control_plane_url)"
control_plane_host_port="$(python3 - "$control_plane_url" <<'PY'
from urllib.parse import urlparse
import sys

parsed = urlparse(sys.argv[1])
if not parsed.scheme or not parsed.hostname:
    raise SystemExit(2)
port = parsed.port or (443 if parsed.scheme == "https" else 80)
print(f"{parsed.hostname} {port}")
PY
)"
control_plane_host="${control_plane_host_port% *}"
control_plane_port="${control_plane_host_port##* }"

remote_probe() {
  local quoted_container quoted_backlog quoted_state
  quoted_container="$(shell_quote "$agent_container")"
  quoted_backlog="$(shell_quote "$backlog_path")"
  quoted_state="$(shell_quote "$state_path")"
  remote_exec "set -eu; docker inspect ${quoted_container} >/dev/null; test -s ${quoted_state}; if test -e ${quoted_backlog}; then test -f ${quoted_backlog}; fi"
}

block_control_plane() {
  local quoted_host quoted_port
  quoted_host="$(shell_quote "$control_plane_host")"
  quoted_port="$(shell_quote "$control_plane_port")"
  remote_exec "set -eu
host=${quoted_host}
port=${quoted_port}
ip=\$(getent ahostsv4 \"\$host\" | awk '{print \$1; exit}')
test -n \"\$ip\"
chain=OUTPUT
if ! iptables -C \"\$chain\" -p tcp -d \"\$ip\" --dport \"\$port\" -m comment --comment xrayc-backlog-uat -j REJECT >/dev/null 2>&1; then
  iptables -I \"\$chain\" 1 -p tcp -d \"\$ip\" --dport \"\$port\" -m comment --comment xrayc-backlog-uat -j REJECT
fi"
  BLOCKED=1
}

unblock_control_plane() {
  local quoted_host quoted_port
  quoted_host="$(shell_quote "$control_plane_host")"
  quoted_port="$(shell_quote "$control_plane_port")"
  remote_exec "set -eu
host=${quoted_host}
port=${quoted_port}
ip=\$(getent ahostsv4 \"\$host\" | awk '{print \$1; exit}' || true)
test -n \"\$ip\" || exit 0
while iptables -C OUTPUT -p tcp -d \"\$ip\" --dport \"\$port\" -m comment --comment xrayc-backlog-uat -j REJECT >/dev/null 2>&1; do
  iptables -D OUTPUT -p tcp -d \"\$ip\" --dport \"\$port\" -m comment --comment xrayc-backlog-uat -j REJECT
done"
  BLOCKED=0
}

generate_client_traffic() {
  local round
  for ((round = 1; round <= BACKLOG_TRAFFIC_ROUNDS; round++)); do
    xrayc_real_e2e_fetch_url_via_proxy_to_file "$CLIENT_PROXY_URL" "$TRAFFIC_URL" "${TMP_DIR}/traffic-${round}.bin" "traffic backlog UAT client traffic failed"
  done
}

wait_remote_backlog_present() {
  local elapsed=0 quoted_backlog quoted_line user_match_command key quoted_user
  quoted_backlog="$(shell_quote "$backlog_path")"
  quoted_line="$(shell_quote "$ACCESS_LINE_ID")"
  user_match_command="false"
  while IFS= read -r key; do
    [[ -n "$key" ]] || continue
    quoted_user="$(shell_quote "$key")"
    user_match_command="${user_match_command} || grep -Fq -- ${quoted_user} ${quoted_backlog}"
  done < "$EXPECTED_USER_KEYS"
  while [[ "$elapsed" -le "$BACKLOG_WAIT_SECONDS" ]]; do
    if remote_exec "test -s ${quoted_backlog} && grep -Fq -- ${quoted_line} ${quoted_backlog} && { ${user_match_command}; }" >/dev/null 2>&1; then
      echo "traffic backlog file persisted"
      return
    fi
    sleep 5
    elapsed=$((elapsed + 5))
  done
  echo "traffic backlog file was not persisted before timeout" >&2
  exit 1
}

wait_remote_backlog_cleared() {
  local elapsed=0 quoted_backlog
  quoted_backlog="$(shell_quote "$backlog_path")"
  while [[ "$elapsed" -le "$BACKLOG_WAIT_SECONDS" ]]; do
    if remote_exec "test ! -e ${quoted_backlog}" >/dev/null 2>&1; then
      echo "traffic backlog file cleared"
      return
    fi
    sleep 5
    elapsed=$((elapsed + 5))
  done
  echo "traffic backlog file was not cleared before timeout" >&2
  exit 1
}

restart_agent_container() {
  if xrayc_real_e2e_bool_is_true "$BACKLOG_SKIP_AGENT_RESTART"; then
    echo "agent restart skipped by BACKLOG_SKIP_AGENT_RESTART"
    return
  fi
  local quoted_container
  quoted_container="$(shell_quote "$agent_container")"
  remote_exec "docker restart ${quoted_container} >/dev/null"
  echo "agent container restarted"
}

ledger_baseline() {
  local expected_user_key
  expected_user_key="$(head -n 1 "$EXPECTED_USER_KEYS")"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "access_line_id=$ACCESS_LINE_ID" \
    -v "xray_user_key=$expected_user_key" <<'SQL'
SELECT COALESCE(SUM(billed_bytes), 0)::bigint
FROM usage_ledgers
WHERE traffic_source = 'access_line'
  AND access_line_id = :'access_line_id'::uuid
  AND xray_user_key = :'xray_user_key';
SQL
}

wait_ledger_increase() {
  local before="$1"
  local elapsed=0 current=""
  while [[ "$elapsed" -le "$BACKLOG_WAIT_SECONDS" ]]; do
    current="$(ledger_baseline)"
    if [[ "$current" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ && "$current" -gt "$before" ]]; then
      echo "traffic backlog ledger replay observed"
      return
    fi
    sleep 5
    elapsed=$((elapsed + 5))
  done
  echo "traffic backlog replay did not increase ledger before timeout" >&2
  exit 1
}

remote_probe
if xrayc_real_e2e_bool_is_true "${BACKLOG_VALIDATE_ONLY:-0}"; then
  echo "traffic backlog UAT validation passed"
  exit 0
fi

before_ledger="$(ledger_baseline)"
echo "blocking control-plane traffic for access-agent"
block_control_plane
generate_client_traffic
wait_remote_backlog_present
restart_agent_container
wait_remote_backlog_present
echo "restoring control-plane traffic for access-agent"
unblock_control_plane
wait_remote_backlog_cleared
wait_ledger_increase "$before_ledger"
echo "traffic backlog UAT completed"
