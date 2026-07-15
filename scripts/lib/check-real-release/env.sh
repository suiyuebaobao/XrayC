#!/usr/bin/env bash
# 中文说明：提供真实发布门禁脚本使用的环境变量清单。
# 中文说明：集中保存环境占位符、必填变量和公共 IP 校验函数。
# 中文说明：集中保存直接模式运行时观测数据库断言函数。
# 中文说明：本文件由 scripts/check-real-release.sh source，不应单独执行。
# 中文说明：拆分后入口脚本保留发布门禁编排，避免单文件过长。

print_required_env() {
  cat <<'EOF'
BASE_URL
XRAYC_ENV
SEED_DEMO_DATA
JWT_SECRET
JWT_EXPIRES_IN
JWT_REFRESH_EXPIRES_IN
REAL_RELEASE_MATRIX_MODE
DEPLOY_ARTIFACT_TOKEN
SUB_TOKEN or SUBSCRIPTION_URL
XRAYC_REAL_E2E_INVENTORY
XRAYC_REAL_E2E_TARGET
USER_ACCESS_TOKEN
DATABASE_URL (production PostgreSQL control-plane URL)
E2E_USER_ACCOUNT
E2E_USER_PASSWORD
E2E_ADMIN_ACCOUNT
E2E_ADMIN_PASSWORD
Required release matrix mode:
REAL_RELEASE_MATRIX_MODE=relay
Relay mode credentials:
ADMIN_LOGIN_ACCOUNT
ADMIN_LOGIN_PASSWORD
Diagnostic direct mode only; not accepted by this full release gate:
AGENT_TOKEN
ACCESS_NODE_ID
ACCESS_LINE_ID
EXIT_ENDPOINT_ID
CLIENT_PROXY_URL
EXPECTED_EXIT_IP
EXPECTED_ACCESS_SERVERS
All matrix modes protocol assets:
THIRD_PARTY_SOCKS_RAW_URL or THIRD_PARTY_SOCKS_HOST+THIRD_PARTY_SOCKS_PORT
EXPECTED_EXIT_IP_SOCKS
THIRD_PARTY_HTTP_RAW_URL or THIRD_PARTY_HTTP_HOST+THIRD_PARTY_HTTP_PORT+credentials
EXPECTED_EXIT_IP_HTTP
THIRD_PARTY_VLESS_RAW_URL or THIRD_PARTY_HOST/THIRD_PARTY_VLESS_HOST+THIRD_PARTY_VLESS_PORT+UUID
EXPECTED_EXIT_IP_VLESS
THIRD_PARTY_TROJAN_RAW_URL or THIRD_PARTY_TROJAN_HOST+THIRD_PARTY_TROJAN_PORT+password
EXPECTED_EXIT_IP_TROJAN
THIRD_PARTY_SHADOWSOCKS_RAW_URL or THIRD_PARTY_SHADOWSOCKS_HOST+THIRD_PARTY_SHADOWSOCKS_PORT+method/password
EXPECTED_EXIT_IP_SHADOWSOCKS
THIRD_PARTY_HY2_RAW_URL or THIRD_PARTY_HY2_HOST+THIRD_PARTY_HY2_PORT+password/auth
EXPECTED_EXIT_IP_HY2
Relay mode endpoint IDs are written by prepare-real-protocol-matrix-endpoints.sh:
EXIT_ENDPOINT_ID_SOCKS
EXIT_ENDPOINT_ID_HTTP
EXIT_ENDPOINT_ID_VLESS
EXIT_ENDPOINT_ID_TROJAN
EXIT_ENDPOINT_ID_SHADOWSOCKS
EXIT_ENDPOINT_ID_HY2
Direct mode per-protocol client proxy URLs:
CLIENT_PROXY_URL_SOCKS
CLIENT_PROXY_URL_HTTP
CLIENT_PROXY_URL_VLESS
CLIENT_PROXY_URL_TROJAN
CLIENT_PROXY_URL_SHADOWSOCKS
CLIENT_PROXY_URL_HY2
Required long-stability gate:
RUN_REAL_STABILITY_UAT=1
UAT_PROFILE=10m
UAT_DURATION_SECONDS=600
UAT_ACCESS_NODE_IDS
UAT_ACCESS_LINE_IDS
UAT_EXIT_ENDPOINT_IDS
UAT_CLIENT_PROXY_URLS
UAT_EXPECTED_EXIT_IPS
UAT_USER_ACCESS_TOKENS
Required access-agent traffic backlog recovery gate:
RUN_ACCESS_TRAFFIC_BACKLOG_UAT=1
XRAY_USER_KEY
BACKLOG_TARGET_INDEX=2
BACKLOG_INSTALL_DIR=/opt/xrayc-real-relay
BACKLOG_WAIT_SECONDS=180
BACKLOG_TRAFFIC_ROUNDS=2
Manual backlog preflight only:
BACKLOG_VALIDATE_ONLY=1
Optional 24h long-stability gate:
RUN_REAL_STABILITY_UAT_24H=1
Required large runtime loadtest gate:
RUN_RUNTIME_LOADTEST=1
RUNTIME_LOADTEST_DATABASE_URL
RUNTIME_LOADTEST_PROFILE=large
Required isolated HTTP/API runtime loadtest gate:
RUNTIME_HTTP_LOADTEST_PROFILE=large
RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1
Required multi-replica auth UAT:
RUN_AUTH_HA_UAT=1
ADMIN_ACCESS_TOKEN or E2E_ADMIN_ACCOUNT/E2E_ADMIN_PASSWORD
Required multi-replica auth stability UAT:
RUN_AUTH_HA_STABILITY_UAT=1
AUTH_HA_STABILITY_PROFILE=10m
AUTH_HA_STABILITY_DURATION_SECONDS=600
Required operator recovery UAT:
RUN_OPS_MISTAKE_RECOVERY_UAT=1
Required large operator recovery UAT:
RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT=1
OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL or RUNTIME_LOADTEST_DATABASE_URL
Required multi-user real traffic UAT:
RUN_REAL_MULTI_USER_TRAFFIC_UAT=1
XRAYC_REAL_MULTI_USER_COUNT=10
XRAYC_REAL_MULTI_USER_DURATION_SECONDS=2400
XRAYC_REAL_MULTI_USER_ROUNDS=2
XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES=3
Required real subscription client compatibility gate:
RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT=1
RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC=1
Optional checker images when local binaries are unavailable:
XRAYC_MIHOMO_IMAGE=metacubex/mihomo:latest
Required user inbound protocol matrix gate:
RUN_REAL_ACCESS_INBOUND_MATRIX_E2E=1
RUN_REAL_RUNTIME_RESTORE_DEPLOY=1
REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS=vless,trojan,shadowsocks
EOF
}

