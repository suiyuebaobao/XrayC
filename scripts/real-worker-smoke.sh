#!/usr/bin/env bash
# 用途：启动隔离 Docker Compose 项目执行 worker 维护 smoke 测试。
# 范围：验证本地 worker 周期任务和运行时保留逻辑，不使用宿主数据库。
# 输入：可配置构建策略、保留项目、等待超时和随机端口。
# 输出：输出服务启动、等待和检查结果，失败时可选择保留 compose 项目。
# 依赖：需要 docker compose、本地镜像或可构建源码以及 PostgreSQL 服务。
# 安全：使用固定测试凭据和隔离项目名，禁止读取真实 DATABASE_URL。
# 约束：默认清理容器、卷和临时目录，除非 WORKER_SMOKE_KEEP=1。
# 行为：按需构建或复用镜像，启动依赖服务并等待 worker 完成维护。
# 失败：镜像、compose、服务健康或 worker 断言失败都会返回非零。
# 维护：worker 服务名、环境变量或维护日志变化时同步检查逻辑。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  bash scripts/real-worker-smoke.sh

Optional environment variables:
  WORKER_SMOKE_BUILD=auto|1|0   Build local images when missing; default auto.
  WORKER_SMOKE_KEEP=1           Keep the temporary compose project after failure.
  WORKER_SMOKE_TIMEOUT=90       Seconds to wait for worker maintenance.

This script starts an isolated Docker Compose project, never uses host
DATABASE_URL, and never prints secrets or private server data.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

PROJECT="xrayc-worker-smoke-$(date +%s)-$$"
RUN_ID="$(printf '%s' "${WORKER_SMOKE_RUN_ID:-$(date +%s)-$$}" | tr -cd 'A-Za-z0-9_.-' | cut -c 1-40)"
POSTGRES_PORT="${WORKER_SMOKE_POSTGRES_PORT:-$((25432 + RANDOM % 1000))}"
WORKER_SMOKE_BUILD="${WORKER_SMOKE_BUILD:-auto}"
WORKER_SMOKE_TIMEOUT="${WORKER_SMOKE_TIMEOUT:-90}"
TMP_DIR="$(mktemp -d)"

if [[ -z "$RUN_ID" ]]; then
  echo "worker smoke run id is empty after sanitization" >&2
  exit 2
fi

compose() {
  env \
    COMPOSE_PROJECT_NAME="$PROJECT" \
    POSTGRES_DB=xrayc \
    POSTGRES_USER=xrayc \
    POSTGRES_PASSWORD=change-me \
    POSTGRES_PORT="$POSTGRES_PORT" \
    DATABASE_URL=postgres://xrayc:change-me@postgres:5432/xrayc \
    XRAYC_ENV=development \
    JWT_SECRET=worker-smoke-jwt-secret-change-me \
    SEED_DEMO_DATA=true \
    WORKER_TICK_SECONDS=1 \
    WORKER_RUNTIME_RETENTION_DAYS=1 \
    RUST_LOG=xrayc_worker=info,xrayc_api=info \
    docker compose -p "$PROJECT" "$@"
}

cleanup() {
  local status=$?
  if [[ "${WORKER_SMOKE_KEEP:-0}" != "1" ]]; then
    compose down -v --remove-orphans >/dev/null 2>&1 || true
  else
    echo "Keeping worker smoke compose project: ${PROJECT}"
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

compose_up() {
  local services=("$@")
  case "$WORKER_SMOKE_BUILD" in
    1|true|TRUE|yes|YES)
      compose up -d --build "${services[@]}"
      ;;
    0|false|FALSE|no|NO)
      compose up -d --no-build "${services[@]}"
      ;;
    auto)
      if docker image inspect xrayc/rust-app:local >/dev/null 2>&1; then
        compose up -d --no-build "${services[@]}"
      else
        compose up -d --build "${services[@]}"
      fi
      ;;
    *)
      echo "WORKER_SMOKE_BUILD must be auto, 1, or 0" >&2
      exit 2
      ;;
  esac
}

