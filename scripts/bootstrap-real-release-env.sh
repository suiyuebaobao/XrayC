#!/usr/bin/env bash
# 用途：生成真实发布环境的私有 .env.real-release 草稿。
# 说明：主流程编排私有数据源、调用 helper，并避免在终端输出敏感值。
set -euo pipefail
IFS=$'\n\t'

if [[ $- == *x* ]]; then
  set +x
  echo "bootstrap-real-release-env: disabled shell xtrace to avoid printing private values" >&2
fi

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
HELPER_DIR="$BASE_DIR/scripts/lib/bootstrap-real-release-env"
# shellcheck disable=SC1091
. "$HELPER_DIR/functions.sh"

OUTPUT_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
INVENTORY_FILE="${XRAYC_REAL_E2E_INVENTORY:-文档/私有/remote-e2e-inventory.json}"
DEPLOY_TOKEN_FILE="${XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE:-文档/私有/deploy-artifact-token.env}"
BASE_URL_VALUE="${BASE_URL:-http://127.0.0.1:8080}"
REQUESTED_BASE_URL="${BASE_URL:-}"
SMOKE_LOGIN_ACCOUNT="${SMOKE_LOGIN_ACCOUNT:-demo@example.test}"
SMOKE_LOGIN_PASSWORD="${SMOKE_LOGIN_PASSWORD:-demo123456}"
SMOKE_ADMIN_ACCOUNT="${SMOKE_ADMIN_ACCOUNT:-admin@example.test}"
SMOKE_ADMIN_PASSWORD="${SMOKE_ADMIN_PASSWORD:-admin123456}"
OVERWRITE=0

# 真实发布环境文件包含私有地址和令牌，创建过程默认只允许当前用户读取。
umask 077

usage() {
  cat <<'EOF'
usage: bash scripts/bootstrap-real-release-env.sh [--overwrite]

Create a private .env.real-release draft from local/private sources. The script
does not print secret values, tokens, raw proxy URLs, passwords, IPs, or
subscription URLs; it only prints variable-name level statistics.

If DATABASE_URL is already provided in the environment, the script uses it for
read-only endpoint id/host discovery before falling back to local Compose.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --overwrite) OVERWRITE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done

if [[ -e "$OUTPUT_FILE" && "$OVERWRITE" != "1" ]]; then
  echo "bootstrap-real-release-env: output file exists; use --overwrite" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
values_file="$tmp_dir/values.env"
db_query_file="$tmp_dir/db-discovery.sql"
touch "$values_file"

write_db_discovery_query "$db_query_file"

if [[ -f "$INVENTORY_FILE" ]]; then
  python3 - "$INVENTORY_FILE" > "$tmp_dir/inventory.env" <<'PY'
import json, shlex, sys
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = payload.get("targets") if isinstance(payload, dict) else None
target = targets[0] if isinstance(targets, list) and targets else {}
for env_name, key in {
    "XRAYC_REAL_E2E_TARGET": "alias",
    "BASE_URL": "control_plane_url",
    "DEPLOY_ARTIFACT_TOKEN": "deploy_artifact_token",
    "ACCESS_NODE_ID": "node_id",
    "AGENT_TOKEN": "node_token",
}.items():
    value = str(target.get(key, "")).strip()
    if value:
        print(f"{env_name}={shlex.quote(value)}")
PY
  # shellcheck disable=SC1090
  . "$tmp_dir/inventory.env"
fi

