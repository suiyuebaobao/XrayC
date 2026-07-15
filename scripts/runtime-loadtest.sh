#!/usr/bin/env bash
# 中文说明：runtime loadtest 的入口脚本，负责解析参数、校验环境并串联压测流程。
# 中文说明：数据库 SQL、清理和测量细节拆到 scripts/lib/runtime-loadtest/ helper 中。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=lib/runtime-loadtest/data.sh
source "${SCRIPT_DIR}/lib/runtime-loadtest/data.sh"
# shellcheck source=lib/runtime-loadtest/measurements.sh
source "${SCRIPT_DIR}/lib/runtime-loadtest/measurements.sh"

# Docker 默认 /dev/shm 较小，large 压测禁用 PostgreSQL 并行 DSM，避免误判为业务失败。
export PGOPTIONS="${PGOPTIONS:-} -c max_parallel_workers_per_gather=0 -c max_parallel_maintenance_workers=0 -c maintenance_work_mem=16MB -c work_mem=8MB"

usage() {
  cat <<'USAGE'
usage: DATABASE_URL="<test-postgres-url>" bash scripts/runtime-loadtest.sh

Seeds high-volume runtime rows in an isolated PostgreSQL database, checks
critical query latency with EXPLAIN ANALYZE, then cleans up by default.

Optional:
  RUNTIME_LOADTEST_PROFILE       smoke, standard, or large. Default standard.
  RUNTIME_LOADTEST_LEDGER_ROWS   Default 100000; large profile default 1000000.
  RUNTIME_LOADTEST_AUDIT_ROWS    Default 50000; large profile default 500000.
  RUNTIME_LOADTEST_RUNTIME_ROWS  Default 20000; large profile default 100000.
  RUNTIME_LOADTEST_TRAFFIC_ROWS  Default matches runtime rows.
  RUNTIME_LOADTEST_MAX_QUERY_MS  Default 2000.
  RUNTIME_LOADTEST_MIN_USERS     Default 3; smoke default 2; large default 100.
  RUNTIME_LOADTEST_MIN_NODES     Default 2; smoke default 1; large default 3.
  RUNTIME_LOADTEST_MIN_LINES     Default 3; smoke default 2; large default 6.
  RUNTIME_LOADTEST_MIN_EXITS     Default 3; smoke default 2; large default 6.
  RUNTIME_LOADTEST_MIN_REPLICAS  Legacy alias for min lines/exits when those are unset.
  RUNTIME_LOADTEST_VALIDATE_ONLY=1  Validate env/profile/db isolation only.
  RUNTIME_LOADTEST_KEEP_DATA=1   Keep generated rows for manual inspection.
  RUNTIME_LOADTEST_ALLOW_PRODUCTION=1  Required when XRAYC_ENV=production.
  RUNTIME_LOADTEST_FORBID_DATABASE_URL  Database URL that must not be reused.
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

DATABASE_URL="${DATABASE_URL:-}"
RUNTIME_LOADTEST_PROFILE="${RUNTIME_LOADTEST_PROFILE:-standard}"
case "$RUNTIME_LOADTEST_PROFILE" in
  smoke)
    default_ledger_rows=100
    default_audit_rows=50
    default_runtime_rows=30
    default_min_users=2
    default_min_nodes=1
    default_min_lines=2
    default_min_exits=2
    ;;
  standard)
    default_ledger_rows=100000
    default_audit_rows=50000
    default_runtime_rows=20000
    default_min_users=3
    default_min_nodes=2
    default_min_lines=3
    default_min_exits=3
    ;;
  large)
    default_ledger_rows=1000000
    default_audit_rows=500000
    default_runtime_rows=100000
    default_min_users=100
    default_min_nodes=3
    default_min_lines=6
    default_min_exits=6
    ;;
  *)
    echo "RUNTIME_LOADTEST_PROFILE must be smoke, standard, or large" >&2
    exit 2
    ;;
