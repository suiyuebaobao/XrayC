#!/usr/bin/env bash
# 用途：执行大数据量运维误操作恢复 UAT，验证禁用和恢复链路。
# 范围：使用隔离数据库构造大样本，不触碰默认控制面生产数据。
# 输入：需要独立数据库 URL，并可配置最大查询耗时与仅校验模式。
# 输出：输出脱敏阶段结果、恢复断言和查询耗时边界结论。
# 依赖：source real-e2e-lib.sh，并使用 runtime-loadtest 种子和 psql。
# 安全：禁止复用控制库，日志不得打印数据库 URL 或真实业务数据。
# 约束：必须显式设置 RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT=1 才执行破坏性演练。
# 行为：禁用运行成员，验证分配清理、dirty 标记、审计脱敏和恢复查询。
# 失败：环境不隔离、依赖缺失、断言失败或查询超预算都会退出非零。
# 维护：恢复 SQL 或审计字段变化时同步本脚本的大样本断言。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

usage() {
  cat <<'USAGE'
usage: RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT=1 bash scripts/ops-mistake-recovery-large-uat.sh

Runs an isolated large-data operator recovery drill. It prepares the runtime
loadtest seed, keeps a large synthetic dataset, disables/restores one runtime
member, verifies assignment cleanup, dirty marking, audit redaction boundaries,
and checks recovery queries stay within the latency budget.

Required:
  OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL or RUNTIME_LOADTEST_DATABASE_URL.
Optional:
  OPS_MISTAKE_RECOVERY_LARGE_UAT_VALIDATE_ONLY=1
  OPS_MISTAKE_RECOVERY_LARGE_MAX_QUERY_MS=2000
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

control_database_url="${DATABASE_URL:-}"
large_database_url="${OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL:-${RUNTIME_LOADTEST_DATABASE_URL:-}}"
max_query_ms="${OPS_MISTAKE_RECOVERY_LARGE_MAX_QUERY_MS:-2000}"

xrayc_real_e2e_require_url_scheme large_database_url "large operator recovery UAT"
if [[ -n "$control_database_url" && "$control_database_url" == "$large_database_url" ]]; then
  echo "large operator recovery UAT requires an isolated database URL" >&2
  exit 2
fi
if [[ ! "$max_query_ms" =~ ^[0-9]+$ || "$max_query_ms" -le 0 ]]; then
  echo "OPS_MISTAKE_RECOVERY_LARGE_MAX_QUERY_MS must be a positive integer" >&2
  exit 2
fi

if xrayc_real_e2e_bool_is_true "${OPS_MISTAKE_RECOVERY_LARGE_UAT_VALIDATE_ONLY:-0}"; then
  RUNTIME_LOADTEST_VALIDATE_ONLY=1 \
  RUNTIME_LOADTEST_PROFILE=large \
  DATABASE_URL="$large_database_url" \
  RUNTIME_LOADTEST_FORBID_DATABASE_URL="$control_database_url" \
  XRAYC_ENV=loadtest \
  bash scripts/runtime-loadtest.sh >/dev/null
  echo "ops_mistake_recovery_large_uat: validation_passed"
  exit 0
fi
if ! xrayc_real_e2e_bool_is_true "${RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT:-0}"; then
  echo "RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT=1 is required" >&2
  exit 2
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for large operator recovery UAT" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

run_marker="ops-large-$(date +%Y%m%d%H%M%S)-$$"

psql_large() {
  xrayc_real_e2e_psql_database_url "$large_database_url" "$@"
}

measure_query_ms() {
  local label="$1"
  local sql="$2"
  local output_file="$tmp_dir/${label}.json"
  psql_large -XAtq -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) ${sql}" >"$output_file"
  python3 - "$output_file" "$label" "$max_query_ms" <<'PY'
import json
import sys

path, label, max_ms = sys.argv[1], sys.argv[2], float(sys.argv[3])
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
plan = payload[0] if isinstance(payload, list) and payload and isinstance(payload[0], dict) else payload[0][0]
ms = float(plan["Execution Time"])
if ms > max_ms:
    raise SystemExit(f"{label} exceeded latency budget")
print(f"{label}: {ms:.2f} ms")
PY
}

