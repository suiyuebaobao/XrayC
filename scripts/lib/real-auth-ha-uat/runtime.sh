# 该 helper 为 real-auth-ha-uat.sh 提供运行期基础能力。
# 内容包括布尔解析、工具检查、Docker Compose 副本门禁和 health 门禁。
# 内容还包括不打印敏感响应体的 HTTP 调用与状态断言函数。
# 文件只定义函数，依赖主脚本先初始化 BASE_URL、tmp_dir 等全局变量。
# 这些函数从主脚本拆出，用于控制主脚本行数并保持 UAT 编排清晰。

bool_is_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required" >&2
    exit 2
  fi
}

compose_cmd() {
  local files=()
  local raw_files="${UAT_COMPOSE_FILES:-docker-compose.yml}"
  local old_ifs="$IFS"
  IFS=','
  read -r -a files <<<"$raw_files"
  IFS="$old_ifs"

  local args=()
  local file
  for file in "${files[@]}"; do
    [[ -n "$file" ]] && args+=(--file "$file")
  done
  if [[ -n "$compose_override_file" ]]; then
    args+=(--file "$compose_override_file")
  fi
  if [[ -n "${UAT_COMPOSE_PROJECT_NAME:-}" ]]; then
    args+=(--project-name "$UAT_COMPOSE_PROJECT_NAME")
  fi

  if docker compose version >/dev/null 2>&1; then
    docker compose "${args[@]}" "$@"
  elif command -v docker-compose >/dev/null 2>&1; then
    docker-compose "${args[@]}" "$@"
  else
    echo "Docker Compose is required" >&2
    exit 2
  fi
}

write_compose_override() {
  compose_override_file="$tmp_dir/real-auth-ha-uat.compose.override.yml"
  cat >"$compose_override_file" <<'YAML'
services:
  api:
    build:
      network: host
    environment:
      XRAYC_ENV: development
  caddy:
    ports: !override
      - "127.0.0.1:${HTTP_PORT:-18080}:80"
YAML
}

start_compose_replicas() {
  require_tool docker
  UAT_COMPOSE_PROJECT_NAME="${UAT_COMPOSE_PROJECT_NAME:-xrayc-auth-ha-uat-${run_id}}"
  export UAT_COMPOSE_PROJECT_NAME
  export HTTP_PORT="${UAT_HTTP_PORT:-$(python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)}"
  export POSTGRES_PORT="${UAT_POSTGRES_PORT:-$(python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)}"
  # 隔离 UAT 必须覆盖真实发布 env，避免 .env.real-release 里的生产库或占位值污染临时 Compose。
  export DATABASE_URL="postgres://xrayc:change-me@postgres:5432/xrayc"
  UAT_PSQL_DATABASE_URL="postgres://xrayc:change-me@127.0.0.1:${POSTGRES_PORT}/xrayc"
  export XRAYC_ENV="development"
  export SEED_DEMO_DATA="true"
  export JWT_SECRET="auth-ha-uat-jwt-secret-change-me"
  export XRAYC_PAYMENT_ENABLED="false"
  export XRAYC_FIELD_ENCRYPTION_KEYS="${XRAYC_FIELD_ENCRYPTION_KEYS:-new:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ,old:AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM,test:unit-test-field-key}"
  # 隔离 Compose 使用独立 JWT 密钥和 demo seed，不能复用外部发布环境的管理员 Token。
  ADMIN_ACCESS_TOKEN=""
  ADMIN_LOGIN_ACCOUNT="${UAT_ISOLATED_ADMIN_ACCOUNT:-admin}"
  ADMIN_LOGIN_PASSWORD="${UAT_ISOLATED_ADMIN_PASSWORD:-admin123456}"
  write_compose_override
  local build_arg=()
  if bool_is_true "$UAT_COMPOSE_NO_BUILD"; then
    build_arg+=(--no-build)
  else
    build_arg+=(--build)
  fi
  compose_started=1
  compose_cmd up -d "${build_arg[@]}" --scale "api=${UAT_MIN_API_REPLICAS}" postgres api worker caddy >/dev/null
  BASE_URL="http://127.0.0.1:${HTTP_PORT}"
  wait_for_health
  verify_compose_replicas
}

