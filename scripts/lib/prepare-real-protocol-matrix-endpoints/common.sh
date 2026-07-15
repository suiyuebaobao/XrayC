#!/usr/bin/env bash
# 用途：提供真实协议矩阵 endpoint 准备脚本的通用 shell 工具函数。
# 范围：仅供 scripts/prepare-real-protocol-matrix-endpoints.sh source 使用。
# 安全：错误信息只包含变量名和粗粒度状态，不输出私密值或响应正文。
# 约束：这里不执行主流程，只定义可复用函数，便于主脚本保持短小。

die() {
  echo "real-protocol-matrix-endpoints: $*" >&2
  exit 1
}

status() {
  printf 'real-protocol-matrix-endpoints: %s\n' "$1"
}

if ! declare -F xrayc_real_e2e_psql_database_url >/dev/null 2>&1; then
  # shellcheck source=scripts/lib/real-e2e/postgres.sh
  . "${BASE_DIR:?}/scripts/lib/real-e2e/postgres.sh"
fi

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "local ${1} command is missing"
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-host* ]] && return 0
  [[ "$value" == *private-file* ]] && return 0
  [[ "$value" == *provider-console* ]] && return 0
  [[ "$value" == *generated-exit* ]] && return 0
  [[ "$value" == *client-runtime* ]] && return 0
  [[ "$value" == *observation* ]] && return 0
  [[ "$value" == *admin-backend* ]] && return 0
  [[ "$value" == *database* ]] && return 0
  [[ "$value" == *compose-postgres* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  return 1
}

value_ready() {
  local value="${1:-}"
  [[ -n "$value" ]] || return 1
  value_is_placeholder "$value" && return 1
  return 0
}

require_ready_value() {
  local name="$1"
  local value="${!name:-}"
  value_ready "$value" || die "${name} is required"
}

require_uuid_value() {
  local label="$1"
  local value="$2"
  [[ "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
    || die "${label} must be a UUID"
}

endpoint_exists_in_database() {
  local endpoint_id="$1"
  local exists=""

  value_ready "${DATABASE_URL:-}" || return 0
  command -v psql >/dev/null 2>&1 || return 0
  exists="$(
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
      -v ON_ERROR_STOP=1 \
      -v endpoint_id="$endpoint_id" <<'SQL' 2>/dev/null || true
SELECT EXISTS (
  SELECT 1
  FROM exit_endpoints
  WHERE id = :'endpoint_id'::uuid
    AND enabled = TRUE
)::text;
SQL
  )"
  [[ "$exists" == "t" || "$exists" == "true" ]]
}

endpoint_payload_matches_database() {
  local endpoint_id="$1"
  local payload_file="$2"
  local payload_json=""
  local matches=""

  value_ready "${DATABASE_URL:-}" || return 0
  command -v psql >/dev/null 2>&1 || return 0
  [[ -f "$payload_file" ]] || return 1
  payload_json="$(python3 - "$payload_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
print(json.dumps(payload, ensure_ascii=False, separators=(",", ":")))
PY
  )"
  matches="$(
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
      -v ON_ERROR_STOP=1 \
      -v endpoint_id="$endpoint_id" \
      -v payload="$payload_json" <<'SQL' 2>/dev/null || true
WITH expected AS (
  SELECT :'payload'::jsonb AS payload
)
SELECT EXISTS (
  SELECT 1
  FROM exit_endpoints endpoint
  CROSS JOIN expected
  WHERE endpoint.id = :'endpoint_id'::uuid
    AND endpoint.enabled = COALESCE((expected.payload->>'enabled')::boolean, TRUE)
    AND endpoint.outbound_type::text = expected.payload->>'outbound_type'
    AND endpoint.host = expected.payload->>'host'
    AND endpoint.port = (expected.payload->>'port')::integer
    AND COALESCE(endpoint.outbound_config, '{}'::jsonb)
      - 'password' - 'pass' - 'username' - 'user' - 'uuid' - 'id'
      - 'private_key' - 'privateKey' - 'secret' - 'token'
      = COALESCE(expected.payload->'outbound_config', '{}'::jsonb)
      - 'password' - 'pass' - 'username' - 'user' - 'uuid' - 'id'
      - 'private_key' - 'privateKey' - 'secret' - 'token'
    AND COALESCE(endpoint.stream_config, '{}'::jsonb) = COALESCE(expected.payload->'stream_config', '{}'::jsonb)
    AND COALESCE(endpoint.probe_config, '{}'::jsonb) = COALESCE(expected.payload->'probe_config', '{}'::jsonb)
)::text;
SQL
  )"
  [[ "$matches" == "t" || "$matches" == "true" ]]
}

refresh_existing_endpoint_health() {
  local endpoint_id="$1"

  value_ready "${DATABASE_URL:-}" || return 0
  command -v psql >/dev/null 2>&1 || return 0
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq \
    -v ON_ERROR_STOP=1 \
    -v endpoint_id="$endpoint_id" <<'SQL' >/dev/null 2>&1 \
    || die "existing endpoint health refresh failed; details redacted"
UPDATE exit_resources r
SET enabled = TRUE,
    status = 'healthy',
    last_probe_status = 'healthy',
    last_probe_at = now()
FROM exit_endpoints e
WHERE e.id = :'endpoint_id'::uuid
  AND e.exit_resource_id = r.id;
UPDATE exit_endpoints
SET enabled = TRUE,
    last_probe_status = 'healthy',
    last_probe_at = now()
WHERE id = :'endpoint_id'::uuid;
SQL
}

protocol_suffix() {
  local protocol="$1"
  case "$protocol" in
    socks|socks5) printf 'SOCKS' ;;
    http) printf 'HTTP' ;;
    vless) printf 'VLESS' ;;
    trojan) printf 'TROJAN' ;;
    shadowsocks|ss) printf 'SHADOWSOCKS' ;;
    hy2|hysteria|hysteria2) printf 'HY2' ;;
    *) printf '' ;;
  esac
}

normalize_protocol() {
  local protocol="$1"
  protocol="$(printf '%s' "$protocol" | tr '[:upper:]' '[:lower:]' | xargs)"
  case "$protocol" in
    socks5) printf 'socks' ;;
    ss) printf 'shadowsocks' ;;
    hysteria|hysteria2) printf 'hy2' ;;
    *) printf '%s' "$protocol" ;;
  esac
}

quote_env_value() {
  python3 - "$1" <<'PY'
import shlex
import sys

print(shlex.quote(sys.argv[1]))
PY
}

upsert_env_value() {
  local name="$1"
  local value="$2"
  python3 - "$ENV_FILE" "$name" "$value" <<'PY'
from pathlib import Path
import os
import re
import shlex
import sys

path = Path(sys.argv[1])
name = sys.argv[2]
value = sys.argv[3]
line = f"{name}={shlex.quote(value)}\n"
pattern = re.compile(rf"^(?:export[ \t]+)?{re.escape(name)}=")

lines = []
replaced = False
if path.exists():
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
for index, existing in enumerate(lines):
    if pattern.match(existing):
        lines[index] = line
        replaced = True
        break
if not replaced:
    if lines and not lines[-1].endswith("\n"):
        lines[-1] += "\n"
    lines.append(line)
path.write_text("".join(lines), encoding="utf-8")
os.chmod(path, 0o600)
PY
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