wait_api() {
  local elapsed=0
  while [[ "$elapsed" -le 90 ]]; do
    if compose exec -T api sh -lc 'curl -fsS http://127.0.0.1:3000/health >/dev/null' >/dev/null 2>&1; then
      echo "worker smoke api: healthy"
      return
    fi
    sleep 2
    elapsed=$((elapsed + 2))
  done
  echo "worker smoke api did not become healthy" >&2
  compose ps >&2 || true
  exit 1
}

api_status() {
  local path="$1"
  compose exec -T api sh -lc "curl --silent --output /dev/null --write-out '%{http_code}' --max-time 10 http://127.0.0.1:3000${path}"
}

api_login_token() {
  local account="$1"
  local password="$2"
  local output="$3"
  local request_file="$TMP_DIR/login-payload.json"
  local account_file="$TMP_DIR/login-account.txt"
  local password_file="$TMP_DIR/login-password.txt"

  printf '%s' "$account" > "$account_file"
  printf '%s' "$password" > "$password_file"
  python3 - "$account_file" "$password_file" > "$request_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    account = fh.read()
with open(sys.argv[2], "r", encoding="utf-8") as fh:
    password = fh.read()
json.dump({"account": account, "password": password}, sys.stdout)
PY
  compose exec -T api sh -lc 'tmp_file="$(mktemp)"; cat > "$tmp_file"; curl -fsS --max-time 10 -H "Content-Type: application/json" --data-binary "@$tmp_file" http://127.0.0.1:3000/api/auth/login; status=$?; rm -f "$tmp_file"; exit "$status"' < "$request_file" >"$output"
  python3 - "$output" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)

data = payload.get("data", payload)
token = data.get("access_token", "")
if not token:
    raise SystemExit("login response missing access token")
print(token)
PY
}

api_get_json() {
  local path="$1"
  local token="$2"
  local output="$3"
  printf '%s' "$token" | compose exec -T api sh -lc '
    token="$(cat)"
    tmp_file="$(mktemp)"
    printf "header = \"Authorization: Bearer %s\"\n" "$token" > "$tmp_file"
    curl -fsS --max-time 10 --config "$tmp_file" "http://127.0.0.1:3000'"${path}"'"
    status=$?
    rm -f "$tmp_file"
    exit "$status"
  ' >"$output"
}

psql_exec() {
  compose exec -T postgres psql -U xrayc -d xrayc -v ON_ERROR_STOP=1 "$@"
}

psql_scalar() {
  local sql="$1"
  psql_exec -Atq -c "$sql" | tr -d '\r'
}

prepare_worker_data() {
  psql_exec <<SQL >/dev/null
DELETE FROM orders WHERE order_no LIKE 'worker-smoke-${RUN_ID}-%';
DELETE FROM refresh_tokens WHERE token_hash LIKE 'worker-smoke-${RUN_ID}-%';
DELETE FROM auth_challenges WHERE target_hash LIKE 'worker-smoke-${RUN_ID}-%';
DELETE FROM login_guard_states WHERE guard_key LIKE 'worker-smoke-${RUN_ID}-%';
DELETE FROM access_line_metric_snapshots WHERE access_line_id = '00000000-0000-0000-0000-000000000501';
DELETE FROM access_user_sessions WHERE access_node_id = '00000000-0000-0000-0000-000000000201';
DELETE FROM access_line_probes WHERE access_line_id = '00000000-0000-0000-0000-000000000501';
DELETE FROM access_exit_probes WHERE access_node_id = '00000000-0000-0000-0000-000000000201';

UPDATE user_subscriptions
SET active = TRUE,
    expires_at = now() - interval '1 minute',
    updated_at = now()
WHERE user_id = '00000000-0000-0000-0000-000000000001';

INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id)
VALUES (
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000601',
  '00000000-0000-0000-0000-000000000501'
)
ON CONFLICT DO NOTHING;