esac
RUNTIME_LOADTEST_LEDGER_ROWS="${RUNTIME_LOADTEST_LEDGER_ROWS:-$default_ledger_rows}"
RUNTIME_LOADTEST_AUDIT_ROWS="${RUNTIME_LOADTEST_AUDIT_ROWS:-$default_audit_rows}"
RUNTIME_LOADTEST_RUNTIME_ROWS="${RUNTIME_LOADTEST_RUNTIME_ROWS:-$default_runtime_rows}"
RUNTIME_LOADTEST_TRAFFIC_ROWS="${RUNTIME_LOADTEST_TRAFFIC_ROWS:-$RUNTIME_LOADTEST_RUNTIME_ROWS}"
RUNTIME_LOADTEST_MAX_QUERY_MS="${RUNTIME_LOADTEST_MAX_QUERY_MS:-2000}"
RUNTIME_LOADTEST_MIN_USERS="${RUNTIME_LOADTEST_MIN_USERS:-$default_min_users}"
RUNTIME_LOADTEST_MIN_NODES="${RUNTIME_LOADTEST_MIN_NODES:-$default_min_nodes}"
legacy_min_replicas="${RUNTIME_LOADTEST_MIN_REPLICAS:-}"
RUNTIME_LOADTEST_MIN_LINES="${RUNTIME_LOADTEST_MIN_LINES:-${legacy_min_replicas:-$default_min_lines}}"
RUNTIME_LOADTEST_MIN_EXITS="${RUNTIME_LOADTEST_MIN_EXITS:-${legacy_min_replicas:-$default_min_exits}}"
run_marker="runtime-loadtest-$(date -u +%Y%m%d%H%M%S)-$$"
run_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

xrayc_real_e2e_require_url_scheme DATABASE_URL "runtime loadtest"
for name in \
  RUNTIME_LOADTEST_LEDGER_ROWS \
  RUNTIME_LOADTEST_AUDIT_ROWS \
  RUNTIME_LOADTEST_RUNTIME_ROWS \
  RUNTIME_LOADTEST_TRAFFIC_ROWS \
  RUNTIME_LOADTEST_MAX_QUERY_MS \
  RUNTIME_LOADTEST_MIN_USERS \
  RUNTIME_LOADTEST_MIN_NODES \
  RUNTIME_LOADTEST_MIN_LINES \
  RUNTIME_LOADTEST_MIN_EXITS; do
  if [[ ! "${!name}" =~ ^[1-9][0-9]*$ ]]; then
    echo "${name} must be a positive integer" >&2
    exit 2
  fi
done
if [[ "${XRAYC_ENV:-}" == "production" && "${RUNTIME_LOADTEST_ALLOW_PRODUCTION:-0}" != "1" ]]; then
  echo "runtime loadtest refuses production unless explicitly allowed" >&2
  exit 2
fi
if [[ -n "${RUNTIME_LOADTEST_FORBID_DATABASE_URL:-}" && "$DATABASE_URL" == "$RUNTIME_LOADTEST_FORBID_DATABASE_URL" ]]; then
  echo "runtime loadtest refuses to reuse the forbidden real-release DATABASE_URL" >&2
  exit 2
fi
xrayc_real_e2e_assert_loadtest_database_url "$DATABASE_URL" "${RUNTIME_LOADTEST_FORBID_DATABASE_URL:-}"
if [[ "${RUNTIME_LOADTEST_REQUIRE_LARGE:-0}" == "1" && "$RUNTIME_LOADTEST_PROFILE" != "large" ]]; then
  echo "runtime loadtest release gate requires RUNTIME_LOADTEST_PROFILE=large" >&2
  exit 2
fi
if [[ "$RUNTIME_LOADTEST_PROFILE" == "large" ]]; then
  if [[ "$RUNTIME_LOADTEST_LEDGER_ROWS" -lt 1000000 || "$RUNTIME_LOADTEST_AUDIT_ROWS" -lt 500000 || "$RUNTIME_LOADTEST_RUNTIME_ROWS" -lt 100000 ]]; then
    echo "large runtime loadtest requires at least 1000000 ledger, 500000 audit, and 100000 runtime rows" >&2
    exit 2
  fi
  if [[ "$RUNTIME_LOADTEST_MIN_USERS" -lt 100 || "$RUNTIME_LOADTEST_MIN_NODES" -lt 3 || "$RUNTIME_LOADTEST_MIN_LINES" -lt 6 || "$RUNTIME_LOADTEST_MIN_EXITS" -lt 6 ]]; then
    echo "large runtime loadtest requires at least 100 users, 3 access nodes, 6 access lines, and 6 exit endpoints" >&2
    exit 2
  fi
fi

if xrayc_real_e2e_bool_is_true "${RUNTIME_LOADTEST_VALIDATE_ONLY:-0}"; then
  echo "runtime loadtest validation passed"
  exit 0
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for runtime loadtest" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

if ! runtime_loadtest_database_has_required_seed_data; then
  echo "runtime loadtest database lacks required multi-user/multi-node/multi-entry/multi-exit seed data" >&2
  exit 2
fi

trap 'runtime_loadtest_cleanup_generated_rows; rm -rf "$tmp_dir"' EXIT

echo "runtime loadtest seeding started"
runtime_loadtest_seed_rows

if ! runtime_loadtest_generated_rows_ready; then
  echo "runtime loadtest seed failed; database has no required rows" >&2
  exit 1
fi

runtime_loadtest_run_measurements
echo "runtime loadtest completed"