config_error() {
  exit 2
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  return 1
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "${name} is required for real release gate" >&2
    config_error
  elif value_is_placeholder "${!name:-}"; then
    echo "${name} still contains a placeholder" >&2
    config_error
  fi
}

require_any_env() {
  local name=""
  for name in "$@"; do
    if [[ -n "${!name:-}" ]] && ! value_is_placeholder "${!name:-}"; then
      return
    fi
  done
  echo "one of the required release gate variables is missing" >&2
  config_error
}

require_gate_enabled() {
  local name="$1"
  if ! xrayc_real_e2e_bool_is_true "${!name:-}"; then
    echo "${name}=1 is required for the full real release gate" >&2
    config_error
  fi
}

require_public_ip() {
  local name="$1"
  require_env "$name"
  if ! python3 - "${!name:-}" <<'PY' >/dev/null 2>&1
import ipaddress
import sys

try:
    ip = ipaddress.ip_address(sys.argv[1])
except ValueError:
    raise SystemExit(1)
raise SystemExit(0 if ip.is_global else 1)
PY
  then
    echo "${name} must be a public IPv4 or IPv6 address" >&2
    config_error
  fi
}

require_matrix_protocol_env() {
  local protocol=""
  local protocols=(socks http vless trojan shadowsocks hy2)
  local suffix=""
  local matrix_mode="${REAL_RELEASE_MATRIX_MODE:-relay}"

  for protocol in "${protocols[@]}"; do
    case "$protocol" in
      socks)
        suffix="SOCKS"
        ;;
      http)
        suffix="HTTP"
        ;;
      vless)
        suffix="VLESS"
        ;;
      trojan)
        suffix="TROJAN"
        ;;
      shadowsocks)
        suffix="SHADOWSOCKS"
        ;;
      hy2)
        suffix="HY2"
        ;;
      *)
        config_error
        ;;
    esac
    require_public_ip "EXPECTED_EXIT_IP_${suffix}"
    if [[ "$matrix_mode" == "direct" ]]; then
      require_env "EXIT_ENDPOINT_ID_${suffix}"
      require_env "CLIENT_PROXY_URL_${suffix}"
    fi
  done
}

assert_recent_runtime_observation() {
  if ! command -v psql >/dev/null 2>&1; then
    echo "psql is required for real runtime observation gate" >&2
    config_error
  fi

  local timeout_seconds="${REAL_RUNTIME_OBSERVATION_TIMEOUT_SECONDS:-180}"
  local interval_seconds="${REAL_RUNTIME_OBSERVATION_POLL_INTERVAL_SECONDS:-5}"
  local elapsed=0
  local result=""
  while [[ "$elapsed" -le "$timeout_seconds" ]]; do
    result="$(
      xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq \
        -v "access_node_id=$ACCESS_NODE_ID" \
        -v "access_line_id=$ACCESS_LINE_ID" \
        -v "exit_endpoint_id=$EXIT_ENDPOINT_ID" \
        -v "gate_started_at=$GATE_STARTED_AT" <<'SQL'
WITH checks AS (
  SELECT 'metric' AS name, EXISTS (
    SELECT 1
    FROM access_line_metric_snapshots
    WHERE access_node_id = :'access_node_id'::uuid
      AND access_line_id = :'access_line_id'::uuid
      AND collected_at >= :'gate_started_at'::timestamptz
  ) AS ok
  UNION ALL
  SELECT 'session', EXISTS (
    SELECT 1
    FROM access_user_sessions
    WHERE access_node_id = :'access_node_id'::uuid
      AND access_line_id = :'access_line_id'::uuid
      AND last_seen_at >= :'gate_started_at'::timestamptz
  )
  UNION ALL
  SELECT 'line_probe', EXISTS (
    SELECT 1
    FROM access_line_probes
    WHERE access_line_id = :'access_line_id'::uuid
      AND probed_at >= :'gate_started_at'::timestamptz
  )
  UNION ALL
  SELECT 'exit_probe', EXISTS (
    SELECT 1
    FROM access_exit_probes
    WHERE access_node_id = :'access_node_id'::uuid
      AND exit_endpoint_id = :'exit_endpoint_id'::uuid
      AND probed_at >= :'gate_started_at'::timestamptz
  )
)
SELECT COALESCE(string_agg(name, ',' ORDER BY name), '')
FROM checks
WHERE NOT ok;
SQL
    )"
    if [[ -z "$result" ]]; then
      echo "real runtime observation: passed"
      return
    fi
    sleep "$interval_seconds"
    elapsed=$((elapsed + interval_seconds))
  done
  echo "real runtime observation missing required records; missing categories redacted to safe labels: ${result}" >&2
  exit 1
}
