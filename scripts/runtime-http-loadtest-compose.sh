#!/usr/bin/env bash
# 用途：用隔离 Docker Compose 环境运行 runtime HTTP 负载测试。
# 范围：启动本地临时服务栈并调用 runtime-http-loadtest.sh 执行压测。
# 输入：接受 profile、构建策略、保留策略和超时等环境变量。
# 输出：输出 compose 阶段、压测摘要和失败时的有限诊断信息。
# 依赖：需要 docker compose、项目镜像和 runtime HTTP loadtest 脚本。
# 安全：使用隔离项目名和临时目录，避免读取宿主真实 DATABASE_URL。
# 约束：默认测试结束清理 compose 项目，除非显式要求保留。
# 行为：按构建策略启动依赖服务，等待健康后执行 HTTP 压测。
# 失败：compose、健康检查或压测失败都会返回非零退出码。
# 维护：compose 服务名或 loadtest 参数变化时需同步本脚本。
set -euo pipefail
IFS=$'\n\t'

usage() {
  cat <<'USAGE'
usage: bash scripts/runtime-http-loadtest-compose.sh

Starts an isolated PostgreSQL + API Compose project with demo seed data, derives
demo user/admin tokens, runs scripts/runtime-http-loadtest.sh in strict full
mode, and removes the temporary Compose project. Output stays limited to safe
stage names, request counts, and latency summaries.

Optional:
  RUNTIME_HTTP_LOADTEST_PROFILE           smoke, standard, large, or stress. Default large.
  RUNTIME_HTTP_LOADTEST_COMPOSE_NO_BUILD  Set to 1 to reuse existing local images.
  RUNTIME_HTTP_LOADTEST_KEEP_COMPOSE      Set to 1 to keep the temporary project.
  RUNTIME_HTTP_LOADTEST_HEALTH_TIMEOUT    Health wait timeout seconds. Default 240.
  RUNTIME_HTTP_LOADTEST_HEALTH_INTERVAL   Health poll interval seconds. Default 5.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
[[ $# -eq 0 ]] || {
  usage >&2
  exit 2
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

bool_is_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

compose_run() {
  local compose_value="${COMPOSE:-docker compose}"
  local -a compose_parts=()
  local old_ifs="$IFS"
  IFS=' ' read -r -a compose_parts <<<"$compose_value"
  IFS="$old_ifs"
  "${compose_parts[@]}" "$@"
}

random_port() {
  python3 - <<'PY'
import socket

sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
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

write_login_payload() {
  local account="$1"
  local password="$2"
  local output_file="$3"
  ACCOUNT="$account" PASSWORD="$password" python3 - "$output_file" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(
        {"account": os.environ["ACCOUNT"], "password": os.environ["PASSWORD"]},
        fh,
    )
PY
}

derive_token() {
  local label="$1"
  local account="$2"
  local password="$3"
  local base_url="$4"
  local tmp_dir="$5"
  local payload_file="$tmp_dir/${label}-login.json"
  local body_file="$tmp_dir/${label}-login.body"
  local status_file="$tmp_dir/${label}-login.status"

  write_login_payload "$account" "$password" "$payload_file"
  if ! curl \
    --silent \
    --show-error \
    --max-time 20 \
    --header "Content-Type: application/json" \
    --data-binary "@${payload_file}" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "${base_url}/api/auth/login" >"$status_file" 2>"$tmp_dir/${label}-login.err"; then
    echo "runtime-http-compose: ${label} login request failed; stderr redacted" >&2
    exit 1
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *)
      echo "runtime-http-compose: ${label} login failed; response redacted" >&2
      exit 1
      ;;
  esac
  local token
  token="$(extract_access_token "$body_file" || true)"
  if [[ -z "$token" ]]; then
    echo "runtime-http-compose: ${label} token missing; response redacted" >&2
    exit 1
  fi
  printf '%s' "$token"
}

project="${RUNTIME_HTTP_LOADTEST_COMPOSE_PROJECT:-xrayc_http_loadtest_$(date +%s)_$$}"
http_port="${RUNTIME_HTTP_LOADTEST_HTTP_PORT:-$(random_port)}"
postgres_port="${RUNTIME_HTTP_LOADTEST_POSTGRES_PORT:-$(random_port)}"
tmp_dir="$(mktemp -d)"
override_file="$tmp_dir/docker-compose.runtime-http-loadtest.yml"

cat >"$override_file" <<YAML
services:
  api:
    build:
      network: host
    ports:
      - "127.0.0.1:${http_port}:3000"
YAML

compose_args=(-f docker-compose.yml -f "$override_file" -p "$project")

cleanup() {
  local exit_code=$?
  if bool_is_true "${RUNTIME_HTTP_LOADTEST_KEEP_COMPOSE:-0}"; then
    echo "runtime-http-compose: kept temporary project ${project}"
  else
    compose_run "${compose_args[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
  exit "$exit_code"
}
trap cleanup EXIT INT TERM

up_flags=(-d --no-build)

echo "runtime-http-compose: project ${project}"
echo "runtime-http-compose: starting isolated postgres and api"
HTTP_PORT="$http_port" \
POSTGRES_PORT="$postgres_port" \
POSTGRES_DB=xrayc \
POSTGRES_USER=xrayc \
POSTGRES_PASSWORD=change-me \
DATABASE_URL=postgres://xrayc:change-me@postgres:5432/xrayc \
XRAYC_ENV=development \
SEED_DEMO_DATA=true \
JWT_SECRET="runtime-http-loadtest-jwt-secret" \
DEPLOY_ARTIFACT_TOKEN="" \
XRAYC_FIELD_ENCRYPTION_KEYS="" \
XRAYC_FIELD_ENCRYPTION_KEYS_FILE="" \
XRAYC_FIELD_ENCRYPTION_KEYS_COMMAND="" \
compose_run "${compose_args[@]}" up "${up_flags[@]}" postgres api

timeout_seconds="${RUNTIME_HTTP_LOADTEST_HEALTH_TIMEOUT:-240}"
interval_seconds="${RUNTIME_HTTP_LOADTEST_HEALTH_INTERVAL:-5}"
elapsed=0
while :; do
  not_ready=0
  for service in postgres api; do
    cid="$(compose_run "${compose_args[@]}" ps -q "$service")"
    status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$cid" 2>/dev/null || printf missing)"
    case "$status" in
      healthy|running) ;;
      *)
        not_ready=1
        echo "runtime-http-compose: waiting for ${service}: ${status}"
        ;;
    esac
  done
  if [[ "$not_ready" -eq 0 ]]; then
    break
  fi
  if [[ "$elapsed" -ge "$timeout_seconds" ]]; then
    compose_run "${compose_args[@]}" ps
    compose_run "${compose_args[@]}" logs --tail=120 postgres api
    exit 1
  fi
  sleep "$interval_seconds"
  elapsed=$((elapsed + interval_seconds))
done

base_url="http://127.0.0.1:${http_port}"
user_token="$(derive_token user demo@example.test demo123456 "$base_url" "$tmp_dir")"
admin_token="$(derive_token admin admin@example.test admin123456 "$base_url" "$tmp_dir")"

echo "runtime-http-compose: running strict HTTP/API loadtest"
BASE_URL="$base_url" \
PROFILE="${RUNTIME_HTTP_LOADTEST_PROFILE:-large}" \
RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1 \
RUNTIME_HTTP_LOADTEST_MAX_AVG_MS="${RUNTIME_HTTP_LOADTEST_MAX_AVG_MS:-2000}" \
RUNTIME_HTTP_LOADTEST_MAX_MAX_MS="${RUNTIME_HTTP_LOADTEST_MAX_MAX_MS:-10000}" \
USER_ACCESS_TOKEN="$user_token" \
ADMIN_ACCESS_TOKEN="$admin_token" \
AGENT_TOKEN="demo-agent-token" \
ACCESS_NODE_ID="00000000-0000-0000-0000-000000000201" \
ACCESS_LINE_ID="00000000-0000-0000-0000-000000000501" \
EXIT_ENDPOINT_ID="00000000-0000-0000-0000-000000000302" \
XRAY_USER_KEY="u-00000000000000000000000000000001@xrayc.local" \
bash scripts/runtime-http-loadtest.sh
