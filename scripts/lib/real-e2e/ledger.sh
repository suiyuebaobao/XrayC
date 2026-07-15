#!/usr/bin/env bash
# 用途：提供 real e2e 访问线路 ledger 基线捕获和增长轮询函数。
# 该文件依赖入口脚本先加载 core 与 postgres helper。
set -euo pipefail

xrayc_real_e2e_capture_ledger_baseline() {
  local access_line_id="${1:-}"
  local exit_endpoint_id="${2:-}"
  local outbound_type="${3:-}"

  if [[ -z "${DATABASE_URL:-}" ]]; then
    if xrayc_real_e2e_bool_is_true "${REQUIRE_LEDGER_SOURCE_CHECK:-false}"; then
      echo "DATABASE_URL is required when REQUIRE_LEDGER_SOURCE_CHECK=true" >&2
      exit 2
    fi
    printf '\n'
    return
  fi
  if [[ -z "$access_line_id" || -z "$exit_endpoint_id" || -z "$outbound_type" ]]; then
    echo "ledger source check requires access line id, exit endpoint id, and outbound type" >&2
    exit 2
  fi
  if ! command -v psql >/dev/null 2>&1; then
    echo "psql is required for ledger source checks" >&2
    exit 2
  fi

  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
    -v "access_line_id=$access_line_id" \
    -v "exit_endpoint_id=$exit_endpoint_id" \
    -v "outbound_type=$outbound_type" <<'SQL'
SELECT COALESCE(SUM(ul.billed_bytes), 0)::bigint
FROM usage_ledgers ul
JOIN exit_endpoints e ON e.id = ul.exit_endpoint_id
WHERE ul.traffic_source = 'access_line'
  AND ul.access_line_id = :'access_line_id'::uuid
  AND ul.exit_endpoint_id = :'exit_endpoint_id'::uuid
  AND e.outbound_type::text = :'outbound_type';
SQL
}

xrayc_real_e2e_wait_ledger_source_increase() {
  local before="${1:-}"
  local access_line_id="${2:-}"
  local exit_endpoint_id="${3:-}"
  local outbound_type="${4:-}"
  local timeout_seconds="${LEDGER_POLL_SECONDS:-90}"
  local interval_seconds="${LEDGER_POLL_INTERVAL_SECONDS:-5}"
  local elapsed=0
  local current=""

  if [[ -z "$before" ]]; then
    echo "Skipping ledger source check: DATABASE_URL not provided."
    return
  fi

  echo "Waiting for access_line ledger increase"
  while [[ "$elapsed" -le "$timeout_seconds" ]]; do
    current="$(xrayc_real_e2e_capture_ledger_baseline "$access_line_id" "$exit_endpoint_id" "$outbound_type")"
    if [[ "$current" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ && "$current" -gt "$before" ]]; then
      echo "access_line ledger increased"
      return
    fi
    sleep "$interval_seconds"
    elapsed=$((elapsed + interval_seconds))
  done

  echo "access_line ledger did not increase for expected endpoint before timeout" >&2
  exit 1
}