verify_compose_replicas() {
  require_tool docker
  local count
  count="$(compose_cmd ps -q api | sed '/^$/d' | wc -l | tr -d ' ')"
  if [[ "$count" -lt "$UAT_MIN_API_REPLICAS" ]]; then
    echo "api replica gate failed: Docker Compose reports fewer than required api containers" >&2
    exit 1
  fi
  echo "api replica gate: ${count} container(s)"
}

wait_for_health() {
  local status_file="$tmp_dir/health.status"
  local attempt
  for attempt in $(seq 1 60); do
    if curl --silent --show-error --location --max-time "$CURL_TIMEOUT" \
      --output "$tmp_dir/health.body" \
      --write-out "%{http_code}" \
      "${BASE_URL%/}/health" >"$status_file" 2>/dev/null; then
      if [[ "$(cat "$status_file")" == 2* ]]; then
        echo "health gate: passed"
        return
      fi
    fi
    sleep 2
  done
  echo "health gate failed; response redacted" >&2
  exit 1
}

status_is_2xx() {
  [[ "$1" == 2* ]]
}

assert_status() {
  local label="$1"
  local expected="$2"
  local actual="$3"
  if [[ "$actual" == "$expected" ]]; then
    echo "${label}: HTTP ${actual}"
    return
  fi
  echo "${label}: expected HTTP ${expected}, got HTTP ${actual}" >&2
  exit 1
}

assert_2xx() {
  local label="$1"
  local actual="$2"
  if status_is_2xx "$actual"; then
    echo "${label}: HTTP ${actual}"
    return
  fi
  echo "${label}: expected 2xx, got HTTP ${actual}" >&2
  exit 1
}

assert_rejected() {
  local label="$1"
  local actual="$2"
  case "$actual" in
    400|401|403|409|422|429)
      echo "${label}: HTTP ${actual}"
      ;;
    *)
      echo "${label}: expected rejection, got HTTP ${actual}" >&2
      exit 1
      ;;
  esac
}

curl_common() {
  curl --silent --show-error --location --max-time "$CURL_TIMEOUT" "$@"
}

get_public() {
  local path="$1"
  local body_file="$2"
  local status_file="$3"
  shift 3
  curl_common "$@" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
}

post_json() {
  local path="$1"
  local payload_file="$2"
  local body_file="$3"
  local status_file="$4"
  shift 4
  curl_common "$@" \
    --request POST \
    --header "Content-Type: application/json" \
    --data-binary "@${payload_file}" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
}

post_empty() {
  local path="$1"
  local body_file="$2"
  local status_file="$3"
  shift 3
  curl_common "$@" \
    --request POST \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
}

get_admin_json() {
  local label="$1"
  local path="$2"
  local body_file="$3"
  local status_file="$4"
  local auth_config="$tmp_dir/admin-get-auth.conf"
  local escaped="${ADMIN_ACCESS_TOKEN//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$auth_config"
  chmod 600 "$auth_config"
  curl_common \
    --config "$auth_config" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
  assert_2xx "$label" "$(cat "$status_file")"
}

put_admin_json() {
  local label="$1"
  local path="$2"
  local payload_file="$3"
  local body_file="$4"
  local status_file="$5"
  local auth_config="$tmp_dir/admin-put-auth.conf"
  local escaped="${ADMIN_ACCESS_TOKEN//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$auth_config"
  chmod 600 "$auth_config"
  curl_common \
    --request PUT \
    --config "$auth_config" \
    --header "Content-Type: application/json" \
    --data-binary "@${payload_file}" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null
  assert_2xx "$label" "$(cat "$status_file")"
}
