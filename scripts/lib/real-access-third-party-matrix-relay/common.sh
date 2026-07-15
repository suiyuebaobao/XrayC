#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的通用基础函数。
# 包含状态输出、依赖检查、占位值校验、数据库包装、JSON/环境文件写入。
# 还包含测试退出清理逻辑，供主入口和其他 helper 共同复用。
# 本文件不直接编排 E2E 流程，也不输出第三方私有 endpoint 明文。

die() {
  echo "real_matrix_relay_e2e: $*" >&2
  exit 1
}

status() {
  printf 'real_matrix_relay_e2e: %s\n' "$1"
}

require_file() {
  local path="$1"
  [[ -f "$path" ]] || die "required private file is missing"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "local ${1} command is missing"
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  return 1
}

require_release_value() {
  local name="$1"
  local value="${!name:-}"
  if [[ -z "$value" || "$value" == admin@example.test || "$value" == demo@example.test ]]; then
    die "${name} is required for real release relay E2E"
  fi
  value_is_placeholder "$value" && die "${name} still contains a placeholder"
  return 0
}

psql_db() {
  xrayc_real_e2e_psql_database_url "$DB_URL" "$@"
}

cleanup() {
  set +e
  if [[ "$keep_debug_artifacts" == "1" && "$completed_ok" != "1" ]]; then
    status "keeping_debug_artifacts=1 path=$TMP_DIR"
    return 0
  fi
  if [[ "$original_subscription_snapshot_saved" == "1" && -n "${user_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v user_id="$user_id" \
      -v active="$original_sub_active" \
      -v expires_at="$original_sub_expires_at" \
      -v used_bytes="$original_sub_used_bytes" \
      -v limit_bytes="$original_sub_limit_bytes" <<'SQL' >/dev/null 2>&1 || true
UPDATE user_subscriptions
SET active = :'active'::boolean,
    expires_at = :'expires_at'::timestamptz,
    used_bytes = :'used_bytes'::bigint,
    limit_bytes = :'limit_bytes'::bigint,
    updated_at = now()
WHERE user_id = :'user_id'::uuid;
SQL
  fi
  if [[ "$original_user_snapshot_saved" == "1" && -n "${user_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v user_id="$user_id" \
      -v disabled="$original_user_disabled" <<'SQL' >/dev/null 2>&1 || true
UPDATE users SET disabled = :'disabled'::boolean WHERE id = :'user_id'::uuid;
SQL
  fi
  if [[ -n "${access_line_id:-}" || -n "${pool_id:-}" || -n "${access_node_id:-}" || -n "${group_id:-}" ]]; then
    psql_db -Xq -v ON_ERROR_STOP=0 \
      -v access_line_id="${access_line_id:-00000000-0000-0000-0000-000000000000}" \
      -v pool_id="${pool_id:-00000000-0000-0000-0000-000000000000}" \
      -v access_node_id="${access_node_id:-00000000-0000-0000-0000-000000000000}" \
      -v group_id="${group_id:-00000000-0000-0000-0000-000000000000}" <<'SQL' >/dev/null 2>&1 || true
DELETE FROM user_access_line_assignments WHERE access_line_id = :'access_line_id'::uuid;
DELETE FROM user_exit_assignments WHERE access_line_id = :'access_line_id'::uuid OR exit_pool_id = :'pool_id'::uuid;
DELETE FROM usage_ledgers WHERE access_line_id = :'access_line_id'::uuid;
DELETE FROM plan_line_groups WHERE line_group_id = :'group_id'::uuid;
DELETE FROM line_group_binding_nodes WHERE line_group_id = :'group_id'::uuid;
DELETE FROM line_group_exit_endpoints WHERE line_group_id = :'group_id'::uuid;
DELETE FROM line_groups WHERE id = :'group_id'::uuid;
DELETE FROM access_line_metric_snapshots WHERE access_line_id = :'access_line_id'::uuid;
DELETE FROM access_user_sessions WHERE access_line_id = :'access_line_id'::uuid;
DELETE FROM access_lines WHERE id = :'access_line_id'::uuid;
DELETE FROM access_exit_probe_states WHERE access_node_id = :'access_node_id'::uuid;
DELETE FROM access_exit_probes WHERE access_node_id = :'access_node_id'::uuid;
DELETE FROM exit_pool_members WHERE exit_pool_id = :'pool_id'::uuid;
DELETE FROM exit_pools WHERE id = :'pool_id'::uuid;
DELETE FROM access_nodes WHERE id = :'access_node_id'::uuid;
SQL
  fi
  if declare -F remote_exec >/dev/null 2>&1; then
    remote_exec 1 "docker rm -f xrayc-real-matrix-relay-client >/dev/null 2>&1 || true; rm -rf /opt/xrayc-real-matrix-relay-client" >/dev/null 2>&1 || true
    remote_exec 2 "docker compose -p xrayc-real-matrix-relay -f /opt/xrayc-real-matrix-relay/docker-compose.yml down --remove-orphans >/dev/null 2>&1 || true; docker rm -f \$(docker ps -aq --filter label=com.docker.compose.project=xrayc-real-matrix-relay) >/dev/null 2>&1 || true; rm -rf /opt/xrayc-real-matrix-relay" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
}

on_error() {
  local line="${1:-unknown}"
  status "failed_line=${line}"
}

json_value() {
  local file="$1"
  local expression="$2"
  python3 - "$file" "$expression" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
cursor = payload.get("data", payload) if isinstance(payload, dict) else payload
for part in sys.argv[2].split("."):
    if not isinstance(cursor, dict):
        raise SystemExit(1)
    cursor = cursor.get(part)
    if cursor is None:
        raise SystemExit(1)
print(cursor)
PY
}

json_array_first_value() {
  local file="$1"
  local expression="$2"
  python3 - "$file" "$expression" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
cursor = payload.get("data", payload) if isinstance(payload, dict) else payload
for part in sys.argv[2].split("."):
    if not isinstance(cursor, dict):
        raise SystemExit(1)
    cursor = cursor.get(part)
    if cursor is None:
        raise SystemExit(1)
if not isinstance(cursor, list) or not cursor:
    raise SystemExit(1)
print(cursor[0])
PY
}

write_json() {
  local output="$1"
  local code="$2"
  python3 - "$output" > "$output" <<PY
import json
import os
import secrets
import sys

output = sys.argv[1]
$code
PY
}

write_shell_env() {
  local output="$1"
  shift
  python3 - "$output" "$@" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    for name in sys.argv[2:]:
        fh.write(f"export {name}={shlex.quote(os.environ[name])}\n")
PY
  chmod 600 "$output"
}
