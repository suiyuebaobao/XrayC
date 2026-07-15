# 用途：提供真实运行时稳定性 UAT 主脚本的函数实现。
# 说明：该文件由 scripts/real-runtime-stability-uat.sh source，不单独执行。
# 约束：函数依赖主脚本初始化的环境变量、临时目录和计数变量。

usage() {
  cat <<'USAGE'
usage: bash scripts/real-runtime-stability-uat.sh

Runs a long real-environment stability UAT without printing secrets.
The script loads .env.real-release or XRAYC_REAL_RELEASE_ENV_FILE.

Required:
  BASE_URL, DATABASE_URL, SUB_TOKEN or SUBSCRIPTION_URL,
  CLIENT_PROXY_URL, EXPECTED_EXIT_IP, ACCESS_NODE_ID, ACCESS_LINE_ID,
  EXIT_ENDPOINT_ID, USER_ACCESS_TOKEN.

Optional:
  UAT_PROFILE                      10m, 6h, or 24h. Default 10m.
  UAT_DURATION_SECONDS             Default 600 for 10m, 21600 for 6h.
  UAT_POLL_INTERVAL_SECONDS        Default 60.
  UAT_RUNTIME_WARMUP_SECONDS       Default 300.
  UAT_RUNTIME_STALE_SECONDS        Default 600.
  UAT_MAX_QUEUED_PROBE_AGE_SECONDS Default 600.
  UAT_MIN_USERS                    Default 1, 24h profile default 2.
  UAT_MIN_ACCESS_NODES             Default 1, 24h profile default 2.
  UAT_MIN_REPLICAS                 Default 1, 24h profile default 2.
  UAT_ACCESS_NODE_IDS              Comma-separated node ids; defaults to ACCESS_NODE_ID.
  UAT_ACCESS_LINE_IDS              Comma-separated line ids; defaults to ACCESS_LINE_ID.
  UAT_EXIT_ENDPOINT_IDS            Comma-separated endpoint ids; defaults to EXIT_ENDPOINT_ID.
  UAT_CLIENT_PROXY_URLS            Comma-separated proxy URLs; defaults to CLIENT_PROXY_URL.
  UAT_EXPECTED_EXIT_IPS            Comma-separated expected IPs; defaults to EXPECTED_EXIT_IP.
  UAT_USER_ACCESS_TOKENS           Comma-separated bearer tokens; defaults to USER_ACCESS_TOKEN.
  AGENT_TOKEN                      Optional access-agent token for explicit API post checks.
  UAT_AGENT_TOKENS                 Comma-separated agent tokens; defaults to AGENT_TOKEN.
  UAT_REQUIRE_AGENT_API=1          Require agent observation checks; default 0.
  UAT_AGENT_API_MODE               observe or post. Default observe; post writes heartbeat/config-result.
  UAT_REQUIRE_CONFIG_APPLIED=1     Require desired config to be applied after warmup. Default 1.
  UAT_MAX_PENDING_PROBES           Default 50.
  UAT_STAGE_RETRIES                Per-stage retry attempts before recording failure. Default 3.
  UAT_STAGE_RETRY_DELAY_SECONDS    Delay between stage retries. Default 5.
  PUBLIC_IP_URL                    Default https://api.ipify.org.
  UAT_CLIENT_TRAFFIC_URL           Default https://speed.cloudflare.com/__down?bytes=1048576.
  UAT_CLIENT_TRAFFIC_RATE          Default 64k.
  UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS Default 25.
  UAT_POST_TRAFFIC_SETTLE_SECONDS  Default 5.
  UAT_VALIDATE_ONLY=1              Validate env and gate profile only, then exit.
USAGE
}

authorization_header() {
  local token="${1:-}"
  case "$token" in
    *$'\n'*|*$'\r'*)
      echo "authorization token must not contain newlines" >&2
      exit 2
      ;;
    [Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]:*)
      echo "authorization token must be a raw token or Bearer token, not a full header" >&2
      exit 2
      ;;
    [Bb][Ee][Aa][Rr][Ee][Rr]\ *)
      token="${token#* }"
      ;;
  esac
  [[ -n "$token" ]] || { echo "authorization token must not be empty" >&2; exit 2; }
  printf 'Authorization: Bearer %s' "$token"
}

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  local old_ifs="$IFS"
  IFS=','
  read -r -a out_ref <<<"$raw"
  IFS="$old_ifs"
}

require_count_at_least() {
  local label="$1"
  local raw="$2"
  local min_count="$3"
  local values=()
  split_csv "$raw" values
  local count=0
  local value=""
  for value in "${values[@]}"; do
    [[ -n "$value" ]] && count=$((count + 1))
  done
  if [[ "$count" -lt "$min_count" ]]; then
    echo "${label} must contain at least ${min_count} value(s) for ${UAT_PROFILE} stability UAT" >&2
    exit 2
  fi
}

