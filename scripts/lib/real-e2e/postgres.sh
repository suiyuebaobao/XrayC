#!/usr/bin/env bash
# 用途：提供 real e2e 中从 DATABASE_URL 安全生成 psql 连接配置的函数。
# 该文件只定义 Postgres 相关 helper，不主动建立连接或执行查询。
set -euo pipefail

xrayc_real_e2e_write_pg_service_from_url() {
  local database_url="$1"
  local service_file="$2"
  local pass_file="$3"
  local url_file="$4"

  printf '%s' "$database_url" > "$url_file"
  python3 - "$url_file" "$service_file" "$pass_file" <<'PY'
import os
import sys
from urllib.parse import parse_qs, unquote, urlparse

url_file, service_file, pass_file = sys.argv[1], sys.argv[2], sys.argv[3]
with open(url_file, "r", encoding="utf-8") as fh:
    raw_url = fh.read().strip()

parsed = urlparse(raw_url)
if parsed.scheme not in {"postgres", "postgresql"}:
    raise SystemExit(2)

host = parsed.hostname or ""
port = parsed.port or 5432
user = unquote(parsed.username or "")
password = unquote(parsed.password or "")
dbname = unquote((parsed.path or "").lstrip("/"))
if not host or not dbname:
    raise SystemExit(2)

def clean(value: str) -> str:
    if "\n" in value or "\r" in value:
        raise SystemExit(2)
    return value

query = parse_qs(parsed.query)
lines = [
    "[xrayc_real_e2e]",
    f"host={clean(host)}",
    f"port={int(port)}",
    f"dbname={clean(dbname)}",
]
if user:
    lines.append(f"user={clean(user)}")
lines.append(f"passfile={clean(pass_file)}")
for key in ("sslmode", "sslrootcert", "sslcert", "sslkey", "connect_timeout"):
    value = query.get(key, [""])[0]
    if value:
        lines.append(f"{key}={clean(value)}")

with open(service_file, "w", encoding="utf-8") as fh:
    fh.write("\n".join(lines) + "\n")
os.chmod(service_file, 0o600)

def pgpass_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace(":", "\\:")

with open(pass_file, "w", encoding="utf-8") as fh:
    if user and password:
        fh.write(
            ":".join(
                [
                    pgpass_escape(host),
                    str(int(port)),
                    pgpass_escape(dbname),
                    pgpass_escape(user),
                    pgpass_escape(password),
                ]
            )
            + "\n"
        )
os.chmod(pass_file, 0o600)
PY
}

xrayc_real_e2e_psql_database_url() {
  local database_url="$1"
  shift
  local tmp_dir service_file pass_file url_file password_file status old_errexit

  tmp_dir="$(mktemp -d)"
  service_file="$tmp_dir/pgservice.conf"
  pass_file="$tmp_dir/pgpass"
  url_file="$tmp_dir/database-url.txt"
  password_file="$tmp_dir/password.txt"
  if ! xrayc_real_e2e_write_pg_service_from_url "$database_url" "$service_file" "$pass_file" "$url_file"; then
    rm -rf "$tmp_dir"
    echo "DATABASE_URL is invalid for psql connection" >&2
    return 2
  fi
  python3 - "$url_file" "$password_file" <<'PY'
import os
import sys
from urllib.parse import unquote, urlparse

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    raw_url = fh.read().strip()
password = unquote(urlparse(raw_url).password or "")
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    fh.write(password)
os.chmod(sys.argv[2], 0o600)
PY
  old_errexit=0
  case "$-" in
    *e*) old_errexit=1; set +e ;;
  esac
  PGPASSWORD="$(cat "$password_file")" PGSERVICEFILE="$service_file" PGPASSFILE="$pass_file" PGSERVICE=xrayc_real_e2e psql "$@"
  status=$?
  if [[ "$old_errexit" -eq 1 ]]; then
    set -e
  fi
  rm -rf "$tmp_dir"
  return "$status"
}

xrayc_real_e2e_assert_loadtest_database_url() {
  local database_url="$1"
  local forbidden_url="${2:-}"

  DATABASE_URL_VALUE="$database_url" FORBIDDEN_URL_VALUE="$forbidden_url" python3 - <<'PY'
import os
from urllib.parse import unquote, urlparse

def parse(raw: str):
    parsed = urlparse(raw.strip())
    if parsed.scheme not in {"postgres", "postgresql"}:
        raise SystemExit("runtime loadtest DATABASE_URL must be postgres")
    host = (parsed.hostname or "").lower()
    port = parsed.port or 5432
    dbname = unquote((parsed.path or "").lstrip("/"))
    if not host or not dbname:
        raise SystemExit("runtime loadtest DATABASE_URL must include host and database name")
    return host, port, dbname

target = parse(os.environ["DATABASE_URL_VALUE"])
if "loadtest" not in target[2].lower() and os.environ.get("RUNTIME_LOADTEST_ALLOW_UNSAFE_DATABASE") != "1":
    raise SystemExit("runtime loadtest database name must contain loadtest")

forbidden = os.environ.get("FORBIDDEN_URL_VALUE", "").strip()
if forbidden and parse(forbidden) == target:
    raise SystemExit("runtime loadtest refuses to reuse forbidden database")
PY
}
