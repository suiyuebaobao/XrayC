# 真实 V2 中转池长时间数据 UAT 辅助函数。
# 由主 E2E 在新鲜客户端、中转、出口和订阅均恢复可用后按需调用。
# shellcheck shell=bash

real_v2_long_data_require_uint() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[0-9]+$ ]] || die "${name} must be an unsigned integer"
}

real_v2_long_data_require_positive_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

real_v2_long_data_user_ops() {
  local iteration="$1"
  local subscription_file="$TMP_DIR/long-data-subscription-${iteration}.yaml"
  xrayc_real_e2e_fetch_sensitive_url_to_file \
    "${BASE_URL}/sub/${sub_token}" \
    "$subscription_file" \
    "long data UAT subscription download failed"
  grep -Fq "$subscription_proxy_name" "$subscription_file" \
    || die "long data UAT subscription does not include relay line"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"

  api_auth_get /api/user/subscription "$TMP_DIR/long-data-user-subscription-${iteration}.json" "$user_token"
  api_auth_get /api/user/usage "$TMP_DIR/long-data-user-usage-${iteration}.json" "$user_token"
  api_auth_get /api/orders "$TMP_DIR/long-data-user-orders-${iteration}.json" "$user_token"
  api_auth_get /api/admin/access-operations/summary "$TMP_DIR/long-data-admin-summary-${iteration}.json" "$admin_token"
  api_auth_get "/api/admin/access-operations/ledger-ranking?limit=10" \
    "$TMP_DIR/long-data-admin-ledger-${iteration}.json" "$admin_token"
}

real_v2_long_data_generate_client_traffic() {
  local traffic_urls="$1"
  local traffic_rate="$2"
  local max_time="$3"
  local quoted_url quoted_rate url
  quoted_rate="$(rate_limit_shell_quote "$traffic_rate")"
  local old_ifs="$IFS"
  IFS=','
  read -r -a traffic_url_items <<<"$traffic_urls"
  IFS="$old_ifs"
  for url in "${traffic_url_items[@]}"; do
    url="${url#"${url%%[![:space:]]*}"}"
    url="${url%"${url##*[![:space:]]}"}"
    [[ -n "$url" ]] || continue
    quoted_url="$(rate_limit_shell_quote "$url")"
    if remote_exec 1 "curl --fail --silent --show-error --connect-timeout 8 --max-time ${max_time} --limit-rate ${quoted_rate} --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} ${quoted_url} >/dev/null" \
      >/dev/null 2>&1; then
      return
    fi
  done
  die "long data UAT real client traffic failed"
}

real_v2_long_data_wait_runtime_rows() {
  local started_at="$1"
  local missing=""
  missing="$(psql_db -XAtq \
    -v ON_ERROR_STOP=1 \
    -v started_at="$started_at" \
    -v access_node_id="$access_node_id" \
    -v access_line_id="$access_line_id" \
    -v xray_user_key="$user_xray_key" <<'SQL'
WITH checks AS (
  SELECT 'heartbeat' AS name, NOT EXISTS (
    SELECT 1 FROM access_nodes
    WHERE id = :'access_node_id'::uuid
      AND last_heartbeat_at >= now() - interval '120 seconds'
      AND config_dirty = FALSE
  ) AS failed
  UNION ALL
  SELECT 'traffic_snapshot', NOT EXISTS (
    SELECT 1 FROM access_traffic_snapshots
    WHERE access_line_id = :'access_line_id'::uuid
      AND access_node_id = :'access_node_id'::uuid
      AND collected_at >= :'started_at'::timestamptz
  )
  UNION ALL
  SELECT 'session', NOT EXISTS (
    SELECT 1 FROM access_user_sessions
    WHERE access_line_id = :'access_line_id'::uuid
      AND access_node_id = :'access_node_id'::uuid
      AND xray_user_key = :'xray_user_key'
      AND last_seen_at >= now() - interval '120 seconds'
  )
)
SELECT COALESCE(string_agg(name, ',' ORDER BY name), '')
FROM checks
WHERE failed;
SQL
)"
  [[ -z "$missing" ]] || die "long data UAT runtime rows missing: ${missing}"
}

