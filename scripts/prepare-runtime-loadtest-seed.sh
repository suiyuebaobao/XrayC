#!/usr/bin/env bash
# 用途：为 runtime 负载测试准备确定性的隔离数据库基线数据。
# 范围：检查表结构并补齐用户、计划、节点、线路、出口池等最小样本。
# 输入：需要 DATABASE_URL，可配置用户、节点、线路和出口数量下限。
# 输出：输出准备阶段结果，不打印数据库 URL 或生成的敏感数据。
# 依赖：source real-e2e-lib.sh，并使用 psql 访问测试数据库。
# 安全：拒绝生产环境和显式禁止的数据库 URL，避免误写控制库。
# 约束：所有最小数量变量必须为正整数，默认面向隔离 loadtest 库。
# 行为：缺表或缺列会先失败，结构就绪后按 run marker 写入测试数据。
# 失败：环境非法、psql 缺失、结构不满足或 SQL 执行失败会返回非零。
# 维护：runtime loadtest 数据模型变化时同步 required_tables 和 SQL。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

# Docker 默认 /dev/shm 较小，loadtest 库建索引和统计刷新必须避免并行 DSM 放大。
export PGOPTIONS="${PGOPTIONS:-} -c max_parallel_workers_per_gather=0 -c max_parallel_maintenance_workers=0 -c maintenance_work_mem=16MB -c work_mem=8MB"

usage() {
  cat <<'USAGE'
usage: DATABASE_URL="<isolated-postgres-url>" bash scripts/prepare-runtime-loadtest-seed.sh

Prepare deterministic baseline data for scripts/runtime-loadtest.sh.

Optional:
  RUNTIME_LOADTEST_MIN_USERS     Default 100.
  RUNTIME_LOADTEST_MIN_NODES     Default 3.
  RUNTIME_LOADTEST_MIN_LINES     Default 6.
  RUNTIME_LOADTEST_MIN_EXITS     Default 6.
  RUNTIME_LOADTEST_FORBID_DATABASE_URL  Database URL that must not be reused.
  RUNTIME_LOADTEST_ALLOW_PRODUCTION=1   Allow XRAYC_ENV=production.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

DATABASE_URL="${DATABASE_URL:-}"
RUNTIME_LOADTEST_MIN_USERS="${RUNTIME_LOADTEST_MIN_USERS:-100}"
RUNTIME_LOADTEST_MIN_NODES="${RUNTIME_LOADTEST_MIN_NODES:-3}"
RUNTIME_LOADTEST_MIN_LINES="${RUNTIME_LOADTEST_MIN_LINES:-6}"
RUNTIME_LOADTEST_MIN_EXITS="${RUNTIME_LOADTEST_MIN_EXITS:-6}"
seed_marker="runtime-loadtest-seed"

xrayc_real_e2e_require_url_scheme DATABASE_URL "runtime loadtest seed"
for name in \
  RUNTIME_LOADTEST_MIN_USERS \
  RUNTIME_LOADTEST_MIN_NODES \
  RUNTIME_LOADTEST_MIN_LINES \
  RUNTIME_LOADTEST_MIN_EXITS; do
  if [[ ! "${!name}" =~ ^[1-9][0-9]*$ ]]; then
    echo "${name} must be a positive integer" >&2
    exit 2
  fi
done
if [[ "${XRAYC_ENV:-}" == "production" && "${RUNTIME_LOADTEST_ALLOW_PRODUCTION:-0}" != "1" ]]; then
  echo "runtime loadtest seed refuses production unless explicitly allowed" >&2
  exit 2
fi
if [[ -n "${RUNTIME_LOADTEST_FORBID_DATABASE_URL:-}" && "$DATABASE_URL" == "$RUNTIME_LOADTEST_FORBID_DATABASE_URL" ]]; then
  echo "runtime loadtest seed refuses to reuse the forbidden DATABASE_URL" >&2
  exit 2
fi
xrayc_real_e2e_assert_loadtest_database_url "$DATABASE_URL" "${RUNTIME_LOADTEST_FORBID_DATABASE_URL:-}"
if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required for runtime loadtest seed" >&2
  exit 2
fi

if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.access_line_usage_rollups') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202605250003_access_line_usage_rollups.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.idx_audit_logs_created_id_page') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202605250004_audit_log_page_covering_index.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.access_user_session_events') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202605240001_access_user_session_events.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.idx_usage_ledgers_user_observed_page') IS NULL OR to_regclass('public.idx_access_user_session_events_observed_at') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202605250005_runtime_user_log_indexes.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.idx_access_line_probes_idempotency') IS NULL OR to_regclass('public.idx_access_exit_probes_idempotency') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202605250006_probe_idempotency_indexes.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 <<'SQL' | grep -q '^ready$'; then
SELECT CASE WHEN EXISTS (
  SELECT 1 FROM information_schema.columns
  WHERE table_schema = 'public'
    AND table_name = 'access_lines'
    AND column_name = 'exit_endpoint_id'
) THEN 'ready' ELSE 'missing' END;
SQL
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -f "$BASE_DIR/migrations/202606060001_access_lines_exit_endpoint.sql" >/dev/null
fi
if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -c "SELECT CASE WHEN to_regclass('public.idx_access_lines_exit_endpoint_id') IS NULL THEN 'missing' ELSE 'ready' END" \
  | grep -q '^ready$'; then
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 \
    -c "CREATE INDEX IF NOT EXISTS idx_access_lines_exit_endpoint_id ON access_lines(exit_endpoint_id)" >/dev/null
