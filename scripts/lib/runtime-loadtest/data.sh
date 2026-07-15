#!/usr/bin/env bash
# 中文说明：runtime-loadtest 数据 helper，封装数据库准备检查、种子写入和清理 SQL。
# 中文说明：此文件由 scripts/runtime-loadtest.sh source 使用，避免入口脚本保留大段 SQL。
# shellcheck shell=bash

runtime_loadtest_database_has_required_seed_data() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "min_users=$RUNTIME_LOADTEST_MIN_USERS" \
    -v "min_nodes=$RUNTIME_LOADTEST_MIN_NODES" \
    -v "min_lines=$RUNTIME_LOADTEST_MIN_LINES" \
    -v "min_exits=$RUNTIME_LOADTEST_MIN_EXITS" <<'SQL' | grep -q '^ready$'
WITH counts AS (
  SELECT
    (SELECT COUNT(*)::BIGINT FROM users) AS users,
    (SELECT COUNT(*)::BIGINT FROM access_nodes) AS nodes,
    (SELECT COUNT(*)::BIGINT FROM access_lines) AS lines,
    (SELECT COUNT(DISTINCT access_node_id)::BIGINT FROM access_lines) AS line_nodes,
    (SELECT COUNT(*)::BIGINT FROM exit_endpoints) AS exits,
    (
      SELECT COUNT(*)::BIGINT
      FROM access_lines l
      JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
      JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
      WHERE l.enabled = TRUE
        AND e.enabled = TRUE
    ) AS line_exit_routes
)
SELECT CASE
  WHEN users >= :'min_users'::BIGINT
   AND nodes >= :'min_nodes'::BIGINT
   AND lines >= :'min_lines'::BIGINT
   AND line_nodes >= LEAST(:'min_nodes'::BIGINT, :'min_lines'::BIGINT)
   AND exits >= :'min_exits'::BIGINT
   AND line_exit_routes >= GREATEST(:'min_lines'::BIGINT, :'min_exits'::BIGINT)
  THEN 'ready' ELSE 'missing' END
FROM counts;
SQL
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 <<'SQL' >/dev/null
ANALYZE usage_ledgers;
ANALYZE access_line_usage_rollups;
VACUUM (ANALYZE) audit_logs;
ANALYZE access_line_metric_snapshots;
ANALYZE access_traffic_snapshots;
ANALYZE access_user_sessions;
ANALYZE access_exit_probes;
ANALYZE access_exit_probe_states;
ANALYZE access_nodes;
SQL
}

runtime_loadtest_cleanup_generated_rows() {
  if [[ "${RUNTIME_LOADTEST_KEEP_DATA:-0}" == "1" ]]; then
    echo "runtime loadtest generated rows kept"
    return
  fi
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "marker=$run_marker" \
    -v "run_started=$run_started" \
    -v "runtime_rows=$RUNTIME_LOADTEST_RUNTIME_ROWS" <<'SQL' >/dev/null
DELETE FROM usage_ledgers WHERE xray_user_key LIKE :'marker' || '%';
DELETE FROM access_traffic_snapshots WHERE xray_user_key LIKE :'marker' || '%';
DELETE FROM audit_logs WHERE action = :'marker';
DELETE FROM access_line_metric_snapshots
WHERE online_users = 987654
  AND active_connections = 123456
  AND unique_client_ips = 654321
  AND collected_at >= :'run_started'::timestamptz - (:'runtime_rows'::BIGINT * interval '1 second');
DELETE FROM access_user_sessions WHERE xray_user_key LIKE :'marker' || '%';
DELETE FROM access_line_probes WHERE status = :'marker';
DELETE FROM access_exit_probes WHERE status = :'marker';
DELETE FROM access_exit_probe_states WHERE last_error_summary = :'marker';
SQL
}

runtime_loadtest_seed_rows() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "marker=$run_marker" \
    -v "run_started=$run_started" \
    -v "ledger_rows=$RUNTIME_LOADTEST_LEDGER_ROWS" \
    -v "audit_rows=$RUNTIME_LOADTEST_AUDIT_ROWS" \
    -v "runtime_rows=$RUNTIME_LOADTEST_RUNTIME_ROWS" \
    -v "traffic_rows=$RUNTIME_LOADTEST_TRAFFIC_ROWS" \
    -v "min_users=$RUNTIME_LOADTEST_MIN_USERS" \
    -v "min_nodes=$RUNTIME_LOADTEST_MIN_NODES" \
    -v "min_lines=$RUNTIME_LOADTEST_MIN_LINES" \
    -v "min_exits=$RUNTIME_LOADTEST_MIN_EXITS" <<'SQL' >/dev/null
