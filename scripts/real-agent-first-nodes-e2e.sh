#!/usr/bin/env bash
# 用途：验证真实测试服务器的 agent 先安装再平台新增节点流程。
# 范围：读取私有 inventory，逐台远端安装 access-agent，然后调用管理端创建中转节点。
# 输入：默认加载 .env.real-release，可通过 XRAYC_REAL_RELEASE_ENV_FILE 覆盖。
# 输出：只打印 alias 和粗粒度状态，不打印主机、密码、token、鉴权码或 URL。
# 依赖：real-remote-access-deploy-e2e.sh、curl、python3、ssh/sshpass 和私有测试清单。
# 安全：生成的节点鉴权码只写入本机临时 0600 文件，脚本退出即删除。
# 约束：默认使用 inventory 中的当前测试服务器数量，至少 3 台；平台新增前会按鉴权码内 node_id 尝试删除旧测试节点。
# 行为：安装成功后创建平台节点，等待 access-agent 心跳变为非离线状态。
# 失败：任一服务器安装、平台新增、心跳轮询失败都会返回非零。
# 维护：新增输出字段时必须保持脱敏，不得打印真实资产字段。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  set -a; . "$REAL_RELEASE_ENV_FILE"; set +a
fi

BASE_URL="${BASE_URL:-}"
INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-admin}}}"
ADMIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-${XRAYC_REAL_E2E_ADMIN_PASSWORD:-}}}"
ALLOW_INSECURE_HTTP="${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-1}"
HEARTBEAT_WAIT_SECONDS="${XRAYC_AGENT_FIRST_HEARTBEAT_WAIT_SECONDS:-120}"
HEARTBEAT_POLL_SECONDS="${XRAYC_AGENT_FIRST_HEARTBEAT_POLL_SECONDS:-5}"
PUBLIC_PORT="${XRAYC_AGENT_FIRST_PUBLIC_PORT:-443}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

require_value() {
  local name="$1"
  local value="$2"
  if [[ -z "$value" ]]; then
    echo "${name} is required for real agent-first nodes E2E." >&2
    exit 2
  fi
}

require_value BASE_URL "$BASE_URL"
require_value XRAYC_REAL_E2E_INVENTORY "$INVENTORY"
require_value ADMIN_LOGIN_PASSWORD "$ADMIN_PASSWORD"
if [[ ! -f "$INVENTORY" ]]; then
  echo "private inventory file was not found." >&2
  exit 2
fi

login_response="${TMP_DIR}/admin-login.json"
login_status="${TMP_DIR}/admin-login.status"
ADMIN_ACCOUNT_JSON="$ADMIN_ACCOUNT" ADMIN_PASSWORD_JSON="$ADMIN_PASSWORD" \
python3 - > "${TMP_DIR}/admin-login-body.json" <<'PY'
import json
import os
print(json.dumps({"account": os.environ["ADMIN_ACCOUNT_JSON"], "password": os.environ["ADMIN_PASSWORD_JSON"]}, ensure_ascii=False))
PY
if ! curl --silent --show-error --location --max-time 20 \
  --header "Content-Type: application/json" \
  --output "$login_response" \
  --write-out "%{http_code}" \
  --data-binary "@${TMP_DIR}/admin-login-body.json" \
  "${BASE_URL%/}/api/auth/login" > "$login_status" 2> "${TMP_DIR}/curl-login.err"; then
  echo "real agent-first e2e: admin login request failed; stderr redacted." >&2
  exit 1
fi
if [[ "$(cat "$login_status")" != "200" ]]; then
  echo "real agent-first e2e: admin login failed." >&2
  exit 1
fi
ADMIN_TOKEN="$(python3 - "$login_response" <<'PY'
import json
import sys
data=json.load(open(sys.argv[1], encoding="utf-8"))
print(data.get("data", {}).get("access_token", ""))
PY
)"
require_value ADMIN_ACCESS_TOKEN "$ADMIN_TOKEN"

targets_env="${TMP_DIR}/targets.env"
python3 - "$INVENTORY" "${XRAYC_REAL_TEST_SERVER_COUNT:-}" > "$targets_env" <<'PY'
import json
import shlex
import sys
data=json.load(open(sys.argv[1], encoding="utf-8"))
requested_raw=sys.argv[2].strip()
targets=data.get("targets", [])
requested=int(requested_raw) if requested_raw else len(targets)
if requested < 3:
    raise SystemExit("inventory must contain at least 3 targets")
