# 该 helper 为 real-auth-ha-uat.sh 提供认证 UAT 业务流程函数。
# 内容包括 JSON payload 生成、管理员登录派生、认证安全配置和恢复辅助。
# 内容还包括测试用户数据库清理、邮箱验证码造数和高可用认证场景执行。
# 文件只定义函数，依赖主脚本初始化测试邮箱、临时目录和认证配置变量。
# 这些函数从主脚本拆出，避免主脚本过长并保持敏感输出继续被遮蔽。

extract_data_json() {
  local input_file="$1"
  local output_file="$2"
  python3 - "$input_file" "$output_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload) if isinstance(payload, dict) else payload
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(data, fh, separators=(",", ":"))
PY
}

make_test_security() {
  local email_enabled="$1"
  python3 - "$test_security_file" "$UAT_TEST_EMAIL_DOMAIN" "$email_enabled" <<'PY'
import json
import sys

payload = {
    "require_invite_code": False,
    "allow_user_invite_generation": False,
    "max_invite_codes_per_user": 0,
    "captcha": {
        "register_enabled": True,
        "user_login_enabled": True,
        "admin_login_enabled": True,
        "ttl_seconds": 60,
    },
    "email_verification": {
        "enabled": sys.argv[3] == "1",
        "allowed_domains": [sys.argv[2]],
        "cooldown_seconds": 60,
        "smtp_host": "",
        "smtp_port": 587,
        "smtp_username": "",
        "smtp_from": "",
        "smtp_password": "***",
    },
    "login_guard": {
        "enabled": True,
        "failure_threshold": 2,
        "lock_minutes": 5,
    },
}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(payload, fh, separators=(",", ":"))
PY
}

captcha_payload() {
  local scene="$1"
  local target="$2"
  local out_file="$3"
  local body="$tmp_dir/captcha-${scene}.body"
  local status_file="$tmp_dir/captcha-${scene}.status"
  local encoded_target
  encoded_target="$(python3 - "$target" <<'PY'
import sys
from urllib.parse import quote
print(quote(sys.argv[1], safe=""))
PY
)"
  get_public "/api/auth/captcha?scene=${scene}&target=${encoded_target}" "$body" "$status_file"
  assert_2xx "captcha ${scene}" "$(cat "$status_file")"
  python3 - "$body" "$out_file" <<'PY'
import json
import re
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", {})
question = str(data.get("question", ""))
match = re.search(r"(-?\d+)\s*([+\-*xX])\s*(-?\d+)", question)
if not match:
    raise SystemExit("captcha question format unsupported")
left = int(match.group(1))
op = match.group(2).lower()
right = int(match.group(3))
if op == "+":
    answer = left + right
elif op == "-":
    answer = left - right
else:
    answer = left * right
result = {"id": data.get("id", ""), "answer": str(answer)}
if not result["id"]:
    raise SystemExit("captcha id missing")
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(result, fh, separators=(",", ":"))
PY
}

make_register_payload() {
  local captcha_file="$1"
  local out_file="$2"
  local email_code_id="${3:-}"
  local email_code="${4:-}"
  TEST_EMAIL_VALUE="$test_email" TEST_PASSWORD_VALUE="$test_password" \
  EMAIL_CODE_ID_VALUE="$email_code_id" EMAIL_CODE_VALUE="$email_code" \
    python3 - "$captcha_file" "$out_file" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    captcha = json.load(fh)
payload = {
    "email": os.environ["TEST_EMAIL_VALUE"],
    "password": os.environ["TEST_PASSWORD_VALUE"],
    "captcha_id": captcha["id"],
    "captcha_answer": captcha["answer"],
}
if os.environ.get("EMAIL_CODE_ID_VALUE"):
    payload["email_code_id"] = os.environ["EMAIL_CODE_ID_VALUE"]
    payload["email_code"] = os.environ["EMAIL_CODE_VALUE"]
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(payload, fh, separators=(",", ":"))
PY
}

make_email_code_payload() {
  local email="$1"
  local out_file="$2"
  EMAIL_VALUE="$email" python3 - "$out_file" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump({"email": os.environ["EMAIL_VALUE"]}, fh, separators=(",", ":"))
PY
}

make_login_payload() {
  local password="$1"
  local captcha_file="$2"
  local out_file="$3"
  TEST_EMAIL_VALUE="$test_email" TEST_PASSWORD_VALUE="$password" \
    python3 - "$captcha_file" "$out_file" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    captcha = json.load(fh)
payload = {
    "account": os.environ["TEST_EMAIL_VALUE"],
    "password": os.environ["TEST_PASSWORD_VALUE"],
    "captcha_id": captcha["id"],
    "captcha_answer": captcha["answer"],
}
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    json.dump(payload, fh, separators=(",", ":"))
PY
}

