# 该文件为 ops-mistake-recovery-uat.sh 提供公共函数。
# 这里集中数据库、HTTP、payload、清理和断言逻辑。
# 主脚本只保留 UAT 编排步骤，便于控制行数。

uuid() {
  python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
}

psql_db() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" "$@"
}

psql_scalar() {
  psql_db -XAtq -v ON_ERROR_STOP=1 "$@"
}

curl_common() {
  curl --silent --show-error --location --max-time "${CURL_TIMEOUT:-30}" "$@"
}

status_is_2xx() {
  [[ "$1" == 2* ]]
}

require_2xx() {
  local label="$1"
  local status="$2"
  if status_is_2xx "$status"; then
    echo "ops_mistake_recovery_uat: ${label}_ok"
    return
  fi
  echo "ops_mistake_recovery_uat: ${label} failed with HTTP ${status}; response redacted" >&2
  exit 1
}

api_json() {
  local method="$1"
  local path="$2"
  local payload_file="$3"
  local body_file="$4"
  local status_file="$5"
  local config_file="$tmp_dir/curl-auth-${BASHPID}-${RANDOM}.conf"
  ADMIN_TOKEN_VALUE="$ADMIN_ACCESS_TOKEN" python3 - "$config_file" <<'PY'
import os
import sys

token = os.environ["ADMIN_TOKEN_VALUE"].replace("\\", "\\\\").replace('"', '\\"')
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f'header = "Authorization: Bearer {token}"\n')
PY
  chmod 600 "$config_file"
  local args=(
    --request "$method"
    --config "$config_file"
    --output "$body_file"
    --write-out "%{http_code}"
  )
  if [[ -n "$payload_file" ]]; then
    args+=(--header "Content-Type: application/json" --data-binary "@${payload_file}")
  fi
  curl_common "${args[@]}" "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
}

api_expect_2xx() {
  local label="$1"
  local method="$2"
  local path="$3"
  local payload_file="${4:-}"
  api_json "$method" "$path" "$payload_file" "$tmp_dir/api-${label}.body" "$tmp_dir/api-${label}.status"
  require_2xx "$label" "$(cat "$tmp_dir/api-${label}.status")"
}

write_admin_login_payload() {
  local out_file="$1"
  ADMIN_LOGIN_ACCOUNT_VALUE="$ADMIN_LOGIN_ACCOUNT" ADMIN_LOGIN_PASSWORD_VALUE="$ADMIN_LOGIN_PASSWORD" \
    python3 - "$out_file" <<'PY'
import json
import os
import sys

payload = {
    "account": os.environ["ADMIN_LOGIN_ACCOUNT_VALUE"],
    "password": os.environ["ADMIN_LOGIN_PASSWORD_VALUE"],
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(payload, fh, separators=(",", ":"))
PY
}

extract_access_token() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
token = payload.get("data", {}).get("access_token", "")
if token:
    print(token)
PY
}

derive_admin_token_if_needed() {
  if [[ -n "$ADMIN_ACCESS_TOKEN" && ( -z "$ADMIN_LOGIN_ACCOUNT" || -z "$ADMIN_LOGIN_PASSWORD" ) ]]; then
    return
  fi
  local payload="$tmp_dir/admin-login.json"
  write_admin_login_payload "$payload"
  api_json "POST" "/api/auth/login" "$payload" "$tmp_dir/admin-login.body" "$tmp_dir/admin-login.status"
  require_2xx "admin_login" "$(cat "$tmp_dir/admin-login.status")"
  ADMIN_ACCESS_TOKEN="$(extract_access_token "$tmp_dir/admin-login.body" || true)"
  if [[ -z "$ADMIN_ACCESS_TOKEN" ]]; then
    echo "ops_mistake_recovery_uat: admin login response did not include access token; response redacted" >&2
    exit 1
  fi
}