if [[ -f "$DEPLOY_TOKEN_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$DEPLOY_TOKEN_FILE"
  set +a
fi

EFFECTIVE_BASE_URL="${REQUESTED_BASE_URL:-${BASE_URL:-$BASE_URL_VALUE}}"
set_value BASE_URL "$EFFECTIVE_BASE_URL"
set_value XRAYC_ENV "production"
set_value SEED_DEMO_DATA "false"
set_value JWT_SECRET "${JWT_SECRET:-}"
set_value JWT_EXPIRES_IN "${JWT_EXPIRES_IN:-30m}"
set_value JWT_REFRESH_EXPIRES_IN "${JWT_REFRESH_EXPIRES_IN:-7d}"
case "$EFFECTIVE_BASE_URL" in
  http://127.0.0.1:*|http://127.0.0.1/*|http://localhost:*|http://localhost/*)
    set_value XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP "1"
    ;;
esac
set_value DEPLOY_ARTIFACT_TOKEN "${DEPLOY_ARTIFACT_TOKEN:-}"
set_value AGENT_TOKEN "${AGENT_TOKEN:-}"
set_value ACCESS_NODE_ID "${ACCESS_NODE_ID:-}"
set_value XRAYC_REAL_E2E_INVENTORY "$INVENTORY_FILE"
set_value XRAYC_REAL_E2E_TARGET "${XRAYC_REAL_E2E_TARGET:-}"
set_value DATABASE_URL "${DATABASE_URL:-}"
set_value E2E_USER_ACCOUNT "${E2E_USER_ACCOUNT:-$SMOKE_LOGIN_ACCOUNT}"
set_value E2E_USER_PASSWORD "${E2E_USER_PASSWORD:-$SMOKE_LOGIN_PASSWORD}"
set_value E2E_ADMIN_ACCOUNT "${E2E_ADMIN_ACCOUNT:-$SMOKE_ADMIN_ACCOUNT}"
set_value E2E_ADMIN_PASSWORD "${E2E_ADMIN_PASSWORD:-$SMOKE_ADMIN_PASSWORD}"

database_discovery_done=0
if [[ -n "${DATABASE_URL:-}" ]]; then
  if discover_database_from_url "DATABASE_URL" "$DATABASE_URL"; then
    database_discovery_done=1
  fi
fi

if docker compose ps --status running --services 2>/dev/null | grep -Fxq postgres; then
  compose_postgres_user="${POSTGRES_USER:-xrayc}"
  compose_postgres_db="${POSTGRES_DB:-xrayc}"
  postgres_port="$(docker compose port postgres 5432 2>/dev/null | tail -n 1 | sed -E 's/.*:([0-9]+)$/\1/' || true)"
  if [[ -n "$postgres_port" ]]; then
    docker compose exec -T postgres sh -c 'printf "%s\n%s\n%s\n" "$POSTGRES_USER" "$POSTGRES_PASSWORD" "$POSTGRES_DB"' > "$tmp_dir/postgres-env.txt" 2>/dev/null || true
    if [[ -s "$tmp_dir/postgres-env.txt" ]]; then
      mapfile -t postgres_env < "$tmp_dir/postgres-env.txt"
      if [[ "${#postgres_env[@]}" -ge 3 && -n "${postgres_env[0]}" && -n "${postgres_env[1]}" && -n "${postgres_env[2]}" ]]; then
        compose_postgres_user="${postgres_env[0]}"
        compose_postgres_db="${postgres_env[2]}"
        database_url_scheme="postgres"
        database_url_value="${database_url_scheme}://${postgres_env[0]}:${postgres_env[1]}@127.0.0.1:${postgres_port}/${postgres_env[2]}"
        set_value_or_report_placeholder DATABASE_URL "$database_url_value" "local compose"
      fi
    fi
  fi
  if [[ "$database_discovery_done" != "1" ]]; then
    discover_database_from_compose "$compose_postgres_user" "$compose_postgres_db" || true
  fi
fi

login_body="$tmp_dir/login.json"
login_status="$tmp_dir/login.status"
login_request="$tmp_dir/login-request.json"
login_account_file="$tmp_dir/login-account.txt"
login_password_file="$tmp_dir/login-password.txt"
printf '%s' "$SMOKE_LOGIN_ACCOUNT" > "$login_account_file"
printf '%s' "$SMOKE_LOGIN_PASSWORD" > "$login_password_file"
python3 - "$login_account_file" "$login_password_file" > "$login_request" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as account_file:
    account = account_file.read()
with open(sys.argv[2], "r", encoding="utf-8") as password_file:
    password = password_file.read()
json.dump({"account": account, "password": password}, sys.stdout)
PY
if curl --silent --location --max-time "${CURL_TIMEOUT:-15}" \
  --header "Content-Type: application/json" \
  --data-binary "@$login_request" \
  --output "$login_body" \
  --write-out "%{http_code}" \
  "${EFFECTIVE_BASE_URL%/}/api/auth/login" > "$login_status" 2>/dev/null; then
  if [[ "$(cat "$login_status")" == 2* ]]; then
    user_token="$(python3 - "$login_body" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
print(str(payload.get("data", payload).get("access_token", "")).strip())
PY
)"
    set_value USER_ACCESS_TOKEN "$user_token"
  fi
fi

discover_expected_access_servers_from_subscription

set_placeholder DATABASE_URL "postgres://<user>:<password>@<host>:<port>/<database>"
compose_database_url="$(
  python3 - "$values_file" <<'PY' 2>/dev/null || true
import shlex
import sys
from urllib.parse import urlsplit, urlunsplit

values = {}
for raw in open(sys.argv[1], "r", encoding="utf-8"):
    raw = raw.strip()
    if not raw or raw.startswith("#") or "=" not in raw:
        continue
    key, value = raw.split("=", 1)
    try:
        parts = shlex.split(value)
        decoded = parts[0] if parts else ""
    except ValueError:
        decoded = value
    values[key] = decoded

source = values.get("DATABASE_URL", "")
if not source or "<" in source or ">" in source:
    raise SystemExit(0)
parsed = urlsplit(source)
if not parsed.scheme.startswith("postgres") or not parsed.hostname:
    raise SystemExit(0)
userinfo = ""
if parsed.username:
    userinfo += parsed.username
    if parsed.password is not None:
        userinfo += ":" + parsed.password
    userinfo += "@"
print(urlunsplit((parsed.scheme, f"{userinfo}postgres:5432", parsed.path, parsed.query, parsed.fragment)))
PY
)"
set_value XRAYC_REAL_RELEASE_COMPOSE_DATABASE_URL "$compose_database_url"
set_placeholder JWT_SECRET "<copy-from-private-kms-or-secret-store>"
set_placeholder ACCESS_LINE_ID "<access-line-uuid>"
set_placeholder EXIT_ENDPOINT_ID "<exit-endpoint-uuid>"
set_placeholder EXPECTED_ACCESS_SERVERS "<access-host:port>"
set_placeholder CLIENT_PROXY_URL "<copy-from-client-runtime>"
set_placeholder EXPECTED_EXIT_IP "<copy-from-client-runtime>"
for suffix in SOCKS HTTP VLESS TROJAN SHADOWSOCKS HY2; do
  set_placeholder "CLIENT_PROXY_URL_${suffix}" "<copy-from-client-runtime>"
  set_placeholder "EXPECTED_EXIT_IP_${suffix}" "<copy-from-client-runtime>"