WITH selected_users AS (
  SELECT id, xray_user_key
  FROM users
  ORDER BY created_at ASC, id ASC
  LIMIT :'min_users'
),
selected_line_routes AS (
  SELECT
    l.access_node_id,
    l.id AS access_line_id,
    l.exit_pool_id AS exit_pool_id,
    e.id AS exit_endpoint_id,
    row_number() OVER (ORDER BY l.access_node_id, l.id, e.id) AS route_rn
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pool_members m ON m.exit_pool_id = l.exit_pool_id
  JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
  WHERE l.enabled = TRUE
    AND e.enabled = TRUE
),
refs AS (
  SELECT
    u.id AS user_id,
    u.xray_user_key AS real_xray_user_key,
    r.access_node_id,
    r.access_line_id,
    r.exit_pool_id,
    r.exit_endpoint_id,
    row_number() OVER (ORDER BY u.id, r.route_rn) AS rn
  FROM selected_users u
  CROSS JOIN selected_line_routes r
),
valid_refs AS (
  SELECT *
  FROM refs
  WHERE user_id IS NOT NULL
    AND access_node_id IS NOT NULL
    AND access_line_id IS NOT NULL
    AND exit_pool_id IS NOT NULL
    AND exit_endpoint_id IS NOT NULL
),
ref_count AS (
  SELECT COUNT(*)::BIGINT AS total FROM valid_refs
),
ledger_seed AS (
  INSERT INTO usage_ledgers (
    access_line_id, user_id, xray_user_key, traffic_source,
    delta_uplink, delta_downlink, billing_multiplier,
    billed_bytes, delta_total, billed_uplink, billed_downlink,
    collected_at, recorded_at, exit_endpoint_id
  )
  SELECT
    r.access_line_id,
    r.user_id,
    :'marker' || '-ledger-' || gs::TEXT,
    'access_line',
    1024,
    2048,
    1.000,
    3072,
    3072,
    1024,
    2048,
    :'run_started'::timestamptz - (gs::BIGINT % 86400) * interval '1 second',
    now(),
    r.exit_endpoint_id
  FROM generate_series(1, :'ledger_rows'::BIGINT) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  RETURNING 1
),
audit_seed AS (
  INSERT INTO audit_logs (
    actor_user_id, actor_email, action, resource_type,
    request_summary, result, created_at
  )
  SELECT
    r.user_id,
    'runtime-loadtest@example.test',
    :'marker',
    'runtime',
    jsonb_build_object('marker', :'marker', 'seq', gs),
    'success',
    :'run_started'::timestamptz - (gs::BIGINT % 86400) * interval '1 second'
  FROM generate_series(1, :'audit_rows'::BIGINT) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  RETURNING 1
),
metric_seed AS (
  INSERT INTO access_line_metric_snapshots (
    access_node_id, exit_pool_id, access_line_id,
    online_users, active_connections, unique_client_ips,
    uplink_rate_bps, downlink_rate_bps, collected_at
  )
  SELECT
    r.access_node_id,
    r.exit_pool_id,
    r.access_line_id,
    987654,
    123456,
    654321,
    1048576,
    2097152,
    :'run_started'::timestamptz - (gs::BIGINT % 86400) * interval '1 second'
  FROM generate_series(1, :'runtime_rows'::BIGINT) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  RETURNING 1
),
traffic_snapshot_seed AS (
  INSERT INTO access_traffic_snapshots (
    access_line_id, xray_user_key, uplink_total, downlink_total, collected_at
  )
  SELECT
    r.access_line_id,
    :'marker' || '-traffic-' || gs::TEXT,
    1048576 + gs::BIGINT,
    2097152 + gs::BIGINT,
    now() - (gs::BIGINT % 600) * interval '1 second'
  FROM generate_series(1, :'traffic_rows'::BIGINT) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  ON CONFLICT (access_line_id, xray_user_key) DO UPDATE SET
    uplink_total = EXCLUDED.uplink_total,
    downlink_total = EXCLUDED.downlink_total,
    collected_at = EXCLUDED.collected_at
  RETURNING 1
),
session_seed AS (
  INSERT INTO access_user_sessions (
    access_node_id, access_line_id, xray_user_key,
    client_ip_hash, active_connection_count, status,
    started_at, last_seen_at
  )
  SELECT
    r.access_node_id,
    r.access_line_id,
    :'marker' || '-session-' || gs::TEXT,
    'sha256:' || lpad(to_hex(gs::BIGINT), 64, '0'),
    1,
    'online',
    now() - interval '1 minute',
    now()
  FROM generate_series(1, GREATEST(1, :'runtime_rows'::BIGINT / 10)) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  RETURNING 1
),
line_probe_seed AS (
  INSERT INTO access_line_probes (access_line_id, status, latency_ms, error_summary, probed_at)
  SELECT
    r.access_line_id,
    :'marker',
    1,
    '',
    :'run_started'::timestamptz + (gs::BIGINT * interval '1 microsecond')
  FROM generate_series(1, GREATEST(1, :'runtime_rows'::BIGINT / 20)) AS gs
  JOIN ref_count rc ON rc.total > 0
  JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1
  RETURNING 1
),
exit_probe_state_seed AS (
  INSERT INTO access_exit_probe_states (
    access_node_id, exit_endpoint_id, effective_status,
    consecutive_failures, consecutive_successes,
    last_probe_status, last_latency_ms, last_error_summary,
    last_probe_at, status_changed_at, updated_at
  )
  SELECT DISTINCT ON (r.access_node_id, r.exit_endpoint_id)
    r.access_node_id,
    r.exit_endpoint_id,
    'healthy',
    0,
    3,
    'healthy',
    1,
    :'marker',
    now(),
    now(),
    now()
  FROM valid_refs r
  ON CONFLICT (access_node_id, exit_endpoint_id) DO UPDATE SET
    effective_status = EXCLUDED.effective_status,
    consecutive_failures = EXCLUDED.consecutive_failures,
    consecutive_successes = EXCLUDED.consecutive_successes,
    last_probe_status = EXCLUDED.last_probe_status,
    last_latency_ms = EXCLUDED.last_latency_ms,
    last_error_summary = EXCLUDED.last_error_summary,
    last_probe_at = EXCLUDED.last_probe_at,
    status_changed_at = EXCLUDED.status_changed_at,
    updated_at = EXCLUDED.updated_at
  RETURNING 1
)
INSERT INTO access_exit_probes (
  access_node_id, exit_endpoint_id, status, latency_ms, error_summary, probed_at
)
SELECT
  r.access_node_id,
  r.exit_endpoint_id,
  :'marker',
  1,
  '',
  :'run_started'::timestamptz + (gs::BIGINT * interval '1 microsecond')
