#!/usr/bin/env bash
# 用途：为真实 inbound 矩阵 E2E 准备缺失协议所需的临时数据库行。
# 范围：仅作为 real-access-inbound-matrix-e2e.sh 的内部辅助脚本使用。
# 输入：读取私有数据库、订阅、用户、节点、线路和协议矩阵环境变量。
# 输出：输出脱敏后的准备结果和 run id，不打印 SQL 原文或私有值。
# 依赖：source real-e2e-lib.sh，并通过 psql 与 python3 生成 token hash。
# 安全：关闭 xtrace，禁止日志包含 DB URL、token、主机、IP、密码或代理。
# 约束：只删除带本次生成 run id 标记的临时行，避免影响既有数据。
# 行为：校验真实环境后按协议补齐 inbound、订阅和访问线路关联数据。
# 失败：环境占位、UUID 非法、数据库结构不符或 SQL 失败会返回非零。
# 维护：inbound 数据模型变化时同步字段校验、插入和清理逻辑。
set -eEuo pipefail
IFS=$'\n\t'
set +x
last_debug_line=0
trap 'last_debug_line=$LINENO' DEBUG
trap 'echo "real_access_inbound_matrix_db_prepare: failed_line=${LINENO}" >&2' ERR
trap 'status=$?; if [[ "$status" -ne 0 ]]; then echo "real_access_inbound_matrix_db_prepare: exit_status=${status} failed_line=${last_debug_line}" >&2; fi' EXIT
BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="${BASE_DIR}/scripts"
# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=lib/real-access-inbound-matrix/db-prepare.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/db-prepare.sh"
die() {
  echo "real_access_inbound_matrix_db_prepare: $*" >&2
  exit 1
}
die_usage() {
  echo "real_access_inbound_matrix_db_prepare: $*" >&2
  exit 2
}
require_command() {
  command -v "$1" >/dev/null 2>&1 || die_usage "local ${1} command is missing"
}
value_is_placeholder() {
  local value="${1:-}"
  value="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *mock* ]] && return 0
  [[ "$value" == *fake* ]] && return 0
  return 1
}
require_real_env() {
  local name="$1"
  if [[ "$name" == "DATABASE_URL" || "$name" == "SUB_TOKEN" ]]; then
    [[ -n "${!name:-}" ]] || die_usage "${name} must be a real private value"
    return 0
  fi
  value_is_placeholder "${!name:-}" && die_usage "${name} must be a real private value"
  return 0
}
require_uuid_env() {
  local name="$1"
  require_real_env "$name"
  [[ "${!name}" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
    || die_usage "${name} must be a UUID"
}
token_hash() {
  TOKEN_VALUE="$SUB_TOKEN" python3 - <<'PY'
import hashlib
import os
print(hashlib.sha256(os.environ["TOKEN_VALUE"].encode("utf-8")).hexdigest())
PY
}
new_run_id() {
  python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
}
normalize_protocol_csv() {
  local raw="$1"
  PROTOCOLS_VALUE="$raw" python3 - <<'PY'
import os

items = []
for item in os.environ["PROTOCOLS_VALUE"].split(","):
    protocol = item.strip().lower().replace("-", "_")
    if protocol == "ss":
        protocol = "shadowsocks"
    if protocol in {
        "vless",
        "trojan",
        "shadowsocks",
    } and protocol not in items:
        items.append(protocol)
if not items:
    raise SystemExit(2)
print(",".join(items))
PY
}
run_psql_to_file() {
  local output_file="$1"
  local error_file="${output_file}.err"
  shift
  if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" "$@" >"$output_file" 2>"$error_file"; then
    rm -f "$output_file"
    sed -E 's#(postgres|postgresql)://[^[:space:]]+#<db-url>#Ig; s#(https?|socks5?|vless|trojan|ss|hysteria2?)://[^[:space:]]+#<url>#Ig; s/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g; s/[0-9a-fA-F-]{36}/<uuid>/g; s/(token|password|secret|key)[=:][^ ]+/\1=<redacted>/Ig' "$error_file" >&2 || true
    rm -f "$error_file"
    die "database operation failed; psql output redacted"
  fi
  rm -f "$error_file"
}

cleanup_rows() {
  local manifest_file="$1"
  local run_id tmp_output

  [[ -f "$manifest_file" ]] || return 0
  # shellcheck disable=SC1090
  . "$manifest_file"
  run_id="${RUN_ID:-}"
  [[ "$run_id" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] || return 0

  require_command psql
  require_real_env DATABASE_URL
  tmp_output="${manifest_file}.cleanup"
  run_psql_to_file "$tmp_output" \
    -XAtq \
    -v ON_ERROR_STOP=1 \
    -v "run_id=${run_id}" <<'SQL'
BEGIN;

CREATE TEMP TABLE xrayc_matrix_cleanup_lines AS
SELECT id, access_node_id, exit_pool_id
FROM access_lines
WHERE name LIKE ('real-inbound-matrix-' || :'run_id' || '-%');

CREATE TEMP TABLE xrayc_matrix_cleanup_pools AS SELECT exit_pool_id AS id FROM xrayc_matrix_cleanup_lines;

CREATE TEMP TABLE xrayc_matrix_cleanup_groups AS
SELECT id
FROM line_groups
WHERE name = 'real-inbound-matrix-' || :'run_id';

CREATE TEMP TABLE xrayc_matrix_cleanup_endpoints AS
SELECT id, exit_resource_id
FROM exit_endpoints
WHERE name = 'real-inbound-matrix-' || :'run_id' || '-socks-endpoint';

CREATE TEMP TABLE xrayc_matrix_cleanup_resources AS
SELECT id
FROM exit_resources
WHERE name = 'real-inbound-matrix-' || :'run_id' || '-socks-resource';

DELETE FROM user_exit_assignments uea
USING xrayc_matrix_cleanup_lines l
WHERE uea.access_line_id = l.id;

DELETE FROM user_access_line_assignments ula
USING xrayc_matrix_cleanup_lines l
WHERE ula.access_line_id = l.id;

DELETE FROM line_group_exit_endpoints lgee
USING xrayc_matrix_cleanup_groups g
WHERE lgee.line_group_id = g.id;

DELETE FROM line_group_binding_nodes lgbn
USING xrayc_matrix_cleanup_lines l
WHERE lgbn.entry_exit_binding_id = l.id;

DELETE FROM plan_line_groups plg
USING xrayc_matrix_cleanup_groups g
WHERE plg.line_group_id = g.id;

DELETE FROM access_entry_exit_bindings b
USING xrayc_matrix_cleanup_lines l
WHERE b.id = l.id OR b.access_entry_id = l.id;

DELETE FROM access_entries e
USING xrayc_matrix_cleanup_lines l
WHERE e.id = l.id;

DELETE FROM access_lines l
USING xrayc_matrix_cleanup_lines c
WHERE l.id = c.id;

DELETE FROM exit_pool_members epm USING xrayc_matrix_cleanup_pools p WHERE epm.exit_pool_id = p.id;

DELETE FROM exit_pools ep
USING xrayc_matrix_cleanup_pools p
WHERE ep.id = p.id;

DELETE FROM exit_endpoints e
USING xrayc_matrix_cleanup_endpoints c
WHERE e.id = c.id;

DELETE FROM exit_resources r
USING xrayc_matrix_cleanup_resources c
WHERE r.id = c.id
  AND NOT EXISTS (SELECT 1 FROM exit_endpoints e WHERE e.exit_resource_id = r.id);

DELETE FROM line_groups g
USING xrayc_matrix_cleanup_groups c
WHERE g.id = c.id;

UPDATE access_nodes n
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'real_inbound_matrix_e2e_cleanup'
WHERE n.id IN (SELECT DISTINCT access_node_id FROM xrayc_matrix_cleanup_lines);

COMMIT;
SQL
  rm -f "$tmp_output"
}

case "${1:-}" in
  prepare)
    [[ $# -eq 3 ]] || die_usage "usage: $0 prepare <protocol-csv> <manifest-file>"
    prepare_rows "$2" "$3"
    ;;
  cleanup)
    [[ $# -eq 2 ]] || die_usage "usage: $0 cleanup <manifest-file>"
    cleanup_rows "$2"
    ;;
  *)
    die_usage "usage: $0 prepare <protocol-csv> <manifest-file> | cleanup <manifest-file>"
    ;;
esac