record_failure() {
  local label="$1"
  failure_count=$((failure_count + 1))
  printf '%s iteration=%s stage=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$iteration" "$label" >>"$failure_log"
  echo "stability UAT stage failed: ${label}; continuing diagnostics" >&2
}

run_stage() {
  local label="$1"
  shift
  local attempt=1
  while [[ "$attempt" -le "$UAT_STAGE_RETRIES" ]]; do
    if ( "$@" ); then
      if [[ "$attempt" -gt 1 ]]; then
        echo "stability UAT stage recovered after retry: ${label}"
      fi
      return 0
    fi
    if [[ "$attempt" -lt "$UAT_STAGE_RETRIES" ]]; then
      echo "stability UAT stage retry scheduled: ${label} attempt=${attempt}" >&2
      sleep "$UAT_STAGE_RETRY_DELAY_SECONDS"
    fi
    attempt=$((attempt + 1))
  done
  record_failure "$label"
  return 1
}

check_health() {
  local status_file="$tmp_dir/health.status"
  if ! curl --fail --silent --location --max-time "${CURL_TIMEOUT:-15}" \
    --output "$tmp_dir/health.body" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}/health" >"$status_file" 2>/dev/null; then
    echo "stability UAT health request failed; response redacted" >&2
    return 1
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *) echo "stability UAT health returned non-success; response redacted" >&2; return 1 ;;
  esac
}

check_subscription() {
  local subscription_file="$tmp_dir/subscription.yaml"
  xrayc_real_e2e_fetch_sensitive_url_to_file \
    "$subscription_url" \
    "$subscription_file" \
    "stability UAT subscription download failed"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"
}

post_agent_json() {
  local path="$1"
  local token="$2"
  local payload_file="$3"
  local label="$4"
  local status_file="$tmp_dir/${label}.status"
  if ! curl --fail --silent --location --max-time "${CURL_TIMEOUT:-15}" \
    --header "$(authorization_header "$token")" \
    --header "Content-Type: application/json" \
    --data-binary "@${payload_file}" \
    --output "$tmp_dir/${label}.body" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" >"$status_file" 2>/dev/null; then
    echo "stability UAT ${label} request failed; response redacted" >&2
    return 1
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *) echo "stability UAT ${label} returned non-success; response redacted" >&2; return 1 ;;
  esac
}

check_agent_api() {
  local node_index node_id token heartbeat_payload config_hash config_payload
  if [[ "$UAT_AGENT_API_MODE" == "observe" ]]; then
    check_agent_observation
    return
  fi
  for node_index in "${!uat_access_node_ids[@]}"; do
    node_id="${uat_access_node_ids[$node_index]}"
    [[ -n "$node_id" ]] || continue
    token="${uat_agent_tokens[$node_index]:-${uat_agent_tokens[0]:-}}"
    if [[ -z "$token" ]]; then
      if xrayc_real_e2e_bool_is_true "$UAT_REQUIRE_AGENT_API"; then
        echo "stability UAT missing agent token for configured node" >&2
        return 2
      fi
      continue
    fi
    heartbeat_payload="$tmp_dir/agent-heartbeat-${node_index}.json"
    printf '{"access_node_id":"%s"}\n' "$node_id" >"$heartbeat_payload"
    post_agent_json "/api/agent/access/heartbeat" "$token" "$heartbeat_payload" "agent-heartbeat-${node_index}"
    config_hash="$(python3 - "$tmp_dir/agent-heartbeat-${node_index}.body" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
print(payload.get("config_hash") or payload.get("desired_config_version") or "")
PY
)"
    if [[ -z "$config_hash" ]]; then
      echo "stability UAT agent heartbeat did not include desired config; response redacted" >&2
      return 1
    fi
    config_payload="$tmp_dir/agent-config-result-${node_index}.json"
    printf '{"access_node_id":"%s","success":true,"config_version":"%s"}\n' "$node_id" "$config_hash" >"$config_payload"
    post_agent_json "/api/agent/access/config-result" "$token" "$config_payload" "agent-config-result-${node_index}"
  done
}