fi

if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 <<'SQL' | grep -q '^ready$'; then
WITH required_tables(name) AS (
  VALUES ('users'), ('plans'), ('user_subscriptions'), ('subscription_tokens'),
    ('access_nodes'), ('exit_resources'), ('exit_endpoints'), ('exit_pools'),
    ('exit_pool_members'), ('access_lines'), ('line_groups'),
    ('line_group_exit_endpoints'), ('plan_line_groups'), ('user_access_line_assignments'),
    ('access_line_usage_rollups')
),
required_columns(table_name, column_name) AS (
  VALUES ('subscription_tokens', 'token_hash'), ('plans', 'is_deleted'),
    ('plan_line_groups', 'billing_multiplier'), ('access_lines', 'xhttp_mode'),
    ('access_lines', 'exit_endpoint_id'),
    ('exit_pool_members', 'allow_new_assignments'),
    ('access_line_usage_rollups', 'billed_bytes')
),
required_indexes(index_name) AS (
  VALUES ('idx_audit_logs_created_id_page'),
    ('idx_usage_ledgers_user_observed_page'),
    ('idx_access_line_probes_idempotency'),
    ('idx_access_exit_probes_idempotency'),
    ('idx_access_lines_exit_endpoint_id')
)
SELECT CASE WHEN
  NOT EXISTS (
    SELECT 1 FROM required_tables WHERE to_regclass('public.' || name) IS NULL
  )
  AND NOT EXISTS (
    SELECT 1
    FROM required_columns c
    WHERE NOT EXISTS (
      SELECT 1 FROM information_schema.columns
      WHERE table_schema = 'public'
        AND table_name = c.table_name
        AND column_name = c.column_name
    )
  )
  AND NOT EXISTS (
    SELECT 1
    FROM required_indexes i
    WHERE to_regclass('public.' || i.index_name) IS NULL
  )
THEN 'ready' ELSE 'missing' END;
SQL
  echo "runtime loadtest seed requires a migrated isolated database" >&2
  exit 2
fi

echo "runtime loadtest seed preparing"
xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
  -v "marker=$seed_marker" \
  -v "min_users=$RUNTIME_LOADTEST_MIN_USERS" \
  -v "min_nodes=$RUNTIME_LOADTEST_MIN_NODES" \
  -v "min_lines=$RUNTIME_LOADTEST_MIN_LINES" \
  -v "min_exits=$RUNTIME_LOADTEST_MIN_EXITS" <<'SQL' >/dev/null
INSERT INTO plans (
  name, is_default, traffic_limit_bytes, billing_multiplier,
  enabled, price_cents, currency,
  duration_days, sort_weight, is_deleted
)
SELECT
  'Runtime Loadtest Seed Plan',
  TRUE,
  1099511627776,
  1.000,
  TRUE,
  0,
  'USDT',
  30,
  100,
  FALSE
WHERE NOT EXISTS (
  SELECT 1 FROM plans WHERE enabled = TRUE AND is_deleted = FALSE
);