if len(targets) != requested:
    raise SystemExit("inventory target count must match requested node count")
hosts=[str(target.get("ssh_host", "")).strip() for target in targets[:requested]]
if len(set(hosts)) != requested:
    raise SystemExit("inventory targets must be distinct hosts")
print(f"TARGET_COUNT={requested}")
for index, target in enumerate(targets[:requested], 1):
    alias=str(target.get("alias") or f"server_{index}").strip()
    if alias != f"server_{index}":
        raise SystemExit("inventory aliases must be sequential server_N labels")
    public_host=""
    for key in ("public_domain", "domain", "tls_domain", "sni", "public_host"):
        value=str(target.get(key) or "").strip()
        if value:
            public_host=value
            break
    if not public_host:
        tls_domains=target.get("tls_cert_domains") or []
        if isinstance(tls_domains, list):
            for value in tls_domains:
                value=str(value or "").strip()
                if value:
                    public_host=value
                    break
    if not public_host:
        public_host=str(target.get("ssh_host") or "").strip()
    if not public_host:
        raise SystemExit(f"target {index} missing public host fallback")
    print(f"TARGET_{index}_ALIAS={shlex.quote(alias)}")
    print(f"TARGET_{index}_PUBLIC_HOST={shlex.quote(public_host)}")
PY
# shellcheck disable=SC1090
. "$targets_env"

curl_admin() {
  local method="$1"
  local path="$2"
  local body_file="${3:-}"
  local output="$4"
  local status_file="$5"
  local args=(
    --silent --show-error --location --max-time 30
    --header "Authorization: Bearer ${ADMIN_TOKEN}"
    --output "$output"
    --write-out "%{http_code}"
  )
  if [[ -n "$body_file" ]]; then
    args+=(--header "Content-Type: application/json" --data-binary "@${body_file}")
  fi
  if ! curl "${args[@]}" -X "$method" "${BASE_URL%/}${path}" > "$status_file" 2> "${TMP_DIR}/curl-admin.err"; then
    echo "real agent-first e2e: admin API request failed; stderr redacted." >&2
    return 1
  fi
}

delete_existing_alias_nodes() {
  local alias="$1"
  local node_id="$2"
  local routing_file="${TMP_DIR}/routing-delete-${alias}.json"
  local routing_status="${TMP_DIR}/routing-delete-${alias}.status"
  local delete_body="${TMP_DIR}/delete-${alias}.json"
  local delete_status="${TMP_DIR}/delete-${alias}.status"

  curl_admin GET "/api/admin/access-routing" "" "$routing_file" "$routing_status"
  if [[ "$(cat "$routing_status")" != "200" ]]; then
    echo "real agent-first e2e: failed to list existing platform nodes for ${alias}." >&2
    exit 1
  fi
  python3 - "$alias" "$node_id" "$routing_file" > "$delete_body" <<'PY'
import json
import sys

alias, node_id, routing_file = sys.argv[1:4]
payload = json.load(open(routing_file, encoding="utf-8"))
nodes = payload.get("access_nodes") or payload.get("accessNodes") or payload.get("data", {}).get("access_nodes") or []
ids = {node_id}
expected_name = f"测试中转-{alias}"
for node in nodes:
    current_id = str(node.get("id") or "").strip()
    name = str(node.get("name") or "").strip()
    if current_id and name == expected_name:
        ids.add(current_id)
print(json.dumps({"access_node_ids": sorted(ids)}, ensure_ascii=False))
PY
  curl_admin POST "/api/admin/access-nodes/batch-delete" "$delete_body" "${delete_body}.out" "$delete_status" >/dev/null || true
}