FROM generate_series(1, GREATEST(1, :'runtime_rows'::BIGINT / 20)) AS gs
JOIN ref_count rc ON rc.total > 0
JOIN valid_refs r ON r.rn = ((gs::BIGINT - 1) % rc.total) + 1;
SQL

  # large profile 会一次性写入数十万审计行；写入后刷新可见性和统计信息，
  # 确保覆盖索引在冷缓存下也能稳定走 index-only scan。
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 <<'SQL' >/dev/null
VACUUM (ANALYZE) audit_logs;
ANALYZE usage_ledgers;
ANALYZE access_line_usage_rollups;
ANALYZE access_line_metric_snapshots;
ANALYZE access_traffic_snapshots;
ANALYZE access_user_sessions;
ANALYZE access_exit_probes;
ANALYZE access_exit_probe_states;
SQL
}

runtime_loadtest_generated_rows_ready() {
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "marker=$run_marker" \
    -v "run_started=$run_started" \
    -v "ledger_rows=$RUNTIME_LOADTEST_LEDGER_ROWS" \
    -v "audit_rows=$RUNTIME_LOADTEST_AUDIT_ROWS" \
    -v "runtime_rows=$RUNTIME_LOADTEST_RUNTIME_ROWS" \
    -v "traffic_rows=$RUNTIME_LOADTEST_TRAFFIC_ROWS" <<'SQL' | grep -q '^ready$'
SELECT CASE
  WHEN (
    SELECT COUNT(*)::BIGINT
    FROM usage_ledgers
    WHERE xray_user_key LIKE :'marker' || '%'
  ) >= :'ledger_rows'::BIGINT
   AND (
    SELECT COUNT(*)::BIGINT
    FROM access_traffic_snapshots
    WHERE xray_user_key LIKE :'marker' || '%'
  ) >= :'traffic_rows'::BIGINT
   AND (
    SELECT COUNT(*)::BIGINT
    FROM audit_logs
    WHERE action = :'marker'
  ) >= :'audit_rows'::BIGINT
   AND (
    SELECT COUNT(*)::BIGINT
    FROM access_line_metric_snapshots
    WHERE online_users = 987654
      AND active_connections = 123456
      AND unique_client_ips = 654321
      AND collected_at >= :'run_started'::timestamptz - (:'runtime_rows'::BIGINT * interval '1 second')
  ) >= :'runtime_rows'::BIGINT
  THEN 'ready' ELSE 'missing' END;
SQL
}
