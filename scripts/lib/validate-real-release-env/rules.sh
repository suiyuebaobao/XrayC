#!/usr/bin/env bash
# 用途：承载真实发布环境变量的门禁规则，供主脚本加载执行。
# 说明：此文件只调用主脚本定义的校验函数，不直接打印或泄露密钥值。
# 约束：仅放置 validate-real-release-env.sh 拆分出的规则逻辑。
# 静态覆盖：RUN_REAL_STABILITY_UAT_24H、UAT_DURATION_SECONDS、AUTH_HA_STABILITY_DURATION_SECONDS。
# 直连诊断变量：CLIENT_PROXY_URL_SOCKS、CLIENT_PROXY_URL_HTTP、CLIENT_PROXY_URL_TROJAN。
# 直连诊断变量：CLIENT_PROXY_URL_SHADOWSOCKS、CLIENT_PROXY_URL_HY2。
# 矩阵端点变量：EXIT_ENDPOINT_ID_SOCKS、EXIT_ENDPOINT_ID_HTTP、EXIT_ENDPOINT_ID_TROJAN。
# 矩阵端点变量：EXIT_ENDPOINT_ID_SHADOWSOCKS、EXIT_ENDPOINT_ID_HY2。
# 预期出口变量：EXPECTED_EXIT_IP_SOCKS、EXPECTED_EXIT_IP_HTTP、EXPECTED_EXIT_IP_HY2。
# 长稳对象变量：UAT_ACCESS_NODE_IDS、UAT_ACCESS_LINE_IDS、UAT_EXIT_ENDPOINT_IDS。
# 订阅兼容变量：RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT、RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC、XRAYC_MIHOMO_IMAGE。

parse_real_release_duration_seconds() {
  local value="${1:-}"
  python3 - "$value" <<'PY' 2>/dev/null
import sys

value = sys.argv[1].strip()
if not value:
    raise SystemExit(1)
suffix = value[-1:].lower()
multiplier = 1
number = value
if suffix in {"s", "m", "h", "d"}:
    number = value[:-1].strip()
    multiplier = {"s": 1, "m": 60, "h": 3600, "d": 86400}[suffix]
try:
    seconds = int(number) * multiplier
except ValueError:
    raise SystemExit(1)
if seconds <= 0:
    raise SystemExit(1)
print(seconds)
PY
}

require_duration_between_seconds() {
  local name="$1"
  local min_seconds="$2"
  local max_seconds="$3"
  local label="$4"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  local seconds
  if ! seconds="$(parse_real_release_duration_seconds "$value")"; then
    report "${name} must be a positive duration with optional s/m/h/d suffix"
    return 0
  fi
  if (( seconds < min_seconds || seconds > max_seconds )); then
    report "${name} must be ${label}"
  fi
}

require_positive_integer_value() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if ! [[ "$value" =~ ^[0-9]+$ ]] || (( value <= 0 )); then
    report "${name} must be a positive integer"
  fi
}

require_non_negative_integer_value() {
  local name="$1"
  require_value "$name"
  local value="${!name:-}"
  value_ready "$value" || return 0
  if ! [[ "$value" =~ ^[0-9]+$ ]]; then
    report "${name} must be a non-negative integer"
  fi
}

