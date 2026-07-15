#!/usr/bin/env bash
# 此 helper 提供真实三节点完整性 E2E 的通用校验和私密 API 请求函数。
# 它由主脚本 source，不直接执行，避免输出密码、token、URL 等敏感值。

fail() {
  echo "real-access-3node-integrity-e2e: $*" >&2
  exit 1
}

die_usage() {
  echo "real-access-3node-integrity-e2e: $*" >&2
  exit 2
}

cleanup() {
  local status=$?
  if [[ "$RESTORE_ON_EXIT" == "1" ]]; then
    restore_account_state >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
  exit "$status"
}

lowercase() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

value_is_placeholder() {
  local value
  value="$(lowercase "${1:-}")"
  [[ -n "$value" ]] || return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-inventory-path* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *upstream-host* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *mock* ]] && return 0
  [[ "$value" == *fake* ]] && return 0
  [[ "$value" == *simulate* ]] && return 0
  [[ "$value" == *your-secret* ]] && return 0
  return 1
}

require_real_value() {
  local name="$1"
  local value="${2:-}"
  if value_is_placeholder "$value"; then
    die_usage "${name} must be set to a real private value"
  fi
}

require_real_env() {
  local name="$1"
  require_real_value "$name" "${!name:-}"
}

require_command() {
  local name="$1"
  command -v "$name" >/dev/null 2>&1 || die_usage "${name} is required"
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

validate_base_url() {
  require_real_env BASE_URL
  xrayc_real_e2e_require_url_scheme BASE_URL "real 3-node integrity e2e"
  BASE_URL="$BASE_URL" python3 - <<'PY'
import os
from urllib.parse import urlsplit

url = os.environ["BASE_URL"]
parsed = urlsplit(url)
host = (parsed.hostname or "").lower()
scheme = parsed.scheme.lower()
allow_local = os.environ.get("XRAYC_REAL_3NODE_ALLOW_LOCAL_BASE_URL") == "1"
allow_http = os.environ.get("XRAYC_REAL_3NODE_ALLOW_INSECURE_HTTP") == "1"
allow_test = os.environ.get("XRAYC_REAL_3NODE_ALLOW_TEST_DOMAINS") == "1"

if not host:
    raise SystemExit("BASE_URL must include a host")
if host in {"example.com", "example.org", "example.net", "example.test"} or (
    host.endswith(".example") or (host.endswith(".test") and not allow_test)
):
    raise SystemExit("BASE_URL looks like documentation or test data")
if scheme != "https":
    local_hosts = {"localhost", "127.0.0.1", "::1"}
    if not (allow_http or (allow_local and host in local_hosts)):
        raise SystemExit("BASE_URL must use HTTPS unless an explicit real E2E override is set")
PY
}

validate_expected_exit_ips() {
  require_real_env EXPECTED_EXIT_IPS
  EXPECTED_EXIT_IPS="$EXPECTED_EXIT_IPS" python3 - <<'PY'
import ipaddress
import os

values = [item.strip() for item in os.environ["EXPECTED_EXIT_IPS"].split(",") if item.strip()]
if not values:
    raise SystemExit("EXPECTED_EXIT_IPS is empty")
for value in values:
    try:
        parsed = ipaddress.ip_address(value)
    except ValueError as exc:
        raise SystemExit("EXPECTED_EXIT_IPS contains a non-IP value") from exc
    if not parsed.is_global:
        raise SystemExit("EXPECTED_EXIT_IPS must contain public/global egress IP values")
PY
}

validate_access_servers() {
  require_real_env EXPECTED_ACCESS_SERVERS
  EXPECTED_ACCESS_SERVERS="$EXPECTED_ACCESS_SERVERS" python3 - <<'PY'
import os

allow_test = os.environ.get("XRAYC_REAL_3NODE_ALLOW_TEST_DOMAINS") == "1"
values = [item.strip() for item in os.environ["EXPECTED_ACCESS_SERVERS"].split(",") if item.strip()]
if not values:
    raise SystemExit("EXPECTED_ACCESS_SERVERS is empty")
for value in values:
    if value.count(":") < 1:
        raise SystemExit("EXPECTED_ACCESS_SERVERS entries must use host:port")
    host, port = value.rsplit(":", 1)
    host_l = host.strip("[]").lower()
    if not host_l:
        raise SystemExit("EXPECTED_ACCESS_SERVERS contains an empty host")
    if host_l in {"example.com", "example.org", "example.net", "example.test"} or (
        host_l.endswith(".example") or (host_l.endswith(".test") and not allow_test)
    ):
        raise SystemExit("EXPECTED_ACCESS_SERVERS looks like documentation or test data")
    try:
        port_i = int(port)
    except ValueError as exc:
        raise SystemExit("EXPECTED_ACCESS_SERVERS contains a non-numeric port") from exc
    if port_i < 1 or port_i > 65535:
        raise SystemExit("EXPECTED_ACCESS_SERVERS contains an invalid port")
PY
}

curl_json_post_private() {
  local label="$1"
  local path="$2"
  local payload_file="$3"
  local body_file="$4"
  local status_file="$5"
  local err_file="$tmp_dir/${label}.err"

  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --header "Content-Type: application/json" \
    --data-binary "@$payload_file" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>"$err_file"; then
    fail "${label} request failed; details redacted"
  fi
}

curl_bearer_private() {
  local label="$1"
  local method="$2"
  local path="$3"
  local token="$4"
  local body_file="$5"
  local status_file="$6"
  local err_file="$tmp_dir/${label}.err"
  local auth_config="$tmp_dir/${label}.auth.conf"
  local escaped="${token//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$auth_config"
  chmod 600 "$auth_config"

  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --request "$method" \
    --config "$auth_config" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>"$err_file"; then
    fail "${label} request failed; details redacted"
  fi
}

json_eval() {
  local expression="$1"
  local file="$2"
  python3 - "$expression" "$file" <<'PY'
import json
import sys

expression, path = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload) if isinstance(payload, dict) else {}

if expression == "access_token":
    keys = ("access_token", "accessToken", "token")
elif expression == "subscription_token":
    keys = ("token", "subscription_token", "subscriptionToken")
elif expression == "subscription_url":
    keys = ("subscription_url", "subscriptionUrl", "download_url", "downloadUrl", "clash_url", "clashUrl", "url")
else:
    raise SystemExit(2)

for key in keys:
    value = data.get(key) if isinstance(data, dict) else None
    if isinstance(value, str) and value.strip():
        print(value.strip())
        raise SystemExit(0)
raise SystemExit(1)
PY
}

