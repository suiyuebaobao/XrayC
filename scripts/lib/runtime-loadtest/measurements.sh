#!/usr/bin/env bash
# 中文说明：runtime-loadtest 测量 helper，封装 EXPLAIN ANALYZE 查询和延迟阈值检查。
# 中文说明：此文件由 scripts/runtime-loadtest.sh source 使用，集中维护压测查询集合。
# shellcheck shell=bash

runtime_loadtest_measure_query_ms() {
  local label="$1"
  local sql="$2"
  local output_file="$tmp_dir/${label}.json"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "marker=$run_marker" \
    -v "audit_rows=$RUNTIME_LOADTEST_AUDIT_ROWS" \
    -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) ${sql}" >"$output_file"
  python3 - "$output_file" "$label" "$RUNTIME_LOADTEST_MAX_QUERY_MS" <<'PY'
import json
import sys

path, label, max_ms = sys.argv[1], sys.argv[2], float(sys.argv[3])
with open(path, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
if isinstance(payload, list) and payload and isinstance(payload[0], dict):
    plan = payload[0]
elif isinstance(payload, list) and payload and isinstance(payload[0], list) and payload[0]:
    plan = payload[0][0]
else:
    raise SystemExit(f"{label} returned unexpected EXPLAIN JSON shape")
ms = float(plan["Execution Time"])
if ms > max_ms:
    raise SystemExit(f"{label} exceeded latency budget")
print(f"{label}: {ms:.2f} ms")
PY
}

runtime_loadtest_run_measurements() {
  runtime_loadtest_measure_query_ms "ledger-ranking" "
SELECT r.access_line_id,
       COALESCE(l.name, r.access_line_id::text) AS access_line_name,
       COALESCE(n.name, '') AS access_node_name,
       COALESCE(l.region_code, '') AS region_code,
       COALESCE(l.listen_host, '') AS listen_host,
       COALESCE(l.listen_port, 0)::integer AS listen_port,
       r.ledger_count,
       r.delta_uplink,
       r.delta_downlink,
       r.delta_total,
       r.billed_uplink,
       r.billed_downlink,
       r.billed_bytes,
       r.latest_collected_at
FROM access_line_usage_rollups r
LEFT JOIN access_lines l ON l.id = r.access_line_id
LEFT JOIN access_nodes n ON n.id = l.access_node_id
ORDER BY r.billed_bytes DESC,
         r.delta_total DESC,
         r.ledger_count DESC,
         access_line_name ASC
LIMIT 50"

  runtime_loadtest_measure_query_ms "ledger-ranking-total" "
SELECT COUNT(*)::bigint AS access_line_count,
       COALESCE(SUM(ledger_count), 0)::bigint AS ledger_count,
       COALESCE(SUM(delta_uplink), 0)::bigint AS delta_uplink,
       COALESCE(SUM(delta_downlink), 0)::bigint AS delta_downlink,
       COALESCE(SUM(delta_total), 0)::bigint AS delta_total,
       COALESCE(SUM(billed_uplink), 0)::bigint AS billed_uplink,
       COALESCE(SUM(billed_downlink), 0)::bigint AS billed_downlink,
       COALESCE(SUM(billed_bytes), 0)::bigint AS billed_bytes,
       MAX(latest_collected_at) AS latest_collected_at
FROM access_line_usage_rollups"

  runtime_loadtest_measure_query_ms "audit-page" "
SELECT id, actor_email, action, resource_type, result, created_at
FROM audit_logs
ORDER BY created_at DESC, id DESC
LIMIT 100 OFFSET 1000"

  runtime_loadtest_measure_query_ms "audit-deep-page" "
WITH page_ids AS (
    SELECT id
    FROM audit_logs
    ORDER BY created_at DESC, id DESC
    LIMIT 200 OFFSET GREATEST(0, ${RUNTIME_LOADTEST_AUDIT_ROWS}::BIGINT - 200)
)
SELECT a.id, a.actor_email, a.action, a.resource_type, a.resource_id,
       a.request_summary, a.result, a.created_at
FROM page_ids p
JOIN audit_logs a ON a.id = p.id
ORDER BY a.created_at DESC, a.id DESC"

  runtime_loadtest_measure_query_ms "runtime-summary" "
SELECT COUNT(*)::BIGINT AS metric_line_count,
       SUM(online_users)::BIGINT AS online_users,
       SUM(active_connections)::BIGINT AS active_connections,
       SUM(unique_client_ips)::BIGINT AS unique_client_ips,
       MAX(collected_at) AS latest_metric_at
FROM (
    SELECT DISTINCT ON (access_line_id)
        access_line_id, online_users, active_connections, unique_client_ips, collected_at
    FROM access_line_metric_snapshots
    ORDER BY access_line_id, collected_at DESC
) latest"

  runtime_loadtest_measure_query_ms "access-routing-runtime-attach" "
SELECT DISTINCT ON (m.access_line_id)
    m.access_line_id,
    COALESCE(m.access_node_id, l.access_node_id) AS access_node_id,
    COALESCE(m.exit_pool_id, l.exit_pool_id) AS exit_pool_id,
    m.online_users, m.active_connections, m.unique_client_ips,
    m.uplink_rate_bps, m.downlink_rate_bps, m.collected_at
FROM access_line_metric_snapshots m
JOIN access_lines l ON l.id = m.access_line_id
ORDER BY m.access_line_id, m.collected_at DESC"

  runtime_loadtest_measure_query_ms "traffic-snapshot-freshness" "
SELECT access_line_id, COUNT(*)::BIGINT AS snapshot_count, max(collected_at) AS latest_collected_at
FROM access_traffic_snapshots
WHERE collected_at >= now() - interval '10 minutes'
GROUP BY access_line_id
ORDER BY latest_collected_at DESC
LIMIT 50"

  runtime_loadtest_measure_query_ms "session-freshness" "
SELECT access_node_id, access_line_id, COUNT(*)::BIGINT AS online_sessions, max(last_seen_at) AS latest_seen_at
FROM access_user_sessions
WHERE status = 'online'
  AND last_seen_at >= now() - interval '10 minutes'
GROUP BY access_node_id, access_line_id
ORDER BY online_sessions DESC
LIMIT 50"

  runtime_loadtest_measure_query_ms "probe-queue-dispatch" "
SELECT access_node_id, COUNT(*)::BIGINT AS pending_probe_tasks, max(probed_at) AS latest_requested_at
FROM access_exit_probes queued
WHERE status = 'queued'
  AND NOT EXISTS (
    SELECT 1
    FROM access_exit_probes result
    WHERE result.access_node_id = queued.access_node_id
      AND result.exit_endpoint_id = queued.exit_endpoint_id
      AND result.status <> 'queued'
      AND result.probed_at >= queued.probed_at
  )
GROUP BY access_node_id
ORDER BY pending_probe_tasks DESC
LIMIT 50"

  runtime_loadtest_measure_query_ms "probe-state-latest" "
SELECT access_node_id, exit_endpoint_id, effective_status, last_probe_at
FROM access_exit_probe_states
ORDER BY updated_at DESC
LIMIT 100"

  runtime_loadtest_measure_query_ms "agent-config-heartbeat" "
SELECT id, config_dirty, last_heartbeat_at, desired_config_hash, applied_config_hash
FROM access_nodes
ORDER BY COALESCE(last_heartbeat_at, created_at) DESC, id DESC
LIMIT 100"
}