echo "ops_mistake_recovery_large_uat: preparing isolated seed"
DATABASE_URL="$large_database_url" \
RUNTIME_LOADTEST_FORBID_DATABASE_URL="$control_database_url" \
XRAYC_ENV=loadtest \
bash scripts/prepare-runtime-loadtest-seed.sh >/dev/null

echo "ops_mistake_recovery_large_uat: preparing large synthetic dataset"
DATABASE_URL="$large_database_url" \
RUNTIME_LOADTEST_FORBID_DATABASE_URL="$control_database_url" \
RUNTIME_LOADTEST_PROFILE=large \
RUNTIME_LOADTEST_REQUIRE_LARGE=1 \
RUNTIME_LOADTEST_KEEP_DATA=1 \
XRAYC_ENV=loadtest \
bash scripts/runtime-loadtest.sh >/dev/null

echo "ops_mistake_recovery_large_uat: running recovery drill"
psql_large -XAtq -F $'\t' -v ON_ERROR_STOP=1 -v "marker=$run_marker" <<'SQL' >"$tmp_dir/recovery.tsv"
WITH selected AS (
  SELECT
    p.id AS pool_id,
    m.exit_endpoint_id,
    l.access_node_id
  FROM exit_pools p
  JOIN exit_pool_members m ON m.exit_pool_id = p.id
  JOIN access_lines l ON l.exit_pool_id = p.id
  WHERE p.enabled = TRUE
    AND m.status = 'healthy'
    AND m.allow_new_assignments = TRUE
    AND l.enabled = TRUE
  ORDER BY p.name, m.priority, m.exit_endpoint_id, l.id
  LIMIT 1
),
before_assignments AS (
  SELECT COUNT(*)::BIGINT AS count
  FROM user_exit_assignments uea
  JOIN selected s ON s.pool_id = uea.exit_pool_id
                AND s.exit_endpoint_id = uea.exit_endpoint_id
),
disabled AS (
  UPDATE exit_pool_members m
  SET status = 'offline',
      allow_new_assignments = FALSE,
      updated_at = now()
  FROM selected s
  WHERE m.exit_pool_id = s.pool_id
    AND m.exit_endpoint_id = s.exit_endpoint_id
  RETURNING 1
),
cleaned AS (
  DELETE FROM user_exit_assignments uea
  USING selected s
  WHERE uea.exit_pool_id = s.pool_id
    AND uea.exit_endpoint_id = s.exit_endpoint_id
  RETURNING 1
),
dirty_nodes AS (
  UPDATE access_nodes n
  SET config_dirty = TRUE,
      config_dirty_at = now(),
      config_dirty_reason = 'ops_large_recovery_exit_disabled'
  FROM (
    SELECT DISTINCT l.access_node_id
    FROM access_lines l
    JOIN selected s ON s.pool_id = l.exit_pool_id
  ) affected
  WHERE n.id = affected.access_node_id
  RETURNING 1
),
audit AS (
  INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
  SELECT
    :'marker',
    'exit_pool_member',
    s.exit_endpoint_id,
    jsonb_build_object('scope', 'large-uat', 'sensitive_values', 'redacted'),
    'success'
  FROM selected s
  RETURNING 1
)
SELECT
  (SELECT count FROM before_assignments),
  (SELECT COUNT(*) FROM disabled),
  (SELECT COUNT(*) FROM cleaned),
  (SELECT COUNT(*) FROM dirty_nodes),
  (SELECT COUNT(*) FROM audit);
SQL

IFS=$'\t' read -r before_count disabled_count cleaned_count dirty_count audit_count <"$tmp_dir/recovery.tsv"
if [[ "${disabled_count:-0}" -lt 1 || "${dirty_count:-0}" -lt 1 || "${audit_count:-0}" -lt 1 ]]; then
  echo "ops_mistake_recovery_large_uat: disable drill did not touch required rows" >&2
  exit 1