INSERT INTO exit_pools (name, region_code, strategy, enabled)
SELECT 'Runtime Loadtest Seed Pool', 'ZZ', 'priority', TRUE
WHERE NOT EXISTS (
  SELECT 1 FROM exit_pools WHERE name = 'Runtime Loadtest Seed Pool'
);

INSERT INTO line_groups (name)
SELECT 'Runtime Loadtest Seed Group'
WHERE NOT EXISTS (
  SELECT 1 FROM line_groups WHERE name = 'Runtime Loadtest Seed Group'
);

INSERT INTO users (email, password_hash, xray_user_key, display_name)
SELECT
  format('%s-user-%s@example.test', :'marker', gs),
  '$argon2id$v=19$m=19456,t=2,p=1$runtime$placeholderhash',
  format('%s-user-%s', :'marker', gs),
  format('Runtime Loadtest User %s', gs)
FROM generate_series(1, :'min_users'::INTEGER) AS gs
ON CONFLICT (email) DO UPDATE SET
  xray_user_key = EXCLUDED.xray_user_key,
  display_name = EXCLUDED.display_name,
  updated_at = now();

WITH selected_users AS (
  SELECT id
  FROM users
  WHERE email LIKE :'marker' || '-user-%@example.test'
  ORDER BY created_at ASC, id ASC
  LIMIT :'min_users'::INTEGER
),
selected_plan AS (
  SELECT id
  FROM plans
  WHERE enabled = TRUE AND is_deleted = FALSE
  ORDER BY is_default DESC, created_at ASC, id ASC
  LIMIT 1
)
INSERT INTO user_subscriptions (
  user_id, plan_id, active, expires_at, used_bytes, limit_bytes, updated_at
)
SELECT u.id, p.id, TRUE, now() + interval '30 days', 0, 1099511627776, now()
FROM selected_users u
CROSS JOIN selected_plan p
ON CONFLICT (user_id) DO UPDATE SET
  plan_id = EXCLUDED.plan_id,
  active = TRUE,
  expires_at = EXCLUDED.expires_at,
  limit_bytes = EXCLUDED.limit_bytes,
  updated_at = now();

WITH numbered_users AS (
  SELECT
    id,
    row_number() OVER (ORDER BY created_at ASC, id ASC) AS rn
  FROM users
  WHERE email LIKE :'marker' || '-user-%@example.test'
  ORDER BY created_at ASC, id ASC
  LIMIT :'min_users'::INTEGER
)
INSERT INTO subscription_tokens (token, token_hash, user_id, expires_at)
SELECT
  format('%s-token-%s', :'marker', rn),
  md5(format('%s-token-%s', :'marker', rn)),
  id,
  now() + interval '30 days'
FROM numbered_users
ON CONFLICT (user_id) DO UPDATE SET
  token_hash = EXCLUDED.token_hash,
  expires_at = EXCLUDED.expires_at,
  revoked_at = NULL;

INSERT INTO access_nodes (
  name, public_host, agent_token_hash, config_dirty,
  last_heartbeat_at, status, agent_version, config_dirty_reason
)
SELECT
  format('Runtime Loadtest Node %s', gs),
  format('loadtest-node-%s.example.test', gs),
  md5(format('%s-agent-token-%s', :'marker', gs)),
  FALSE,
  now(),
  'online',
  'loadtest',
  'loadtest seed'
FROM generate_series(1, :'min_nodes'::INTEGER) AS gs
WHERE NOT EXISTS (
  SELECT 1 FROM access_nodes n
  WHERE n.name = format('Runtime Loadtest Node %s', gs)
);

INSERT INTO exit_resources (
  name, region_code, enabled, provider_name, ownership, status, last_probe_status
)
SELECT
  format('Runtime Loadtest Exit %s', gs),
  'ZZ',
  TRUE,
  'runtime-loadtest',
  'third_party',
  'healthy',
  'healthy'
FROM generate_series(1, :'min_exits'::INTEGER) AS gs
WHERE NOT EXISTS (
  SELECT 1 FROM exit_resources r
  WHERE r.name = format('Runtime Loadtest Exit %s', gs)
);