run_real_v2_long_data_uat_if_requested() {
  local duration="${XRAYC_REAL_E2E_LONG_DATA_SECONDS:-0}"
  real_v2_long_data_require_uint XRAYC_REAL_E2E_LONG_DATA_SECONDS "$duration"
  if [[ "$duration" == "0" ]]; then
    return
  fi

  local poll_seconds="${XRAYC_REAL_E2E_LONG_DATA_POLL_SECONDS:-60}"
  local traffic_urls="${XRAYC_REAL_E2E_LONG_DATA_TRAFFIC_URLS:-${XRAYC_REAL_E2E_LONG_DATA_TRAFFIC_URL:-https://speed.cloudflare.com/__down?bytes=524288,https://proof.ovh.net/files/1Mb.dat,https://api.ipify.org}}"
  local traffic_rate="${XRAYC_REAL_E2E_LONG_DATA_TRAFFIC_RATE:-64k}"
  local traffic_max_time="${XRAYC_REAL_E2E_LONG_DATA_TRAFFIC_MAX_TIME_SECONDS:-60}"
  real_v2_long_data_require_positive_int XRAYC_REAL_E2E_LONG_DATA_POLL_SECONDS "$poll_seconds"
  real_v2_long_data_require_positive_int XRAYC_REAL_E2E_LONG_DATA_TRAFFIC_MAX_TIME_SECONDS "$traffic_max_time"

  local started_at deadline iteration remaining billing_baseline ledger_count ledger_billed
  started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  deadline=$((SECONDS + duration))
  iteration=0
  billing_baseline="$(xrayc_real_e2e_capture_billing_baseline "$BASE_URL" "$user_token" "$TMP_DIR")"
  echo "real_v2_relay_pool_e2e: long_data_uat_started duration_seconds=${duration}"

  while [[ "$SECONDS" -lt "$deadline" ]]; do
    iteration=$((iteration + 1))
    real_v2_long_data_user_ops "$iteration"
    real_v2_long_data_generate_client_traffic "$traffic_urls" "$traffic_rate" "$traffic_max_time"
    sleep 5
    real_v2_long_data_wait_runtime_rows "$started_at"
    echo "real_v2_relay_pool_e2e: long_data_iteration_ok iteration=${iteration}"
    remaining=$((deadline - SECONDS))
    [[ "$remaining" -le 0 ]] && break
    if [[ "$remaining" -lt "$poll_seconds" ]]; then
      sleep "$remaining"
    else
      sleep "$poll_seconds"
    fi
  done

  xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$user_token" "$billing_baseline" "$TMP_DIR"
  IFS=$'\t' read -r ledger_count ledger_billed < <(psql_db -XAtq -F $'\t' \
    -v ON_ERROR_STOP=1 \
    -v started_at="$started_at" \
    -v line_id="$access_line_id" \
    -v user_id="$user_id" \
    -v exit_endpoint_id="$assigned_exit_endpoint_id" <<'SQL'
SELECT COUNT(*)::text, COALESCE(SUM(billed_bytes), 0)::text
FROM usage_ledgers
WHERE access_line_id = :'line_id'::uuid
  AND user_id = :'user_id'::uuid
  AND exit_endpoint_id = :'exit_endpoint_id'::uuid
  AND traffic_source = 'access_line'
  AND recorded_at >= :'started_at'::timestamptz;
SQL
)
  [[ "${ledger_billed:-0}" =~ ^[0-9]+$ && "$ledger_billed" -gt 0 ]] \
    || die "long data UAT ledger did not grow"
  echo "real_v2_relay_pool_e2e: long_data_uat_completed iterations=${iteration} ledger_count=${ledger_count} billed_bytes=${ledger_billed}"
}