check_agent_observation() {
  local missing=""
  if ! missing="$(
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
      -v "access_node_ids=$UAT_ACCESS_NODE_IDS" \
      -v "stale_seconds=$UAT_RUNTIME_STALE_SECONDS" \
      -v "require_config_applied=$UAT_REQUIRE_CONFIG_APPLIED" <<'SQL'
WITH access_nodes AS (
  SELECT trim(value)::uuid AS id, ordinality
  FROM unnest(string_to_array(:'access_node_ids', ',')) WITH ORDINALITY AS t(value, ordinality)
  WHERE trim(value) <> ''
),
checks AS (
  SELECT format('agent_observed_heartbeat_node_%s_missing', n.ordinality) AS name, NOT EXISTS (
    SELECT 1
    FROM public.access_nodes
    WHERE id = n.id
      AND last_heartbeat_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
      AND desired_config_hash IS NOT NULL
      AND desired_config_hash <> ''
  ) AS failed
  FROM access_nodes n
  UNION ALL
  SELECT format('agent_observed_config_node_%s_not_applied', n.ordinality), NOT EXISTS (
    SELECT 1
    FROM public.access_nodes
    WHERE id = n.id
      AND config_dirty = FALSE
      AND desired_config_hash IS NOT NULL
      AND applied_config_hash = desired_config_hash
  )
  FROM access_nodes n
  WHERE :'require_config_applied' = '1'
)
SELECT COALESCE(string_agg(name, ',' ORDER BY name), '')
FROM checks
WHERE failed;
SQL
  )"; then
    echo "stability UAT agent observation query failed; diagnostics redacted" >&2
    return 1
  fi
  if [[ -n "$missing" ]]; then
    if [[ "$SECONDS" -lt "$UAT_RUNTIME_WARMUP_SECONDS" ]]; then
      echo "stability UAT agent observation warming up"
      return
    fi
    echo "stability UAT agent observation failed: ${missing}" >&2
    return 1
  fi
}

generate_client_traffic() {
  local proxy_index proxy_url
  for proxy_index in "${!uat_proxy_urls[@]}"; do
    proxy_url="${uat_proxy_urls[$proxy_index]}"
    [[ -n "$proxy_url" ]] || continue
    if ! curl --fail --silent --location \
      --connect-timeout 8 \
      --max-time "$UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS" \
      --limit-rate "$UAT_CLIENT_TRAFFIC_RATE" \
      --proxy "$proxy_url" \
      --output /dev/null \
      "$UAT_CLIENT_TRAFFIC_URL" 2>/dev/null; then
      echo "stability UAT client traffic generation failed; response redacted" >&2
      return 1
    fi
  done
  sleep "$UAT_POST_TRAFFIC_SETTLE_SECONDS"
}