write_members_payload() {
  local out_file="$1"
  local primary_status="$2"
  local primary_allow="$3"
  local backup_status="$4"
  local backup_allow="$5"
  PRIMARY_ENDPOINT_ID="$primary_endpoint_id" BACKUP_ENDPOINT_ID="$backup_endpoint_id" \
  PRIMARY_STATUS="$primary_status" PRIMARY_ALLOW="$primary_allow" \
  BACKUP_STATUS="$backup_status" BACKUP_ALLOW="$backup_allow" \
    python3 - "$out_file" <<'PY'
import json
import os
import sys

def enabled(name: str) -> bool:
    return os.environ[name] in {"1", "true", "TRUE", "yes", "YES", "on", "ON"}

payload = {
    "members": [
        {
            "exit_endpoint_id": os.environ["PRIMARY_ENDPOINT_ID"],
            "weight": 100,
            "priority": 200,
            "status": os.environ["PRIMARY_STATUS"],
            "allow_new_assignments": enabled("PRIMARY_ALLOW"),
        },
        {
            "exit_endpoint_id": os.environ["BACKUP_ENDPOINT_ID"],
            "weight": 100,
            "priority": 100,
            "status": os.environ["BACKUP_STATUS"],
            "allow_new_assignments": enabled("BACKUP_ALLOW"),
        },
    ]
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(payload, fh, separators=(",", ":"))
PY
}


cleanup() {
  local status=$?
  if ! xrayc_real_e2e_bool_is_true "${OPS_MISTAKE_RECOVERY_UAT_KEEP_DATA:-0}"; then
    psql_db -Xq -v ON_ERROR_STOP=1 \
      -v run_id="$run_id" \
      -v main_user_id="$main_user_id" -v delete_user_id="$delete_user_id" \
      -v plan_id="$plan_id" -v delete_plan_id="$delete_plan_id" \
      -v node_id="$node_id" -v pool_id="$pool_id" -v resource_id="$resource_id" \
      -v primary_endpoint_id="$primary_endpoint_id" -v backup_endpoint_id="$backup_endpoint_id" \
      -v line_id="$line_id" -v group_id="$group_id" <<'SQL' >/dev/null 2>&1 || true
BEGIN;
DELETE FROM usage_ledgers WHERE user_id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM audit_logs
WHERE action LIKE 'ops_mistake_recovery_uat.%'
  AND request_summary->>'run_id' = :'run_id';
DELETE FROM access_exit_probe_states WHERE access_node_id = :'node_id'::uuid OR exit_endpoint_id IN (:'primary_endpoint_id'::uuid, :'backup_endpoint_id'::uuid);
DELETE FROM access_exit_probes WHERE access_node_id = :'node_id'::uuid OR exit_endpoint_id IN (:'primary_endpoint_id'::uuid, :'backup_endpoint_id'::uuid);
DELETE FROM user_exit_assignments WHERE user_id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM user_access_line_assignments WHERE user_id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM subscription_tokens WHERE user_id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM user_subscriptions WHERE user_id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM users WHERE id IN (:'main_user_id'::uuid, :'delete_user_id'::uuid);
DELETE FROM plan_line_groups WHERE plan_id IN (:'plan_id'::uuid, :'delete_plan_id'::uuid) OR line_group_id = :'group_id'::uuid;
DELETE FROM line_group_exit_endpoints WHERE line_group_id = :'group_id'::uuid
  OR exit_endpoint_id IN (:'primary_endpoint_id'::uuid, :'backup_endpoint_id'::uuid);
DELETE FROM access_lines WHERE id = :'line_id'::uuid;
DELETE FROM line_groups WHERE id = :'group_id'::uuid;
DELETE FROM exit_pool_members WHERE exit_pool_id = :'pool_id'::uuid OR exit_endpoint_id IN (:'primary_endpoint_id'::uuid, :'backup_endpoint_id'::uuid);
DELETE FROM exit_endpoints WHERE id IN (:'primary_endpoint_id'::uuid, :'backup_endpoint_id'::uuid);
DELETE FROM exit_resources WHERE id = :'resource_id'::uuid;
DELETE FROM exit_pools WHERE id = :'pool_id'::uuid;
DELETE FROM plans WHERE id IN (:'plan_id'::uuid, :'delete_plan_id'::uuid);
DELETE FROM access_nodes WHERE id = :'node_id'::uuid;
COMMIT;
SQL
  fi
  rm -rf "$tmp_dir"
  exit "$status"
}

require_eq() {
  if [[ "$1" != "$2" ]]; then
    echo "$3" >&2
    exit 1
  fi
}

require_count() {
  if [[ ! "$1" =~ ^[0-9]+$ || "$1" -lt 1 ]]; then
    echo "$2" >&2
    exit 1
  fi
}

fetch_subscription() {
  xrayc_real_e2e_fetch_sensitive_url_to_file "${BASE_URL%/}/sub/${sub_token}" "$1" "subscription download failed"
}

fetch_subscription_optional() {
  local output_file="$1"
  local tmp_curl_dir config_file status_file http_status

  tmp_curl_dir="$(mktemp -d)"
  config_file="${tmp_curl_dir}/curl-url.conf"
  status_file="${tmp_curl_dir}/curl.status"
  xrayc_real_e2e_write_curl_url_config "${BASE_URL%/}/sub/${sub_token}" "$config_file"
  if ! curl --silent --location --max-time "${CURL_TIMEOUT:-30}" \
    --config "$config_file" \
    --output "$output_file" \
    --write-out "%{http_code}" > "$status_file" 2>/dev/null; then
    rm -rf "$tmp_curl_dir"
    echo "subscription availability check failed; raw curl output redacted" >&2
    exit 1
  fi
  http_status="$(cat "$status_file")"
  rm -rf "$tmp_curl_dir"
  case "$http_status" in
    2*) return 0 ;;
    4*|5*)
      : > "$output_file"
      return 1
      ;;
    *)
      echo "subscription availability check returned unexpected HTTP status" >&2
      exit 1
      ;;
  esac
}