make_admin_login_payload() {
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
  local body_file="$1"
  python3 - "$body_file" <<'PY'
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
  if [[ -n "$ADMIN_ACCESS_TOKEN" ]]; then
    return
  fi
  local payload="$tmp_dir/admin-login.json"
  make_admin_login_payload "$payload"
  post_json "/api/auth/login" "$payload" "$tmp_dir/admin-login.body" "$tmp_dir/admin-login.status"
  assert_2xx "derive admin access token" "$(cat "$tmp_dir/admin-login.status")"
  ADMIN_ACCESS_TOKEN="$(extract_access_token "$tmp_dir/admin-login.body" || true)"
  if [[ -z "$ADMIN_ACCESS_TOKEN" ]]; then
    echo "derive admin access token: response did not contain token; response redacted" >&2
    exit 1
  fi
}

cleanup_test_data() {
  if [[ -z "${UAT_PSQL_DATABASE_URL:-${DATABASE_URL:-}}" ]] || ! command -v psql >/dev/null 2>&1; then
    return
  fi
  xrayc_real_e2e_psql_database_url "${UAT_PSQL_DATABASE_URL:-$DATABASE_URL}" -v ON_ERROR_STOP=1 -q >/dev/null 2>&1 <<SQL
DELETE FROM auth_challenges
WHERE target_hash IN (
  encode(digest('register:' || lower('${test_email}'), 'sha256'), 'hex'),
  encode(digest('login:' || lower('${test_email}'), 'sha256'), 'hex'),
  encode(digest('register_email:' || lower('${test_email}'), 'sha256'), 'hex')
);
DELETE FROM login_guard_states
WHERE guard_key = 'login:' || encode(digest(lower('${test_email}'), 'sha256'), 'hex');
DELETE FROM users WHERE email = '${test_email}';
SQL
}

configure_auth_security() {
  local email_enabled="${1:-0}"
  if [[ "$restore_needed" -eq 0 ]]; then
    get_admin_json "read auth security" "/api/admin/auth-security" \
      "$tmp_dir/current-security.body" "$tmp_dir/current-security.status"
    extract_data_json "$tmp_dir/current-security.body" "$original_security_file"
  fi
  make_test_security "$email_enabled"
  put_admin_json "apply UAT auth security" "/api/admin/auth-security" "$test_security_file" \
    "$tmp_dir/apply-security.body" "$tmp_dir/apply-security.status"
  restore_needed=1
}

create_email_challenge() {
  local email="$1"
  local code="$2"
  xrayc_real_e2e_psql_database_url "${UAT_PSQL_DATABASE_URL:-$DATABASE_URL}" -XAtq -v ON_ERROR_STOP=1 \
    -v email="$email" -v code="$code" <<'SQL'
WITH challenge AS (
  SELECT gen_random_uuid() AS id
),
target AS (
  SELECT encode(digest('register_email:' || lower(:'email'), 'sha256'), 'hex') AS target_hash
  FROM challenge
)
INSERT INTO auth_challenges (id, scene, target_hash, code_hash, expires_at)
SELECT challenge.id,
       'register_email',
       target.target_hash,
       encode(digest(challenge.id::text || ':' || lower(:'code'), 'sha256'), 'hex'),
       now() + interval '60 seconds'
FROM challenge, target
RETURNING id::text;
SQL
}

run_email_verification_uat() {
  local email_payload="$tmp_dir/email-code.json"
  make_email_code_payload "$test_email" "$email_payload"

  post_json "/api/auth/email-code" "$email_payload" "$tmp_dir/email-code-disabled.body" "$tmp_dir/email-code-disabled.status"
  assert_status "email code switch disabled" "403" "$(cat "$tmp_dir/email-code-disabled.status")"

  configure_auth_security 1
  post_json "/api/auth/email-code" "$email_payload" "$tmp_dir/email-code-smtp.body" "$tmp_dir/email-code-smtp.status"
  assert_status "email code enabled without smtp" "422" "$(cat "$tmp_dir/email-code-smtp.status")"
  uat_email_code_id="$(create_email_challenge "$test_email" "$uat_email_code")"
  post_json "/api/auth/email-code" "$email_payload" "$tmp_dir/email-code-cooldown.body" "$tmp_dir/email-code-cooldown.status"
  assert_status "email code cooldown shared" "429" "$(cat "$tmp_dir/email-code-cooldown.status")"
}