INSERT INTO user_exit_assignments (
  user_id, access_line_id, exit_pool_id, exit_endpoint_id, failover_reason
)
VALUES (
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000501',
  '00000000-0000-0000-0000-000000000401',
  '00000000-0000-0000-0000-000000000302',
  'worker-smoke'
)
ON CONFLICT (user_id, access_line_id, exit_pool_id) DO UPDATE SET
  exit_endpoint_id = EXCLUDED.exit_endpoint_id,
  failover_reason = EXCLUDED.failover_reason;

UPDATE access_nodes
SET config_dirty = FALSE,
    config_dirty_reason = ''
WHERE id = '00000000-0000-0000-0000-000000000201';

INSERT INTO site_settings (setting_key, setting_value, updated_at)
VALUES (
  'access_operations',
  '{"probe_policy":{"exit_auto_failover_enabled":true,"exit_failure_threshold":3,"exit_recovery_threshold":2,"exit_window_minutes":10,"exit_probe_interval_seconds":30,"probe_queue_batch_size":1,"max_pending_probe_tasks":1}}'::jsonb,
  now()
)
ON CONFLICT (setting_key) DO UPDATE SET
  setting_value = EXCLUDED.setting_value,
  updated_at = now();

INSERT INTO orders (
  order_no, user_id, plan_id, amount_cents, currency, status,
  payment_address, expires_at
)
VALUES
  (
    'worker-smoke-${RUN_ID}-old',
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0000-000000000101',
    100,
    'USDT',
    'pending',
    'worker-smoke',
    now() - interval '1 minute'
  ),
  (
    'worker-smoke-${RUN_ID}-fresh',
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0000-000000000101',
    100,
    'USDT',
    'pending',
    'worker-smoke',
    now() + interval '1 hour'
  );

INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
VALUES
  ('00000000-0000-0000-0000-000000000001', 'worker-smoke-${RUN_ID}-old-refresh', now() - interval '1 minute'),
  ('00000000-0000-0000-0000-000000000001', 'worker-smoke-${RUN_ID}-fresh-refresh', now() + interval '1 hour');

INSERT INTO auth_challenges (scene, target_hash, code_hash, expires_at, used_at)
VALUES
  ('register', 'worker-smoke-${RUN_ID}-old-target', 'worker-smoke-old-code', now() - interval '1 minute', NULL),
  ('register', 'worker-smoke-${RUN_ID}-used-target', 'worker-smoke-used-code', now() + interval '1 hour', now()),
  ('register', 'worker-smoke-${RUN_ID}-fresh-target', 'worker-smoke-fresh-code', now() + interval '1 hour', NULL);

INSERT INTO login_guard_states (guard_key, failure_count, locked_until, expires_at, updated_at)
VALUES
  ('worker-smoke-${RUN_ID}-old-guard', 1, NULL, now() - interval '1 minute', now()),
  ('worker-smoke-${RUN_ID}-fresh-guard', 1, NULL, now() + interval '1 hour', now());

INSERT INTO access_line_metric_snapshots (
  access_line_id, online_users, active_connections, unique_client_ips,
  uplink_rate_bps, downlink_rate_bps, collected_at
)
VALUES
  ('00000000-0000-0000-0000-000000000501', 1, 1, 1, 1, 1, now() - interval '3 days'),
  ('00000000-0000-0000-0000-000000000501', 2, 2, 2, 2, 2, now());

INSERT INTO access_user_sessions (
  access_node_id, access_line_id, xray_user_key, client_ip_hash,
  started_at, last_seen_at
)
VALUES
  (
    '00000000-0000-0000-0000-000000000201',
    '00000000-0000-0000-0000-000000000501',
    'worker-smoke-old-${RUN_ID}@example.test',
    'sha256:0000000000000000000000000000000000000000000000000000000000000000',
    now() - interval '3 days',
    now() - interval '3 days'
  ),
  (
    '00000000-0000-0000-0000-000000000201',
    '00000000-0000-0000-0000-000000000501',
    'worker-smoke-fresh-${RUN_ID}@example.test',
    'sha256:1111111111111111111111111111111111111111111111111111111111111111',
    now(),
    now()
  );

