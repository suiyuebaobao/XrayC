#!/usr/bin/env bash
# 用途：真实发布门禁进入运行态压测前，确保隔离 loadtest PostgreSQL 库存在。
# 范围：只创建或修正 RUNTIME_LOADTEST_DATABASE_URL 指向的角色和数据库。
# 输入：需要 RUNTIME_LOADTEST_DATABASE_URL，并需要 DATABASE_URL 或 RUNTIME_LOADTEST_ADMIN_DATABASE_URL。
# 输出：只打印脱敏状态，不打印数据库 URL、密码或 SQL 明文参数。
# 安全：拒绝复用真实控制库；默认只允许用同一 Postgres 实例的控制库连接做管理入口。
# 约束：目标数据库名必须包含 loadtest，目标 URL 必须带用户名和密码。
# 行为：目标库结构就绪时直接返回；不可连接时创建/修正角色、数据库并对新库应用 migrations。
#       目标库可连接但结构较旧时只补必要的缺失迁移。
# 失败：管理连接权限不足、迁移失败或最终仍无法连接时返回非零。
# 维护：新增运行态压测所需基础 schema 时保持 migrations 目录完整即可。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

target_url="${RUNTIME_LOADTEST_DATABASE_URL:-}"
admin_url="${RUNTIME_LOADTEST_ADMIN_DATABASE_URL:-${DATABASE_URL:-}}"
forbidden_url="${RUNTIME_LOADTEST_FORBID_DATABASE_URL:-${DATABASE_URL:-}}"

if [[ -z "$target_url" ]]; then
  echo "RUNTIME_LOADTEST_DATABASE_URL is required" >&2
  exit 2
fi
if [[ -z "$admin_url" ]]; then
  echo "DATABASE_URL or RUNTIME_LOADTEST_ADMIN_DATABASE_URL is required" >&2
  exit 2
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for runtime loadtest database bootstrap" >&2
  exit 2
fi

RUNTIME_LOADTEST_ALLOW_UNSAFE_DATABASE= \
xrayc_real_e2e_assert_loadtest_database_url "$target_url" "$forbidden_url"

if xrayc_real_e2e_psql_database_url "$target_url" -XAtq -v ON_ERROR_STOP=1 <<'SQL' 2>/dev/null | grep -q '^ready$'; then
WITH required_columns(table_name, column_name) AS (
  VALUES ('access_lines', 'exit_endpoint_id')
)
SELECT CASE WHEN to_regclass('public.users') IS NOT NULL
  AND to_regclass('public.idx_access_lines_exit_endpoint_id') IS NOT NULL
  AND NOT EXISTS (
    SELECT 1
    FROM required_columns c
    WHERE NOT EXISTS (
      SELECT 1 FROM information_schema.columns
      WHERE table_schema = 'public'
        AND table_name = c.table_name
        AND column_name = c.column_name
    )
  )
THEN 'ready' ELSE 'missing' END;
SQL
  echo "runtime loadtest database ready"
  exit 0
fi

mapfile -t parsed < <(
  TARGET_URL="$target_url" ADMIN_URL="$admin_url" python3 - <<'PY'
import os
from urllib.parse import unquote, urlparse

def parse(name: str):
    raw = os.environ[name].strip()
    parsed = urlparse(raw)
    if parsed.scheme not in {"postgres", "postgresql"}:
        raise SystemExit(f"{name} must be postgres")
    host = (parsed.hostname or "").lower()
    port = parsed.port or 5432
    dbname = unquote((parsed.path or "").lstrip("/"))
    user = unquote(parsed.username or "")
    password = unquote(parsed.password or "")
    if not host or not dbname:
        raise SystemExit(f"{name} must include host and database")
    return host, str(port), dbname, user, password

target = parse("TARGET_URL")
admin = parse("ADMIN_URL")
if "loadtest" not in target[2].lower():
    raise SystemExit("runtime loadtest database name must contain loadtest")
if not target[3] or not target[4]:
    raise SystemExit("RUNTIME_LOADTEST_DATABASE_URL must include username and password")
if target[:3] == admin[:3]:
    raise SystemExit("runtime loadtest database must not be the admin/control database")
if "RUNTIME_LOADTEST_ADMIN_DATABASE_URL" not in os.environ and target[:2] != admin[:2]:
    raise SystemExit(
        "RUNTIME_LOADTEST_ADMIN_DATABASE_URL is required when loadtest DB is on another host or port"
    )
for value in (*target, *admin):
    if "\n" in value or "\r" in value or "\t" in value:
        raise SystemExit("database URL contains unsupported control characters")
print("\n".join([*target, *admin]))
PY
)