check_runtime_rows() {
  local missing=""
  local runtime_sql_file="$tmp_dir/runtime-check.sql"
  cat >"$runtime_sql_file" <<'SQL'
WITH
access_nodes AS (
  SELECT trim(value)::uuid AS id, ordinality
  FROM unnest(string_to_array(:'access_node_ids', ',')) WITH ORDINALITY AS t(value, ordinality)
  WHERE trim(value) <> ''
),
access_lines AS (
  SELECT trim(value)::uuid AS id, ordinality
  FROM unnest(string_to_array(:'access_line_ids', ',')) WITH ORDINALITY AS t(value, ordinality)
  WHERE trim(value) <> ''
),
exit_endpoints AS (
  SELECT trim(value)::uuid AS id, ordinality
  FROM unnest(string_to_array(:'exit_endpoint_ids', ',')) WITH ORDINALITY AS t(value, ordinality)
  WHERE trim(value) <> ''
),
checks AS (
  SELECT format('heartbeat_node_%s_stale', n.ordinality) AS name, NOT EXISTS (
    SELECT 1
    FROM public.access_nodes
    WHERE id = n.id
      AND last_heartbeat_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
      AND desired_config_hash IS NOT NULL
      AND desired_config_hash <> ''
  ) AS failed
  FROM access_nodes n
  UNION ALL
  SELECT format('config_node_%s_not_applied', n.ordinality), NOT EXISTS (
    SELECT 1
    FROM public.access_nodes
    WHERE id = n.id
      AND config_dirty = FALSE
      AND desired_config_hash IS NOT NULL
      AND applied_config_hash = desired_config_hash
  )
  FROM access_nodes n
  WHERE :'require_config_applied' = '1'
  UNION ALL
  SELECT format('metric_node_%s_missing', n.ordinality) AS name, NOT EXISTS (
    SELECT 1
    FROM access_line_metric_snapshots
    WHERE access_node_id = n.id
      AND collected_at >= :'started_at'::timestamptz
      AND collected_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  ) AS failed
  FROM access_nodes n
  UNION ALL
  SELECT format('metric_line_%s_missing', l.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_line_metric_snapshots
    WHERE access_line_id = l.id
      AND collected_at >= :'started_at'::timestamptz
      AND collected_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_lines l
  UNION ALL
  SELECT format('session_node_%s_missing', n.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_user_sessions
    WHERE access_node_id = n.id
      AND last_seen_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_nodes n
  UNION ALL
  SELECT format('session_line_%s_missing', l.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_user_sessions
    WHERE access_line_id = l.id
      AND last_seen_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_lines l
  UNION ALL
  SELECT format('traffic_snapshot_line_%s_missing', l.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_traffic_snapshots
    WHERE access_line_id = l.id
      AND collected_at >= :'started_at'::timestamptz
      AND collected_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_lines l
  UNION ALL
  SELECT format('traffic_snapshot_node_%s_missing', n.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_traffic_snapshots
    WHERE access_node_id = n.id
      AND collected_at >= :'started_at'::timestamptz
      AND collected_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_nodes n
  UNION ALL
  SELECT format('usage_ledger_line_%s_missing', l.ordinality), NOT EXISTS (
    SELECT 1
    FROM usage_ledgers
    WHERE access_line_id = l.id
      AND recorded_at >= :'started_at'::timestamptz
      AND recorded_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
      AND billed_bytes > 0
  )
  FROM access_lines l
  UNION ALL
  SELECT format('usage_ledger_node_%s_missing', n.ordinality), NOT EXISTS (
    SELECT 1
    FROM usage_ledgers ul
    JOIN public.access_lines al ON al.id = ul.access_line_id
    WHERE al.access_node_id = n.id
      AND ul.recorded_at >= :'started_at'::timestamptz
      AND ul.recorded_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
      AND ul.billed_bytes > 0
  )
  FROM access_nodes n
  UNION ALL
  SELECT format('line_probe_%s_missing', l.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_line_probes
    WHERE access_line_id = l.id
      AND probed_at >= :'started_at'::timestamptz
      AND probed_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
  )
  FROM access_lines l
  UNION ALL
  SELECT format('exit_probe_node_%s_endpoint_%s_missing', n.ordinality, e.ordinality), NOT EXISTS (
    SELECT 1
    FROM access_exit_probes
    WHERE access_node_id = n.id
      AND exit_endpoint_id = e.id
      AND probed_at >= :'started_at'::timestamptz
      AND probed_at >= now() - (:'stale_seconds'::BIGINT * interval '1 second')
      AND status <> 'queued'
  )
  FROM access_nodes n
  CROSS JOIN exit_endpoints e
  UNION ALL
  SELECT format('probe_queue_node_%s_over_limit', n.ordinality), (
    SELECT COUNT(*)
    FROM access_exit_probes p
    WHERE p.access_node_id = n.id
      AND p.status = 'queued'
      AND NOT EXISTS (
        SELECT 1
        FROM access_exit_probes r
        WHERE r.access_node_id = p.access_node_id
          AND r.exit_endpoint_id = p.exit_endpoint_id
          AND r.status <> 'queued'
          AND r.probed_at >= p.probed_at
      )
  ) > :'max_pending_probes'::BIGINT
  FROM access_nodes n
  UNION ALL
  SELECT format('probe_queue_node_%s_endpoint_%s_stale', n.ordinality, e.ordinality), EXISTS (
    SELECT 1
    FROM access_exit_probes
    WHERE access_node_id = n.id
      AND exit_endpoint_id = e.id
      AND status = 'queued'
      AND probed_at < now() - (:'queued_age_seconds'::BIGINT * interval '1 second')
      AND NOT EXISTS (
        SELECT 1
        FROM access_exit_probes r
        WHERE r.access_node_id = access_exit_probes.access_node_id
          AND r.exit_endpoint_id = access_exit_probes.exit_endpoint_id
          AND r.status <> 'queued'
          AND r.probed_at >= access_exit_probes.probed_at
      )
  )
  FROM access_nodes n
  CROSS JOIN exit_endpoints e
)
SELECT COALESCE(string_agg(name, ',' ORDER BY name), '')
FROM checks
WHERE failed;
SQL
  if ! missing="$(
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
      -v "access_node_ids=$UAT_ACCESS_NODE_IDS" \
      -v "access_line_ids=$UAT_ACCESS_LINE_IDS" \
      -v "exit_endpoint_ids=$UAT_EXIT_ENDPOINT_IDS" \
      -v "started_at=$started_at" \
      -v "stale_seconds=$UAT_RUNTIME_STALE_SECONDS" \
      -v "queued_age_seconds=$UAT_MAX_QUEUED_PROBE_AGE_SECONDS" \
      -v "max_pending_probes=$UAT_MAX_PENDING_PROBES" \
      -v "require_config_applied=$UAT_REQUIRE_CONFIG_APPLIED" \
      -f "$runtime_sql_file"
  )"; then
    echo "stability UAT runtime observation query failed; diagnostics redacted" >&2
    return 1
  fi
  if [[ -n "$missing" ]]; then
    if [[ "$SECONDS" -lt "$UAT_RUNTIME_WARMUP_SECONDS" ]]; then
      echo "stability UAT runtime observation warming up"
      return
    fi
    echo "stability UAT runtime observation failed: ${missing}" >&2
    return 1
  fi
}