INSERT INTO access_line_probes (access_line_id, status, latency_ms, probed_at)
VALUES
  ('00000000-0000-0000-0000-000000000501', 'healthy', 1, now() - interval '3 days'),
  ('00000000-0000-0000-0000-000000000501', 'healthy', 1, now());

INSERT INTO access_exit_probes (
  access_node_id, exit_endpoint_id, status, latency_ms, probed_at
)
VALUES
  ('00000000-0000-0000-0000-000000000201', '00000000-0000-0000-0000-000000000302', 'healthy', 1, now() - interval '3 days'),
  ('00000000-0000-0000-0000-000000000201', '00000000-0000-0000-0000-000000000302', 'healthy', 1, now() - interval '45 seconds');
SQL
}

sql_equals() {
  local label="$1"
  local sql="$2"
  local expected="$3"
  local actual=""
  actual="$(psql_scalar "$sql")"
  if [[ "$actual" != "$expected" ]]; then
    echo "${label}: expected ${expected}, got ${actual}" >&2
    return 1
  fi
}

worker_state_ready() {
  sql_equals "subscription reset active" "SELECT active::text FROM user_subscriptions WHERE user_id = '00000000-0000-0000-0000-000000000001'" "true" || return 1
  sql_equals "subscription reset to default plan" "SELECT (s.plan_id = p.id)::text FROM user_subscriptions s JOIN plans p ON p.is_default = TRUE AND p.enabled = TRUE AND p.is_deleted = FALSE WHERE s.user_id = '00000000-0000-0000-0000-000000000001' ORDER BY p.created_at ASC LIMIT 1" "true" || return 1
  sql_equals "subscription used bytes reset" "SELECT used_bytes::text FROM user_subscriptions WHERE user_id = '00000000-0000-0000-0000-000000000001'" "0" || return 1
  sql_equals "subscription limit reset to default plan" "SELECT (s.limit_bytes = p.traffic_limit_bytes)::text FROM user_subscriptions s JOIN plans p ON p.id = s.plan_id WHERE s.user_id = '00000000-0000-0000-0000-000000000001'" "true" || return 1
  sql_equals "subscription renewed" "SELECT (expires_at > now())::text FROM user_subscriptions WHERE user_id = '00000000-0000-0000-0000-000000000001'" "true" || return 1
  sql_equals "access assignments pruned" "SELECT COUNT(*)::BIGINT FROM user_access_line_assignments WHERE user_id = '00000000-0000-0000-0000-000000000001'" "0" || return 1
  sql_equals "exit assignments pruned" "SELECT COUNT(*)::BIGINT FROM user_exit_assignments WHERE user_id = '00000000-0000-0000-0000-000000000001'" "0" || return 1
  sql_equals "dirty reason" "SELECT config_dirty_reason FROM access_nodes WHERE id = '00000000-0000-0000-0000-000000000201'" "worker_expired_subscription" || return 1
  sql_equals "old order expired" "SELECT status FROM orders WHERE order_no = 'worker-smoke-${RUN_ID}-old'" "expired" || return 1
  sql_equals "fresh order pending" "SELECT status FROM orders WHERE order_no = 'worker-smoke-${RUN_ID}-fresh'" "pending" || return 1
  sql_equals "old refresh removed" "SELECT COUNT(*)::BIGINT FROM refresh_tokens WHERE token_hash = 'worker-smoke-${RUN_ID}-old-refresh'" "0" || return 1
  sql_equals "fresh refresh kept" "SELECT COUNT(*)::BIGINT FROM refresh_tokens WHERE token_hash = 'worker-smoke-${RUN_ID}-fresh-refresh'" "1" || return 1
  sql_equals "old and used challenges removed" "SELECT COUNT(*)::BIGINT FROM auth_challenges WHERE target_hash IN ('worker-smoke-${RUN_ID}-old-target', 'worker-smoke-${RUN_ID}-used-target')" "0" || return 1
  sql_equals "fresh challenge kept" "SELECT COUNT(*)::BIGINT FROM auth_challenges WHERE target_hash = 'worker-smoke-${RUN_ID}-fresh-target'" "1" || return 1
  sql_equals "old guard removed" "SELECT COUNT(*)::BIGINT FROM login_guard_states WHERE guard_key = 'worker-smoke-${RUN_ID}-old-guard'" "0" || return 1
  sql_equals "fresh guard kept" "SELECT COUNT(*)::BIGINT FROM login_guard_states WHERE guard_key = 'worker-smoke-${RUN_ID}-fresh-guard'" "1" || return 1
  sql_equals "metric retention" "SELECT COUNT(*)::BIGINT FROM access_line_metric_snapshots WHERE access_line_id = '00000000-0000-0000-0000-000000000501'" "1" || return 1
  sql_equals "old session pruned" "SELECT COUNT(*)::BIGINT FROM access_user_sessions WHERE xray_user_key = 'worker-smoke-old-${RUN_ID}@example.test'" "0" || return 1
  sql_equals "fresh session kept" "SELECT COUNT(*)::BIGINT FROM access_user_sessions WHERE xray_user_key = 'worker-smoke-fresh-${RUN_ID}@example.test'" "1" || return 1
  sql_equals "line probe retention" "SELECT COUNT(*)::BIGINT FROM access_line_probes WHERE access_line_id = '00000000-0000-0000-0000-000000000501'" "1" || return 1
  sql_equals "exit probe retention" "SELECT COUNT(*)::BIGINT FROM access_exit_probes WHERE access_node_id = '00000000-0000-0000-0000-000000000201' AND status <> 'queued'" "1" || return 1
  sql_equals "scheduled exit probe queued" "SELECT COUNT(*)::BIGINT FROM access_exit_probes WHERE access_node_id = '00000000-0000-0000-0000-000000000201' AND status = 'queued' AND error_summary = 'scheduled probe'" "1" || return 1
}