WITH selected_resources AS (
  SELECT id, row_number() OVER (ORDER BY name ASC, id ASC) AS rn
  FROM exit_resources
  WHERE name LIKE 'Runtime Loadtest Exit %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_exits'::INTEGER
)
INSERT INTO exit_endpoints (
  exit_resource_id, outbound_type, host, port, outbound_config,
  enabled, name, stream_config, probe_config, last_probe_status
)
SELECT
  id,
  'socks'::endpoint_type,
  format('loadtest-exit-%s.example.test', rn),
  30000 + rn,
  jsonb_build_object('username', '', 'password', ''),
  TRUE,
  format('Runtime Loadtest Endpoint %s', rn),
  '{}'::jsonb,
  jsonb_build_object('url', 'https://example.test/generate_204'),
  'healthy'
FROM selected_resources
WHERE NOT EXISTS (
  SELECT 1 FROM exit_endpoints e
  WHERE e.name = format('Runtime Loadtest Endpoint %s', selected_resources.rn)
);

WITH selected_pool AS (
  SELECT id FROM exit_pools WHERE name = 'Runtime Loadtest Seed Pool' ORDER BY created_at ASC LIMIT 1
),
selected_endpoints AS (
  SELECT id, row_number() OVER (ORDER BY name ASC, id ASC) AS rn
  FROM exit_endpoints
  WHERE name LIKE 'Runtime Loadtest Endpoint %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_exits'::INTEGER
)
INSERT INTO exit_pool_members (
  exit_pool_id, exit_endpoint_id, weight, status, priority,
  allow_new_assignments, updated_at
)
SELECT p.id, e.id, 100, 'healthy', e.rn, TRUE, now()
FROM selected_pool p
CROSS JOIN selected_endpoints e
ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
  status = 'healthy',
  allow_new_assignments = TRUE,
  updated_at = now();

WITH selected_pool AS (
  SELECT id FROM exit_pools WHERE name = 'Runtime Loadtest Seed Pool' ORDER BY created_at ASC LIMIT 1
),
selected_endpoints AS (
  SELECT id, row_number() OVER (ORDER BY name ASC, id ASC) AS rn
  FROM exit_endpoints
  WHERE name LIKE 'Runtime Loadtest Endpoint %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_exits'::INTEGER
),
selected_nodes AS (
  SELECT id, row_number() OVER (ORDER BY name ASC, id ASC) AS rn
  FROM access_nodes
  WHERE name LIKE 'Runtime Loadtest Node %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_nodes'::INTEGER
),
line_numbers AS (
  SELECT generate_series(1, :'min_lines'::INTEGER) AS rn
)
INSERT INTO access_lines (
  name, access_node_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port,
  protocol, transport, user_uuid, server_name, public_key,
  short_id, enabled, region_code, region_name, region_flag,
  flow, udp_enabled, xhttp_path, xhttp_host, xhttp_mode,
  identity_mode, user_key_source, inbound_config, visibility_weight
)
SELECT
  format('Runtime Loadtest Line %s', l.rn),
  n.id,
  e.id,
  p.id,
  format('loadtest-line-%s.example.test', l.rn),
  25000 + l.rn,
  'vless',
  'tcp',
  gen_random_uuid()::TEXT,
  'example.test',
  '',
  '',
  TRUE,
  'ZZ',
  'Loadtest',
  'LT',
  '',
  TRUE,
  '',
  '',
  'stream-one',
  'credential',
  'xray_email',
  '{}'::jsonb,
  100
FROM line_numbers l
CROSS JOIN selected_pool p
JOIN selected_nodes n ON n.rn = ((l.rn - 1) % :'min_nodes'::INTEGER) + 1
JOIN selected_endpoints e ON e.rn = ((l.rn - 1) % :'min_exits'::INTEGER) + 1
WHERE NOT EXISTS (
  SELECT 1 FROM access_lines existing
  WHERE existing.name = format('Runtime Loadtest Line %s', l.rn)
);

WITH selected_group AS (
  SELECT id FROM line_groups WHERE name = 'Runtime Loadtest Seed Group' ORDER BY created_at ASC LIMIT 1
),
selected_lines AS (
  SELECT id, exit_pool_id
  FROM access_lines
  WHERE name LIKE 'Runtime Loadtest Line %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_lines'::INTEGER
)
UPDATE access_lines al
SET line_group_id = g.id
FROM selected_group g, selected_lines l
WHERE al.id = l.id;

WITH selected_group AS (
  SELECT id FROM line_groups WHERE name = 'Runtime Loadtest Seed Group' ORDER BY created_at ASC LIMIT 1
),
selected_lines AS (
  SELECT id, exit_pool_id
  FROM access_lines
  WHERE name LIKE 'Runtime Loadtest Line %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_lines'::INTEGER
)
INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
SELECT DISTINCT g.id, m.exit_endpoint_id
FROM selected_group g
CROSS JOIN selected_lines l
JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING;