run_auth_uat() {
  local captcha_file="$tmp_dir/register-captcha.json"
  local register_payload="$tmp_dir/register.json"
  if [[ -z "$uat_email_code_id" ]]; then
    echo "email verification challenge was not prepared" >&2
    exit 1
  fi
  captcha_payload "register" "$test_email" "$captcha_file"
  make_register_payload "$captcha_file" "$register_payload" "$uat_email_code_id" "$uat_email_code"
  post_json "/api/auth/register" "$register_payload" "$tmp_dir/register.body" "$tmp_dir/register.status" \
    --cookie-jar "$tmp_dir/register.cookies"
  assert_2xx "register with one-time captcha and email code" "$(cat "$tmp_dir/register.status")"

  post_json "/api/auth/register" "$register_payload" "$tmp_dir/register-replay.body" "$tmp_dir/register-replay.status"
  assert_rejected "captcha/email code replay after register" "$(cat "$tmp_dir/register-replay.status")"

  local login_captcha="$tmp_dir/login-captcha.json"
  local login_payload="$tmp_dir/login.json"
  captcha_payload "login" "$test_email" "$login_captcha"
  make_login_payload "$test_password" "$login_captcha" "$login_payload"
  post_json "/api/auth/login" "$login_payload" "$tmp_dir/login.body" "$tmp_dir/login.status" \
    --cookie-jar "$tmp_dir/login.cookies"
  assert_2xx "login with captcha" "$(cat "$tmp_dir/login.status")"

  post_json "/api/auth/login" "$login_payload" "$tmp_dir/login-replay.body" "$tmp_dir/login-replay.status"
  assert_rejected "captcha replay after login" "$(cat "$tmp_dir/login-replay.status")"

  post_empty "/api/auth/refresh" "$tmp_dir/refresh-1.body" "$tmp_dir/refresh-1.status" \
    --cookie "$tmp_dir/login.cookies" \
    --cookie-jar "$tmp_dir/refresh.cookies"
  assert_2xx "refresh token rotation" "$(cat "$tmp_dir/refresh-1.status")"

  post_empty "/api/auth/refresh" "$tmp_dir/refresh-replay.body" "$tmp_dir/refresh-replay.status" \
    --cookie "$tmp_dir/login.cookies"
  assert_status "old refresh token replay" "401" "$(cat "$tmp_dir/refresh-replay.status")"

  local attempt
  for attempt in 1 2; do
    local fail_captcha="$tmp_dir/fail-captcha-${attempt}.json"
    local fail_payload="$tmp_dir/fail-login-${attempt}.json"
    captcha_payload "login" "$test_email" "$fail_captcha"
    make_login_payload "$wrong_password" "$fail_captcha" "$fail_payload"
    post_json "/api/auth/login" "$fail_payload" "$tmp_dir/fail-login-${attempt}.body" "$tmp_dir/fail-login-${attempt}.status"
    assert_status "login failure ${attempt}" "401" "$(cat "$tmp_dir/fail-login-${attempt}.status")"
  done

  local locked_captcha="$tmp_dir/locked-captcha.json"
  local locked_payload="$tmp_dir/locked-login.json"
  captcha_payload "login" "$test_email" "$locked_captcha"
  make_login_payload "$test_password" "$locked_captcha" "$locked_payload"
  post_json "/api/auth/login" "$locked_payload" "$tmp_dir/locked-login.body" "$tmp_dir/locked-login.status"
  assert_status "login lock rejects correct password" "429" "$(cat "$tmp_dir/locked-login.status")"
}

run_shared_rate_limit_uat() {
  local rate_target="auth-ha-rate-${run_id}@${UAT_TEST_EMAIL_DOMAIN}"
  local encoded_target
  encoded_target="$(python3 - "$rate_target" <<'PY'
import sys
from urllib.parse import quote
print(quote(sys.argv[1], safe=""))
PY
)"
  local status=""
  local attempt
  for attempt in $(seq 1 "$UAT_RATE_LIMIT_ATTEMPTS"); do
    get_public "/api/auth/captcha?scene=login&target=${encoded_target}" \
      "$tmp_dir/rate-${attempt}.body" "$tmp_dir/rate-${attempt}.status" \
      --header "X-Forwarded-For: 203.0.113.77"
    status="$(cat "$tmp_dir/rate-${attempt}.status")"
    if [[ "$status" == "429" ]]; then
      echo "shared auth rate limit: HTTP 429 after ${attempt} request(s)"
      return
    fi
  done
  echo "shared auth rate limit: expected HTTP 429 within configured attempts, last HTTP ${status}" >&2
  exit 1
}