login_user_if_needed() {
  if [[ -n "$USER_ACCESS_TOKEN" && ( -n "$SUB_TOKEN" || -n "$SUBSCRIPTION_URL" ) ]]; then
    return
  fi
  if [[ -z "$USER_LOGIN_ACCOUNT" || -z "$USER_LOGIN_PASSWORD" ]]; then
    return
  fi

  require_real_value USER_LOGIN_ACCOUNT "$USER_LOGIN_ACCOUNT"
  require_real_value USER_LOGIN_PASSWORD "$USER_LOGIN_PASSWORD"

  local account_file="$tmp_dir/user-login-account.txt"
  local password_file="$tmp_dir/user-login-password.txt"
  local payload_file="$tmp_dir/user-login-payload.json"
  local body_file="$tmp_dir/user-login.body"
  local status_file="$tmp_dir/user-login.status"
  printf '%s' "$USER_LOGIN_ACCOUNT" >"$account_file"
  printf '%s' "$USER_LOGIN_PASSWORD" >"$password_file"
  python3 - "$account_file" "$password_file" >"$payload_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    account = fh.read()
with open(sys.argv[2], "r", encoding="utf-8") as fh:
    password = fh.read()
print(json.dumps({"account": account, "password": password}, separators=(",", ":")))
PY
  curl_json_post_private "user-login" "/api/auth/login" "$payload_file" "$body_file" "$status_file"
  case "$(cat "$status_file")" in
    2*) ;;
    *) fail "user login was rejected; response body redacted" ;;
  esac
  USER_ACCESS_TOKEN="$(json_eval access_token "$body_file")"
}