WITH selected_plan AS (
  SELECT id
  FROM plans
  WHERE enabled = TRUE AND is_deleted = FALSE
  ORDER BY is_default DESC, created_at ASC, id ASC
  LIMIT 1
),
selected_group AS (
  SELECT id FROM line_groups WHERE name = 'Runtime Loadtest Seed Group' ORDER BY created_at ASC LIMIT 1
)
INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
SELECT p.id, g.id, 1.000
FROM selected_plan p
CROSS JOIN selected_group g
ON CONFLICT (plan_id, line_group_id) DO UPDATE SET
  billing_multiplier = EXCLUDED.billing_multiplier;

WITH selected_users AS (
  SELECT id
  FROM users
  WHERE email LIKE :'marker' || '-user-%@example.test'
  ORDER BY created_at ASC, id ASC
  LIMIT :'min_users'::INTEGER
),
selected_group AS (
  SELECT id FROM line_groups WHERE name = 'Runtime Loadtest Seed Group' ORDER BY created_at ASC LIMIT 1
),
selected_lines AS (
  SELECT id
  FROM access_lines
  WHERE name LIKE 'Runtime Loadtest Line %'
  ORDER BY name ASC, id ASC
  LIMIT :'min_lines'::INTEGER
)
INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id, assigned_at)
SELECT u.id, g.id, l.id, now()
FROM selected_users u
CROSS JOIN selected_group g
CROSS JOIN selected_lines l
ON CONFLICT (user_id, line_group_id, access_line_id) DO UPDATE SET
  assigned_at = EXCLUDED.assigned_at;
SQL

if ! xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
  -v "min_users=$RUNTIME_LOADTEST_MIN_USERS" \
  -v "min_nodes=$RUNTIME_LOADTEST_MIN_NODES" \
  -v "min_lines=$RUNTIME_LOADTEST_MIN_LINES" \
  -v "min_exits=$RUNTIME_LOADTEST_MIN_EXITS" <<'SQL' | grep -q '^ready$'; then
WITH counts AS (
  SELECT
    (SELECT COUNT(*)::BIGINT FROM users WHERE email LIKE 'runtime-loadtest-seed-user-%@example.test') AS users,
    (SELECT COUNT(*)::BIGINT FROM access_nodes WHERE name LIKE 'Runtime Loadtest Node %') AS nodes,
    (SELECT COUNT(*)::BIGINT FROM access_lines WHERE name LIKE 'Runtime Loadtest Line %' AND enabled = TRUE) AS lines,
    (
      SELECT COUNT(DISTINCT access_node_id)::BIGINT
      FROM access_lines
      WHERE name LIKE 'Runtime Loadtest Line %' AND enabled = TRUE
    ) AS line_nodes,
    (SELECT COUNT(*)::BIGINT FROM exit_endpoints WHERE name LIKE 'Runtime Loadtest Endpoint %' AND enabled = TRUE) AS exits,
    (
      SELECT COUNT(*)::BIGINT
      FROM access_lines l
      JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
      JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
      WHERE l.name LIKE 'Runtime Loadtest Line %'
        AND l.enabled = TRUE
        AND e.enabled = TRUE
        AND m.status = 'healthy'
    ) AS routes
)
SELECT CASE
  WHEN users >= :'min_users'::BIGINT
   AND nodes >= :'min_nodes'::BIGINT
   AND lines >= :'min_lines'::BIGINT
   AND line_nodes >= LEAST(:'min_nodes'::BIGINT, :'min_lines'::BIGINT)
   AND exits >= :'min_exits'::BIGINT
   AND routes >= GREATEST(:'min_lines'::BIGINT, :'min_exits'::BIGINT)
  THEN 'ready' ELSE 'missing' END
FROM counts;
SQL
  echo "runtime loadtest seed verification failed" >&2
  exit 1
fi

echo "runtime loadtest seed ready: users=${RUNTIME_LOADTEST_MIN_USERS} nodes=${RUNTIME_LOADTEST_MIN_NODES} lines=${RUNTIME_LOADTEST_MIN_LINES} exits=${RUNTIME_LOADTEST_MIN_EXITS}"