echo "real agent-first e2e: installing ${TARGET_COUNT} test access agents"
created_nodes=()
for index in $(seq 1 "$TARGET_COUNT"); do
  alias_var="TARGET_${index}_ALIAS"
  host_var="TARGET_${index}_PUBLIC_HOST"
  alias="${!alias_var}"
  public_host="${!host_var}"
  auth_code_file="${TMP_DIR}/auth-${index}.txt"
  echo "real agent-first e2e: installing agent on ${alias}"
  XRAYC_REAL_E2E_TARGET="$alias" \
    XRAYC_REMOTE_E2E_AUTH_CODE_OUT="$auth_code_file" \
    XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP="$ALLOW_INSECURE_HTTP" \
    XRAYC_REMOTE_E2E_EXPECTED_LISTEN_PORTS_OVERRIDE="" \
    XRAYC_REMOTE_E2E_CHECK_ARTIFACTS="${XRAYC_AGENT_FIRST_CHECK_ARTIFACTS:-1}" \
    XRAYC_REMOTE_E2E_HEARTBEAT_WAIT_SECONDS="${XRAYC_AGENT_FIRST_REMOTE_HEARTBEAT_WAIT_SECONDS:-5}" \
    bash scripts/real-remote-access-deploy-e2e.sh >/dev/null
  if [[ ! -s "$auth_code_file" ]]; then
    echo "real agent-first e2e: auth code file was not produced." >&2
    exit 1
  fi
  node_id="$(python3 - "$auth_code_file" <<'PY'
import sys
code=open(sys.argv[1], encoding="utf-8").read().strip()
parts=code.split(":", 2)
if len(parts) != 3 or parts[0] != "xrayc-agent-v1":
    raise SystemExit("invalid auth code")
print(parts[1])
PY
)"
  create_body="${TMP_DIR}/create-${index}.json"
  delete_existing_alias_nodes "$alias" "$node_id"
  python3 - "$alias" "$public_host" "$PUBLIC_PORT" "$auth_code_file" > "$create_body" <<'PY'
import json
import sys
alias, public_host, public_port, auth_code_file = sys.argv[1:5]
auth_code=open(auth_code_file, encoding="utf-8").read().strip()
payload={
    "name": f"测试中转-{alias}",
    "public_host": public_host,
    "public_port": int(public_port),
    "agent_token": auth_code,
    "remark": "agent first real nodes e2e"
}
print(json.dumps(payload, ensure_ascii=False))
PY
  curl_admin POST "/api/admin/access-nodes" "$create_body" "${TMP_DIR}/create-${index}.json.out" "${TMP_DIR}/create-${index}.status"
  if [[ "$(cat "${TMP_DIR}/create-${index}.status")" != "201" ]]; then
    echo "real agent-first e2e: failed to add installed agent in platform for ${alias}." >&2
    exit 1
  fi
  created_nodes+=("$node_id")
  echo "real agent-first e2e: platform node added for ${alias}"
done

nodes_csv="$(IFS=,; printf '%s' "${created_nodes[*]}")"
deadline="$(($(date +%s) + HEARTBEAT_WAIT_SECONDS))"
echo "real agent-first e2e: waiting for ${TARGET_COUNT} platform heartbeats"
while [[ "$(date +%s)" -le "$deadline" ]]; do
  curl_admin GET "/api/admin/access-routing" "" "${TMP_DIR}/routing.json" "${TMP_DIR}/routing.status"
  if [[ "$(cat "${TMP_DIR}/routing.status")" == "200" ]] && python3 - "$nodes_csv" "${TMP_DIR}/routing.json" <<'PY'
import json
import sys
ids=set(filter(None, sys.argv[1].split(",")))
data=json.load(open(sys.argv[2], encoding="utf-8"))
nodes=data.get("access_nodes") or data.get("accessNodes") or data.get("data", {}).get("access_nodes") or []
seen={}
for node in nodes:
    node_id=str(node.get("id") or "")
    if node_id in ids:
        status=str(node.get("health_status") or node.get("healthStatus") or "")
        heartbeat=node.get("last_heartbeat_at") or node.get("lastHeartbeatAt")
        seen[node_id]=bool(heartbeat) and status not in ("offline", "unknown", "")
missing=ids-set(seen)
if missing or not all(seen.values()):
    raise SystemExit(1)
PY
  then
    echo "real agent-first e2e completed: ${TARGET_COUNT} nodes online"
    exit 0
  fi
  sleep "$HEARTBEAT_POLL_SECONDS"
done

echo "real agent-first e2e: heartbeat did not become healthy for all ${TARGET_COUNT} nodes." >&2
exit 1