done
set_placeholder THIRD_PARTY_SOCKS_HOST "<socks-upstream-host>"
set_placeholder THIRD_PARTY_SOCKS_RAW_URL "<copy-from-provider-console>"
set_placeholder EXIT_ENDPOINT_ID_SOCKS "<socks-exit-endpoint-uuid>"
set_placeholder THIRD_PARTY_HTTP_HOST "<http-upstream-host>"
set_placeholder THIRD_PARTY_HTTP_RAW_URL "<copy-from-provider-console>"
set_placeholder EXIT_ENDPOINT_ID_HTTP "<http-exit-endpoint-uuid>"
set_placeholder THIRD_PARTY_HOST "<vless-upstream-host>"
set_placeholder THIRD_PARTY_VLESS_HOST "<vless-upstream-host>"
set_placeholder THIRD_PARTY_VLESS_RAW_URL "<copy-from-provider-console>"
set_placeholder EXIT_ENDPOINT_ID_VLESS "<vless-exit-endpoint-uuid>"
set_placeholder THIRD_PARTY_TROJAN_HOST "<trojan-upstream-host>"
set_placeholder THIRD_PARTY_TROJAN_RAW_URL "<copy-from-provider-console>"
set_placeholder THIRD_PARTY_TROJAN_SECURITY "tls"
set_placeholder THIRD_PARTY_TROJAN_SNI "<trojan-sni>"
set_placeholder EXIT_ENDPOINT_ID_TROJAN "<trojan-exit-endpoint-uuid>"
set_placeholder THIRD_PARTY_SHADOWSOCKS_HOST "<shadowsocks-upstream-host>"
set_placeholder THIRD_PARTY_SHADOWSOCKS_RAW_URL "<copy-from-provider-console>"
set_placeholder EXIT_ENDPOINT_ID_SHADOWSOCKS "<shadowsocks-exit-endpoint-uuid>"
set_placeholder THIRD_PARTY_HY2_HOST "<hy2-upstream-host>"
set_placeholder THIRD_PARTY_HY2_RAW_URL "<copy-from-provider-console>"
set_placeholder EXIT_ENDPOINT_ID_HY2 "<hy2-exit-endpoint-uuid>"
set_placeholder REAL_RUNTIME_OBSERVATION_TIMEOUT_SECONDS "180"
set_placeholder REAL_RUNTIME_OBSERVATION_POLL_INTERVAL_SECONDS "5"

{
  echo "# Private real release env generated by bootstrap-real-release-env.sh."
  echo "# Do not commit. Values are intentionally not printed by the generator."
  sort -u "$values_file"
} > "$OUTPUT_FILE"
chmod 600 "$OUTPUT_FILE"

filled_count="$(count_filled_variables "$OUTPUT_FILE")"
placeholder_count="$(grep -c '<' "$OUTPUT_FILE" || true)"
echo "bootstrap-real-release-env: wrote private env draft"
echo "bootstrap-real-release-env: filled variables=${filled_count}"
echo "bootstrap-real-release-env: placeholder variables=${placeholder_count}"
echo "bootstrap-real-release-env: run make real-release-gap-report for a grouped checklist"
echo "bootstrap-real-release-env: run make check-real-release-env before the real release gate"
