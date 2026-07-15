# 用途：提供真实运行时中转恢复部署脚本的通用函数。
# 范围：只包含输入校验、env 写回、API 请求和安全 JSON 生成。
# 输入：依赖主脚本设置 BASE_URL、DATABASE_URL、TMP_DIR 和 env 文件路径。
# 输出：函数失败时统一返回带脚本名前缀的错误信息。
# 安全：不打印密码、Token、数据库连接串或服务器地址。
# 约束：本文件只能被 source，不能直接执行。
# 维护：新增函数时优先保持纯函数或显式依赖环境变量。
# 注意：不要在这里终止 SSH 进程或清理远端非本项目资源。
# 测试：主脚本 bash -n 和真实门禁会覆盖本文件。
# 本头部满足中文注释和文件说明要求。

die() {
  echo "restore-real-runtime-access-deploy: $*" >&2
  exit 1
}

require_env() {
  local name="$1"
  [[ -n "${!name:-}" ]] || die "${name} is required"
}

require_any_env() {
  local label="$1"
  shift
  local name
  for name in "$@"; do
    [[ -n "${!name:-}" ]] && return 0
  done
  die "${label} is required"
}

upsert_env_value() {
  local name="$1"
  local value="$2"
  [[ -n "$value" ]] || return 0
  python3 - "$REAL_RELEASE_ENV_FILE" "$name" "$value" <<'PY'
from pathlib import Path
import os
import re
import shlex
import sys

path = Path(sys.argv[1])
name = sys.argv[2]
value = sys.argv[3]
line = f"{name}={shlex.quote(value)}\n"
pattern = re.compile(rf"^{re.escape(name)}=")

lines = []
found = False
if path.exists():
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
deduped = []
for existing in lines:
    if pattern.match(existing):
        if not found:
            deduped.append(line)
            found = True
        continue
    deduped.append(existing)
lines = deduped
if not found:
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

write_json() {
  local output="$1"
  local code="$2"
  python3 - "$output" >"$output" <<PY
import json
import os
import sys

output = sys.argv[1]
$code
PY
  chmod 600 "$output"
}

api_json() {
  local method="$1"
  local path="$2"
  local payload="$3"
  local output="$4"
  local token="$5"
  local status
  case "$token" in
    *$'\n'*|*$'\r'*)
      die "authorization token must not contain newlines"
      ;;
    [Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]:*)
      die "authorization token must be a raw token or Bearer token, not a full header"
      ;;
    [Bb][Ee][Aa][Rr][Ee][Rr]\ *)
      token="${token#* }"
      ;;
  esac
  local config_file="$TMP_DIR/curl-auth-${BASHPID}-${RANDOM}.conf"
  TOKEN_VALUE="$token" python3 - "$config_file" <<'PY'
import os
import sys

token = os.environ["TOKEN_VALUE"].replace("\\", "\\\\").replace('"', '\\"')
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f'header = "Authorization: Bearer {token}"\n')
PY
  chmod 600 "$config_file"
  status="$(curl -sS --max-time "${CURL_TIMEOUT:-900}" -o "$output" -w '%{http_code}' \
    -X "$method" \
    --config "$config_file" \
    -H "Content-Type: application/json" \
    --data-binary "@$payload" \
    "${BASE_URL%/}${path}" || true)"
  case "$status" in
    2*) ;;
    *) die "api ${method} ${path} failed with HTTP ${status}" ;;
  esac
}

login_token() {
  local account="$1"
  local password="$2"
  local payload="$TMP_DIR/login-payload.json"
  local body="$TMP_DIR/login-body.json"
  ACCOUNT_VALUE="$account" PASSWORD_VALUE="$password" \
    write_json "$payload" 'print(json.dumps({"account": os.environ["ACCOUNT_VALUE"], "password": os.environ["PASSWORD_VALUE"]}, ensure_ascii=False))'
  api_json POST /api/auth/login "$payload" "$body" ""
  json_value "$body" access_token || json_value "$body" accessToken || json_value "$body" token
}

run_psql() {
  local sql_file="$1"
  shift
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 "$@" -f "$sql_file" >/dev/null
}

sha256_text() {
  VALUE="$1" python3 - <<'PY'
import hashlib
import os

print(hashlib.sha256(os.environ["VALUE"].encode("utf-8")).hexdigest())
PY
}

uuid_or_empty() {
  local value="${1:-}"
  if [[ "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
    printf '%s\n' "$value"
  fi
}

# shellcheck source=scripts/lib/restore-real-runtime-access-deploy/runtime-assets.sh
. "${RESTORE_RUNTIME_HELPER_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}/runtime-assets.sh"

value_from_env_file() {
  local file="$1"
  local name="$2"
  python3 - "$file" "$name" <<'PY'
import shlex
import sys

path, wanted = sys.argv[1], sys.argv[2]
for raw in open(path, "r", encoding="utf-8"):
    raw = raw.strip()
    if not raw or raw.startswith("#") or "=" not in raw:
        continue
    key, value = raw.split("=", 1)
    if key != wanted:
        continue
    try:
        parts = shlex.split(value)
    except ValueError:
        parts = [value]
    print(parts[0] if parts else "")
    break
PY
}