target_host="${parsed[0]}"
target_port="${parsed[1]}"
target_db="${parsed[2]}"
target_user="${parsed[3]}"
target_password="${parsed[4]}"

echo "runtime loadtest database bootstrap preparing"
xrayc_real_e2e_psql_database_url "$admin_url" -Xq -v ON_ERROR_STOP=1 \
  -v "target_user=$target_user" \
  -v "target_password=$target_password" \
  -v "target_db=$target_db" <<'SQL' >/dev/null
SELECT CASE
  WHEN EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'target_user')
  THEN format('ALTER ROLE %I WITH LOGIN PASSWORD %L', :'target_user', :'target_password')
  ELSE format('CREATE ROLE %I LOGIN PASSWORD %L', :'target_user', :'target_password')
END
\gexec

SELECT format('CREATE DATABASE %I OWNER %I', :'target_db', :'target_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'target_db')
\gexec

SELECT format('ALTER DATABASE %I OWNER TO %I', :'target_db', :'target_user')
\gexec
SQL

if ! xrayc_real_e2e_psql_database_url "$target_url" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.users') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  echo "runtime loadtest database bootstrap migrating"
  for migration in "$BASE_DIR"/migrations/*.sql; do
    xrayc_real_e2e_psql_database_url "$target_url" -Xq -v ON_ERROR_STOP=1 -f "$migration" >/dev/null
  done
fi
if ! xrayc_real_e2e_psql_database_url "$target_url" -XAtq -v ON_ERROR_STOP=1 <<'SQL' | grep -q '^ready$'; then
SELECT CASE WHEN EXISTS (
  SELECT 1 FROM information_schema.columns
  WHERE table_schema = 'public'
    AND table_name = 'access_lines'
    AND column_name = 'exit_endpoint_id'
) THEN 'ready' ELSE 'missing' END;
SQL
  echo "runtime loadtest database bootstrap migrating latest access line endpoint binding"
  xrayc_real_e2e_psql_database_url "$target_url" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202606060001_access_lines_exit_endpoint.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$target_url" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.idx_access_lines_exit_endpoint_id') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  echo "runtime loadtest database bootstrap adding latest access line endpoint index"
  xrayc_real_e2e_psql_database_url "$target_url" -Xq -v ON_ERROR_STOP=1 \
    -c "CREATE INDEX IF NOT EXISTS idx_access_lines_exit_endpoint_id ON access_lines(exit_endpoint_id)" >/dev/null
fi

if ! xrayc_real_e2e_psql_database_url "$target_url" -XAtq -v ON_ERROR_STOP=1 <<'SQL' | grep -q '^ready$'; then
SELECT CASE WHEN to_regclass('public.users') IS NOT NULL
  AND to_regclass('public.idx_access_lines_exit_endpoint_id') IS NOT NULL
  AND EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = 'access_lines'
      AND column_name = 'exit_endpoint_id'
  )
THEN 'ready' ELSE 'missing' END;
SQL
  echo "runtime loadtest database bootstrap did not reach required schema" >&2
  exit 1
fi
printf 'runtime loadtest database ready: host=%s port=%s db=%s user=%s\n' \
  "$target_host" \
  "$target_port" \
  "$target_db" \
  "$target_user"