discover_subscription_if_needed() {
  if [[ -n "$SUB_TOKEN" || -n "$SUBSCRIPTION_URL" ]]; then
    return
  fi
  [[ -n "$USER_ACCESS_TOKEN" ]] || return

  local body_file="$tmp_dir/user-subscription.body"
  local status_file="$tmp_dir/user-subscription.status"
  curl_bearer_private "user-subscription" "GET" "/api/user/subscription" "$USER_ACCESS_TOKEN" "$body_file" "$status_file"
  case "$(cat "$status_file")" in
    2*) ;;
    *) fail "user subscription discovery was rejected; response body redacted" ;;
  esac
  SUB_TOKEN="$(json_eval subscription_token "$body_file" || true)"
  SUBSCRIPTION_URL="$(json_eval subscription_url "$body_file" || true)"
}

extract_subscription_token_from_url() {
  local url="$1"
  SUBSCRIPTION_URL="$url" BASE_URL="$BASE_URL" python3 - <<'PY'
import os
from urllib.parse import urljoin, urlsplit

url = os.environ["SUBSCRIPTION_URL"].strip()
base = os.environ["BASE_URL"].rstrip("/") + "/"
if url.startswith("/"):
    url = urljoin(base, url.lstrip("/"))
path = urlsplit(url).path
parts = [part for part in path.split("/") if part]
for index, part in enumerate(parts):
    if part == "sub" and index + 1 < len(parts):
        print(parts[index + 1])
        raise SystemExit(0)
raise SystemExit(1)
PY
}

subscription_download_url() {
  if [[ -n "$SUBSCRIPTION_URL" ]]; then
    if [[ "$SUBSCRIPTION_URL" == /* ]]; then
      printf '%s%s\n' "${BASE_URL%/}" "$SUBSCRIPTION_URL"
    else
      printf '%s\n' "$SUBSCRIPTION_URL"
    fi
  else
    printf '%s/sub/%s\n' "${BASE_URL%/}" "$SUB_TOKEN"
  fi
}

sha256_literal() {
  printf '%s' "$1" | sha256sum | awk '{print $1}'
}

first_expected_access_server() {
  EXPECTED_ACCESS_SERVERS="$EXPECTED_ACCESS_SERVERS" python3 - <<'PY'
import os
values = [item.strip() for item in os.environ["EXPECTED_ACCESS_SERVERS"].split(",") if item.strip()]
if len(values) == 1:
    print(values[0])
else:
    raise SystemExit(1)
PY
}

validate_required_inputs() {
  require_command curl
  require_command python3
  require_command psql
  require_command sha256sum
  validate_base_url
  require_real_env DATABASE_URL
  require_real_env CLIENT_PROXY_URL
  validate_expected_exit_ips
  validate_access_servers
  if [[ -n "$INVENTORY" ]]; then
    require_real_env INVENTORY
    require_real_env ACCESS_TARGET
  else
    require_real_value XRAYC_REAL_3NODE_ACCESS_SSH_HOST "$DIRECT_ACCESS_SSH_HOST"
    require_real_value XRAYC_REAL_3NODE_ACCESS_SSH_USER "$DIRECT_ACCESS_SSH_USER"
  fi
  if [[ "$ALLOW_MUTATION" != "1" ]]; then
    die_usage "XRAYC_REAL_3NODE_ALLOW_MUTATION=1 is required for disabled/quota eviction checks"
  fi
}

validate_auth_inputs() {
  if [[ -n "$SUBSCRIPTION_URL" && -z "$SUB_TOKEN" ]]; then
    SUB_TOKEN="$(extract_subscription_token_from_url "$SUBSCRIPTION_URL" || true)"
  fi
  if [[ -n "$SUB_TOKEN" ]]; then
    require_real_value SUB_TOKEN "$SUB_TOKEN"
  fi
  if [[ -n "$SUBSCRIPTION_URL" ]]; then
    require_real_value SUBSCRIPTION_URL "$SUBSCRIPTION_URL"
    if [[ "$SUBSCRIPTION_URL" != /* ]]; then
      xrayc_real_e2e_require_url_scheme_if_set SUBSCRIPTION_URL
    fi
  fi
  if [[ -n "$USER_ACCESS_TOKEN" ]]; then
    require_real_value USER_ACCESS_TOKEN "$USER_ACCESS_TOKEN"
  fi
  if [[ -z "$SUB_TOKEN" ]]; then
    die_usage "SUB_TOKEN is required or must be extractable from SUBSCRIPTION_URL"
  fi
  [[ -n "$USER_ACCESS_TOKEN" ]] || die_usage "USER_ACCESS_TOKEN is required unless login credentials are provided"
}