assert_redacted() {
  xrayc_real_e2e_assert_subscription_missing_literal "$1" "$primary_host" "subscription leaked primary exit host"
  xrayc_real_e2e_assert_subscription_missing_literal "$1" "$backup_host" "subscription leaked backup exit host"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$1"
}

assert_has_line() {
  grep -Fq -- "$line_name" "$1" || { echo "subscription is missing temporary access line" >&2; exit 1; }
  grep -Fq -- "$line_host" "$1" || { echo "subscription is missing temporary ingress host" >&2; exit 1; }
  assert_redacted "$1"
}

assert_missing_line() {
  if grep -Fq -- "$pool_name" "$1" || grep -Fq -- "$line_host" "$1"; then
    echo "subscription still exposes unavailable temporary access line" >&2
    exit 1
  fi
  assert_redacted "$1"
}

assignment_count() {
  psql_scalar -v user_id="$main_user_id" -v line_id="$line_id" -v pool_id="$pool_id" <<'SQL'
SELECT COUNT(*)::text
FROM user_access_line_assignments ula
JOIN user_exit_assignments uea ON uea.user_id = ula.user_id AND uea.access_line_id = ula.access_line_id
WHERE ula.user_id = :'user_id'::uuid AND ula.access_line_id = :'line_id'::uuid AND uea.exit_pool_id = :'pool_id'::uuid;
SQL
}

assigned_endpoint() {
  psql_scalar -v user_id="$main_user_id" -v line_id="$line_id" -v pool_id="$pool_id" <<'SQL'
SELECT COALESCE(exit_endpoint_id::text, '')
FROM user_exit_assignments
WHERE user_id = :'user_id'::uuid AND access_line_id = :'line_id'::uuid AND exit_pool_id = :'pool_id'::uuid
LIMIT 1;
SQL
}

dirty_reason() {
  psql_scalar -v node_id="$node_id" <<'SQL'
SELECT config_dirty_reason FROM access_nodes WHERE id = :'node_id'::uuid;
SQL
}