echo "Starting isolated worker smoke project"
compose_up postgres api
wait_api

if [[ "$(api_status /sub/demo-token)" != 2* ]]; then
  echo "demo subscription must be downloadable before worker mutation" >&2
  exit 1
fi

user_login="$TMP_DIR/user-login.json"
admin_login="$TMP_DIR/admin-login.json"
user_token="$(api_login_token demo@example.test demo123456 "$user_login")"
admin_token="$(api_login_token admin admin123456 "$admin_login")"

prepare_worker_data
compose_up worker

echo "Waiting for worker maintenance assertions"
elapsed=0
until worker_state_ready >/dev/null 2>&1; do
  if [[ "$elapsed" -ge "$WORKER_SMOKE_TIMEOUT" ]]; then
    echo "worker maintenance assertions did not pass before timeout" >&2
    worker_state_ready || true
    compose logs --tail=120 worker >&2 || true
    exit 1
  fi
  sleep 2
  elapsed=$((elapsed + 2))
done
worker_state_ready

if [[ "$(api_status /sub/demo-token)" != 2* ]]; then
  echo "demo subscription should remain downloadable after worker resets to the default plan" >&2
  exit 1
fi

api_get_json /api/user/subscription "$user_token" "$TMP_DIR/user-subscription.json"
python3 - "$TMP_DIR/user-subscription.json" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)

data = payload.get("data", payload)
traffic = data.get("traffic") or {}
if not data.get("access_lines"):
    raise SystemExit("reset user subscription should expose default-plan access lines")
if traffic.get("used_gb") not in (0, 0.0):
    raise SystemExit("reset user subscription should clear used traffic")
if traffic.get("total_gb") not in (10, 10.0):
    raise SystemExit("reset user subscription should restore the default traffic limit")
print("user subscription api check passed")
PY

api_get_json /api/orders "$user_token" "$TMP_DIR/user-orders.json"
api_get_json /api/admin/orders "$admin_token" "$TMP_DIR/admin-orders.json"

echo "worker docker smoke checks completed"