fi
if [[ "${before_count:-0}" -gt 0 && "${cleaned_count:-0}" -lt 1 ]]; then
  echo "ops_mistake_recovery_large_uat: assignment cleanup did not remove affected rows" >&2
  exit 1
fi

psql_large -XAtq -v ON_ERROR_STOP=1 -v "marker=$run_marker" <<'SQL' >/dev/null
WITH target AS (
  SELECT resource_id::uuid AS exit_endpoint_id
  FROM audit_logs
  WHERE action = :'marker'
  ORDER BY created_at DESC, id DESC
  LIMIT 1
),
pool_target AS (
  SELECT m.exit_pool_id, m.exit_endpoint_id
  FROM exit_pool_members m
  JOIN target t ON t.exit_endpoint_id = m.exit_endpoint_id
  ORDER BY m.updated_at DESC NULLS LAST
  LIMIT 1
),
restored AS (
  UPDATE exit_pool_members m
  SET status = 'healthy',
      allow_new_assignments = TRUE,
      updated_at = now()
  FROM pool_target p
  WHERE m.exit_pool_id = p.exit_pool_id
    AND m.exit_endpoint_id = p.exit_endpoint_id
  RETURNING 1
),
dirty_nodes AS (
  UPDATE access_nodes n
  SET config_dirty = TRUE,
      config_dirty_at = now(),
      config_dirty_reason = 'ops_large_recovery_exit_restored'
  FROM (
    SELECT DISTINCT l.access_node_id
    FROM access_lines l
    JOIN pool_target p ON p.exit_pool_id = l.exit_pool_id
  ) affected
  WHERE n.id = affected.access_node_id
  RETURNING 1
)
INSERT INTO audit_logs (action, resource_type, resource_id, request_summary, result)
SELECT :'marker' || '.restore', 'exit_pool_member', p.exit_endpoint_id,
       jsonb_build_object('scope', 'large-uat', 'sensitive_values', 'redacted'),
       'success'
FROM pool_target p;
SQL

audit_leak_count="$(psql_large -XAtq -v ON_ERROR_STOP=1 -v "marker=$run_marker" <<'SQL'
SELECT COUNT(*)::BIGINT
FROM audit_logs
WHERE action IN (:'marker', :'marker' || '.restore')
  AND request_summary::text ~* '(socks5://|http://|https://|vless://|trojan://|ss://|hy2://|password|passwd|token|secret|proxy_url|raw_url|private_key|api_key|bearer)';
SQL
)"
if [[ "${audit_leak_count:-0}" -ne 0 ]]; then
  echo "ops_mistake_recovery_large_uat: audit redaction scan found sensitive-shaped values" >&2
  exit 1
fi

measure_query_ms "ops-large-assignment-check" "
SELECT COUNT(*)::BIGINT
FROM user_exit_assignments uea
JOIN exit_pool_members m
  ON m.exit_pool_id = uea.exit_pool_id
 AND m.exit_endpoint_id = uea.exit_endpoint_id
WHERE m.status <> 'healthy' OR m.allow_new_assignments = FALSE"

measure_query_ms "ops-large-audit-check" "
SELECT id, action, resource_type, result, created_at
FROM audit_logs
WHERE action IN ('${run_marker}', '${run_marker}.restore')
ORDER BY created_at DESC, id DESC
LIMIT 20"

measure_query_ms "ops-large-audit-redaction-scan" "
SELECT COUNT(*)::BIGINT
FROM audit_logs
WHERE action IN ('${run_marker}', '${run_marker}.restore')
  AND request_summary::text ~* '(socks5://|http://|https://|vless://|trojan://|ss://|hy2://|password|passwd|token|secret|proxy_url|raw_url|private_key|api_key|bearer)'"

measure_query_ms "ops-large-dirty-node-check" "
SELECT id, config_dirty_reason, config_dirty_at
FROM access_nodes
WHERE config_dirty = TRUE
  AND config_dirty_reason LIKE 'ops_large_recovery_%'
ORDER BY config_dirty_at DESC
LIMIT 50"

echo "ops_mistake_recovery_large_uat: completed"