require_production_jwt_runtime_env() {
  require_value JWT_SECRET
  local jwt_secret="${JWT_SECRET:-}"
  if value_ready "$jwt_secret" && (( ${#jwt_secret} < 32 )); then
    report "JWT_SECRET must be at least 32 characters for production"
  fi
  require_duration_between_seconds JWT_EXPIRES_IN 60 1800 "between 60s and 30m in production"
  require_duration_between_seconds JWT_REFRESH_EXPIRES_IN 3600 2592000 "between 1h and 30d in production"
}

run_real_release_env_validation() {
  require_exact_value XRAYC_ENV production
  require_exact_value SEED_DEMO_DATA false
  require_production_jwt_runtime_env
  case "${REAL_RELEASE_MATRIX_MODE:-relay}" in
    relay) ;;
    direct) report "REAL_RELEASE_MATRIX_MODE=direct is diagnostic only; full real release requires relay" ;;
    *) report "REAL_RELEASE_MATRIX_MODE must be relay" ;;
  esac
  require_https_or_private_http BASE_URL
  require_value DEPLOY_ARTIFACT_TOKEN
  require_any_value "SUB_TOKEN or SUBSCRIPTION_URL" SUB_TOKEN SUBSCRIPTION_URL
  if [[ -n "${SUBSCRIPTION_URL:-}" ]]; then
    require_url_scheme SUBSCRIPTION_URL
  fi
  require_json_file XRAYC_REAL_E2E_INVENTORY
  require_optional_file XRAYC_SERVER_ACCOUNT_FILE
  require_value XRAYC_REAL_E2E_TARGET
  require_value USER_ACCESS_TOKEN
  require_postgres_url DATABASE_URL
  require_value E2E_USER_ACCOUNT
  require_value E2E_USER_PASSWORD
  require_value E2E_ADMIN_ACCOUNT
  require_value E2E_ADMIN_PASSWORD
  for gate_name in \
    RUN_REAL_ACCESS_INBOUND_MATRIX_E2E \
    RUN_REAL_RUNTIME_RESTORE_DEPLOY \
    RUN_REAL_STABILITY_UAT \
    RUN_ACCESS_TRAFFIC_BACKLOG_UAT \
    RUN_RUNTIME_LOADTEST \
    RUN_AUTH_HA_UAT \
    RUN_AUTH_HA_STABILITY_UAT \
    RUN_OPS_MISTAKE_RECOVERY_UAT \
    RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT \
    RUN_REAL_MULTI_USER_TRAFFIC_UAT \
    RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT \
    RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC; do
    require_enabled_gate "$gate_name"
  done
  for forbidden_name in \
    UAT_VALIDATE_ONLY \
    BACKLOG_VALIDATE_ONLY \
    BACKLOG_SKIP_AGENT_RESTART \
    RUNTIME_LOADTEST_VALIDATE_ONLY \
    RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY \
    AUTH_HA_STABILITY_VALIDATE_ONLY \
    OPS_MISTAKE_RECOVERY_UAT_VALIDATE_ONLY \
    OPS_MISTAKE_RECOVERY_LARGE_UAT_VALIDATE_ONLY; do
    forbid_true_flag "$forbidden_name"
  done
  if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "direct" ]]; then
    require_value AGENT_TOKEN
    require_uuid ACCESS_NODE_ID
    require_uuid ACCESS_LINE_ID
    require_uuid EXIT_ENDPOINT_ID
    require_url_scheme CLIENT_PROXY_URL
    require_public_ip EXPECTED_EXIT_IP
    require_value EXPECTED_ACCESS_SERVERS
  else
    require_value ADMIN_LOGIN_ACCOUNT
    require_value ADMIN_LOGIN_PASSWORD
  fi

  require_protocol_group SOCKS THIRD_PARTY_SOCKS_RAW_URL THIRD_PARTY_SOCKS_HOST THIRD_PARTY_SOCKS_PORT
  require_protocol_group HTTP THIRD_PARTY_HTTP_RAW_URL THIRD_PARTY_HTTP_HOST THIRD_PARTY_HTTP_PORT
  require_any_value "THIRD_PARTY_HTTP_PASSWORD, THIRD_PARTY_HTTP_KEY, or raw URL credentials" THIRD_PARTY_HTTP_PASSWORD THIRD_PARTY_HTTP_KEY THIRD_PARTY_HTTP_RAW_URL
  if value_ready "${THIRD_PARTY_VLESS_RAW_URL:-}"; then
    require_url_scheme THIRD_PARTY_VLESS_RAW_URL
  else
    require_any_value "THIRD_PARTY_HOST or THIRD_PARTY_VLESS_HOST" THIRD_PARTY_HOST THIRD_PARTY_VLESS_HOST
    require_host_if_ready THIRD_PARTY_HOST
    require_host_if_ready THIRD_PARTY_VLESS_HOST
    require_value THIRD_PARTY_VLESS_PORT
    require_port_if_ready THIRD_PARTY_VLESS_PORT
    require_value THIRD_PARTY_UUID
  fi
  require_public_ip EXPECTED_EXIT_IP_VLESS
  require_matrix_endpoint_uuid EXIT_ENDPOINT_ID_VLESS
  if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "direct" ]]; then
    require_url_scheme CLIENT_PROXY_URL_VLESS
  fi
  if [[ "${THIRD_PARTY_SECURITY:-${THIRD_PARTY_VLESS_SECURITY:-}}" == "reality" ]] \
    || value_ready "${THIRD_PARTY_PUBLIC_KEY:-}" \
    || value_ready "${THIRD_PARTY_VLESS_PUBLIC_KEY:-}"; then
    require_any_value "THIRD_PARTY_PUBLIC_KEY or THIRD_PARTY_VLESS_PUBLIC_KEY" THIRD_PARTY_PUBLIC_KEY THIRD_PARTY_VLESS_PUBLIC_KEY
  fi
  require_protocol_group TROJAN THIRD_PARTY_TROJAN_RAW_URL THIRD_PARTY_TROJAN_HOST THIRD_PARTY_TROJAN_PORT
  require_any_value "THIRD_PARTY_TROJAN_PASSWORD, THIRD_PARTY_TROJAN_KEY, or raw URL credentials" THIRD_PARTY_TROJAN_PASSWORD THIRD_PARTY_TROJAN_KEY THIRD_PARTY_TROJAN_RAW_URL
  require_trojan_tls_ready
  require_protocol_group SHADOWSOCKS THIRD_PARTY_SHADOWSOCKS_RAW_URL THIRD_PARTY_SHADOWSOCKS_HOST THIRD_PARTY_SHADOWSOCKS_PORT
  require_any_value "THIRD_PARTY_SHADOWSOCKS_PASSWORD, THIRD_PARTY_SHADOWSOCKS_KEY, or raw URL credentials" THIRD_PARTY_SHADOWSOCKS_PASSWORD THIRD_PARTY_SHADOWSOCKS_KEY THIRD_PARTY_SHADOWSOCKS_RAW_URL
  require_protocol_group HY2 THIRD_PARTY_HY2_RAW_URL THIRD_PARTY_HY2_HOST THIRD_PARTY_HY2_PORT
  require_any_value "THIRD_PARTY_HY2_PASSWORD, THIRD_PARTY_HY2_AUTH, THIRD_PARTY_HY2_KEY, or raw URL credentials" THIRD_PARTY_HY2_PASSWORD THIRD_PARTY_HY2_AUTH THIRD_PARTY_HY2_KEY THIRD_PARTY_HY2_RAW_URL

  if [[ "${RUN_REAL_ACCESS_INBOUND_MATRIX_E2E:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_value REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS
    if value_ready "${REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS:-}"; then
      inbound_protocols=",${REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS,,},"
      inbound_protocols="${inbound_protocols// /}"
      inbound_protocols="${inbound_protocols//-/_}"
      for required_protocol in \
        vless \
        trojan \
        shadowsocks; do
        if [[ "$inbound_protocols" != *",${required_protocol},"* ]]; then
          report "REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS must include ${required_protocol}"
        fi
      done
      IFS=',' read -r -a inbound_protocol_items <<< "${REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS,,}"
      for inbound_protocol in "${inbound_protocol_items[@]}"; do
        inbound_protocol="${inbound_protocol// /}"
        inbound_protocol="${inbound_protocol//-/_}"
        case "$inbound_protocol" in
          vless|trojan|shadowsocks) ;;
          *) report "REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS contains unsupported protocol" ;;
        esac
      done
    fi
    for name in \
      REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_VLESS \
      REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_TROJAN \
      REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_SHADOWSOCKS; do
      if value_ready "${!name:-}"; then
        require_public_ip "$name"
      fi
    done
  fi

  if [[ "${RUN_REAL_STABILITY_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    case "${UAT_PROFILE:-10m}" in
      10m|6h|24h) ;;
      *) report "UAT_PROFILE must be 10m, 6h or 24h" ;;
    esac
    if [[ "${UAT_PROFILE:-10m}" == "10m" && "${UAT_DURATION_SECONDS:-600}" != "600" ]]; then
      report "UAT_DURATION_SECONDS must be 600 for the 10m release profile"
    fi
    require_url_scheme CLIENT_PROXY_URL
    require_public_ip EXPECTED_EXIT_IP
    require_uuid ACCESS_NODE_ID
    require_uuid ACCESS_LINE_ID
    require_uuid EXIT_ENDPOINT_ID
    require_value USER_ACCESS_TOKEN
    require_any_value "UAT_CLIENT_PROXY_URLS or CLIENT_PROXY_URL" UAT_CLIENT_PROXY_URLS CLIENT_PROXY_URL
    require_any_value "UAT_EXPECTED_EXIT_IPS or EXPECTED_EXIT_IP" UAT_EXPECTED_EXIT_IPS EXPECTED_EXIT_IP
    require_any_value "UAT_USER_ACCESS_TOKENS or USER_ACCESS_TOKEN" UAT_USER_ACCESS_TOKENS USER_ACCESS_TOKEN
  fi

  if [[ "${RUN_ACCESS_TRAFFIC_BACKLOG_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_url_scheme CLIENT_PROXY_URL
    require_any_value "SUB_TOKEN or SUBSCRIPTION_URL" SUB_TOKEN SUBSCRIPTION_URL
    require_uuid ACCESS_NODE_ID
    require_uuid ACCESS_LINE_ID
    require_json_file XRAYC_REAL_E2E_INVENTORY
  fi

  if [[ "${RUN_RUNTIME_LOADTEST:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_postgres_url RUNTIME_LOADTEST_DATABASE_URL
    if [[ "${RUNTIME_LOADTEST_DATABASE_URL:-}" == "${DATABASE_URL:-}" ]]; then
      report "RUNTIME_LOADTEST_DATABASE_URL must not reuse DATABASE_URL"
    fi
    require_exact_value RUNTIME_LOADTEST_PROFILE large
  fi

  require_exact_value RUNTIME_HTTP_LOADTEST_PROFILE large
  require_exact_value RUNTIME_HTTP_LOADTEST_REQUIRE_FULL 1
  if [[ "${RUNTIME_HTTP_LOADTEST_REQUIRE_FULL:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_value USER_ACCESS_TOKEN
    require_value ADMIN_ACCESS_TOKEN
    require_value AGENT_TOKEN
    require_uuid ACCESS_NODE_ID
    require_uuid ACCESS_LINE_ID
    require_uuid EXIT_ENDPOINT_ID
    require_value XRAY_USER_KEY
  fi

  if [[ "${RUN_AUTH_HA_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_value E2E_ADMIN_ACCOUNT
    require_value E2E_ADMIN_PASSWORD
  fi

  if [[ "${RUN_AUTH_HA_STABILITY_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    case "${AUTH_HA_STABILITY_PROFILE:-10m}" in
      10m|6h|24h) ;;
      *) report "AUTH_HA_STABILITY_PROFILE must be 10m, 6h or 24h" ;;
    esac
    if [[ "${AUTH_HA_STABILITY_PROFILE:-10m}" == "10m" && "${AUTH_HA_STABILITY_DURATION_SECONDS:-600}" != "600" ]]; then
      report "AUTH_HA_STABILITY_DURATION_SECONDS must be 600 for the 10m release profile"
    fi
    require_value E2E_ADMIN_ACCOUNT
    require_value E2E_ADMIN_PASSWORD
  fi

  if [[ "${RUN_OPS_MISTAKE_RECOVERY_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_https_or_private_http BASE_URL
    require_postgres_url DATABASE_URL
  fi

  if [[ "${RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    if [[ -n "${OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL:-}" ]]; then
      require_postgres_url OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL
      if [[ "${OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL:-}" == "${DATABASE_URL:-}" ]]; then
        report "OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL must not reuse DATABASE_URL"
      fi
    else
      require_postgres_url RUNTIME_LOADTEST_DATABASE_URL
    fi
  fi

  if [[ "${RUN_REAL_MULTI_USER_TRAFFIC_UAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_json_file XRAYC_REAL_E2E_INVENTORY
    require_positive_integer_value XRAYC_REAL_MULTI_USER_COUNT
    require_positive_integer_value XRAYC_REAL_MULTI_USER_DURATION_SECONDS
    require_positive_integer_value XRAYC_REAL_MULTI_USER_ROUNDS
    require_non_negative_integer_value XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES
    if value_ready "${XRAYC_REAL_MULTI_USER_COUNT:-}" && (( XRAYC_REAL_MULTI_USER_COUNT < 10 )); then
      report "XRAYC_REAL_MULTI_USER_COUNT must be at least 10 for the full real release gate"
    fi
    if value_ready "${XRAYC_REAL_MULTI_USER_DURATION_SECONDS:-}" && (( XRAYC_REAL_MULTI_USER_DURATION_SECONDS < 2400 )); then
      report "XRAYC_REAL_MULTI_USER_DURATION_SECONDS must be at least 2400 for the full real release gate"
    fi
    if value_ready "${XRAYC_REAL_MULTI_USER_ROUNDS:-}" && (( XRAYC_REAL_MULTI_USER_ROUNDS < 2 )); then
      report "XRAYC_REAL_MULTI_USER_ROUNDS must be at least 2 for the full real release gate"
    fi
  fi

  if [[ "${RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]]; then
    require_any_value "SUB_TOKEN or SUBSCRIPTION_URL" SUB_TOKEN SUBSCRIPTION_URL
    if ! command -v mihomo >/dev/null 2>&1 \
      && ! command -v clash >/dev/null 2>&1 \
      && ! command -v docker >/dev/null 2>&1; then
      report "mihomo/clash checker or Docker is required for RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT"
    fi
    if [[ "${RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC:-1}" =~ ^(1|true|TRUE|yes|YES)$ ]] \
      && ! command -v docker >/dev/null 2>&1; then
      report "Docker is required for RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC"
    fi
  fi

  if [[ "${REAL_RUNTIME_OBSERVATION_TIMEOUT_SECONDS:-180}" =~ ^[0-9]+$ ]] && [[ "${REAL_RUNTIME_OBSERVATION_POLL_INTERVAL_SECONDS:-5}" =~ ^[0-9]+$ ]]; then
    :
  else
    report "REAL_RUNTIME_OBSERVATION_* values must be integers"
  fi

  for name in BACKLOG_TARGET_INDEX BACKLOG_WAIT_SECONDS BACKLOG_TRAFFIC_ROUNDS; do
    value="${!name:-}"
    if [[ -n "$value" && ! "$value" =~ ^[1-9][0-9]*$ ]]; then
      report "${name} must be a positive integer"
    fi
  done
  if [[ -n "${BACKLOG_INSTALL_DIR:-}" ]] && value_is_placeholder "${BACKLOG_INSTALL_DIR:-}"; then
    report "BACKLOG_INSTALL_DIR still contains a placeholder"
  fi
}
