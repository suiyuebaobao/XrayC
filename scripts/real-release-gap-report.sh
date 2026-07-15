#!/usr/bin/env bash
# 用途：生成真实发布环境缺口报告，展示变量是否缺失、占位或就绪。
# 范围：只读取本地 env 和脚本内变量清单，不执行远端或部署动作。
# 输入：默认读取 .env.real-release，可用 XRAYC_REAL_RELEASE_ENV_FILE 指定。
# 输出：按控制面、远端、协议和测试分组输出脱敏状态摘要。
# 依赖：使用 Bash 间接变量展开和占位符识别函数完成分类。
# 安全：只输出状态，不输出任何环境变量真实值、token、密码或 URL。
# 约束：占位符规则应覆盖模板中的 copy-from、example、change-me 等模式。
# 行为：存在 env 文件时自动加载，缺失时仍生成缺口状态。
# 失败：报告本身不负责修复环境，异常只限脚本错误或不可读路径。
# 维护：新增真实发布变量时需同步分组清单和占位符识别规则。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
ENV_FILE_STATUS="default-missing"
if [[ -n "${XRAYC_REAL_RELEASE_ENV_FILE:-}" ]]; then
  ENV_FILE_STATUS="custom-missing"
fi
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  if [[ -n "${XRAYC_REAL_RELEASE_ENV_FILE:-}" ]]; then
    ENV_FILE_STATUS="custom-present"
  else
    ENV_FILE_STATUS="default-present"
  fi
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-target-name* ]] && return 0
  [[ "$value" == *private-inventory-path* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *upstream-host* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *your-secret* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *example* ]] && return 0
  return 1
}

var_state() {
  local name="$1"
  local value="${!name:-}"
  if [[ -z "$value" ]]; then
    printf 'missing'
  elif value_is_placeholder "$value"; then
    printf 'placeholder'
  else
    printf 'ready'
  fi
}

base_url_state() {
  local state=""
  state="$(var_state BASE_URL)"
  if [[ "$state" != "ready" ]]; then
    printf '%s' "$state"
    return
  fi
  if [[ "${BASE_URL:-}" == https://* ]]; then
    printf 'ready'
    return
  fi
  if [[ "${BASE_URL:-}" == http://* && "${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-0}" == "1" ]]; then
    printf 'ready-private-http'
    return
  fi
  printf 'invalid-http'
}

print_control_group() {
  local names=(XRAYC_ENV SEED_DEMO_DATA JWT_SECRET JWT_EXPIRES_IN JWT_REFRESH_EXPIRES_IN REAL_RELEASE_MATRIX_MODE BASE_URL DEPLOY_ARTIFACT_TOKEN DATABASE_URL)
  local name=""
  local state=""
  local missing=0

  for name in "${names[@]}"; do
    if [[ "$name" == "BASE_URL" ]]; then
      state="$(base_url_state)"
    elif [[ "$name" == "XRAYC_ENV" ]]; then
      if [[ "${XRAYC_ENV:-}" == "production" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    elif [[ "$name" == "SEED_DEMO_DATA" ]]; then
      if [[ "${SEED_DEMO_DATA:-}" == "false" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    elif [[ "$name" == "REAL_RELEASE_MATRIX_MODE" ]]; then
      if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "relay" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    else
      state="$(var_state "$name")"
    fi
    if [[ "$state" != "ready" && "$state" != "ready-private-http" ]]; then
      missing=$((missing + 1))
    fi
  done

  if [[ "$missing" -eq 0 ]]; then
    printf 'real-release-gap: [控制面与部署] ready\n'
    return
  fi

  printf 'real-release-gap: [控制面与部署] needs %s item(s)\n' "$missing"
  for name in "${names[@]}"; do
    if [[ "$name" == "BASE_URL" ]]; then
      state="$(base_url_state)"
    elif [[ "$name" == "XRAYC_ENV" ]]; then
      if [[ "${XRAYC_ENV:-}" == "production" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    elif [[ "$name" == "SEED_DEMO_DATA" ]]; then
      if [[ "${SEED_DEMO_DATA:-}" == "false" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    elif [[ "$name" == "REAL_RELEASE_MATRIX_MODE" ]]; then
      if [[ "${REAL_RELEASE_MATRIX_MODE:-relay}" == "relay" ]]; then
        state="ready"
      else
        state="$(var_state "$name")"
        if [[ "$state" == "ready" ]]; then
          state="invalid"
        fi
      fi
    else
      state="$(var_state "$name")"
    fi
    if [[ "$state" != "ready" && "$state" != "ready-private-http" ]]; then
      printf '  - %s: %s\n' "$name" "$state"
    fi
  done
}

print_group() {
  local group_name="$1"
  shift
  local name=""
  local state=""
  local missing=0

  for name in "$@"; do
    state="$(var_state "$name")"
    if [[ "$state" != "ready" ]]; then
      missing=$((missing + 1))
    fi
  done

  if [[ "$missing" -eq 0 ]]; then
    printf 'real-release-gap: [%s] ready\n' "$group_name"
    return
  fi

  printf 'real-release-gap: [%s] needs %s item(s)\n' "$group_name" "$missing"
  for name in "$@"; do
    state="$(var_state "$name")"
    if [[ "$state" != "ready" ]]; then
      printf '  - %s: %s\n' "$name" "$state"
    fi
  done
}

print_any_group() {
  local group_name="$1"
  shift
  local name=""
  local label=""
  local ready=0
  local state=""

  for name in "$@"; do
    if [[ -z "$label" ]]; then
      label="$name"
    else
      label="${label} or ${name}"
    fi
    state="$(var_state "$name")"
    if [[ "$state" == "ready" ]]; then
      ready=1
    fi
  done

  if [[ "$ready" -eq 1 ]]; then
    printf 'real-release-gap: [%s] ready\n' "$group_name"
    return
  fi

  printf 'real-release-gap: [%s] needs one of %s\n' "$group_name" "$label"
  for name in "$@"; do
    printf '  - %s: %s\n' "$name" "$(var_state "$name")"
  done
}

print_optional_default_group() {
  local group_name="$1"
  shift
  local name=""
  local state=""
  local value=""
  local invalid=0

  for name in "$@"; do
    value="${!name:-}"
    if [[ -n "$value" ]] && ! [[ "$value" =~ ^[0-9]+$ ]]; then
      invalid=$((invalid + 1))
    fi
  done

  if [[ "$invalid" -eq 0 ]]; then
    printf 'real-release-gap: [%s] optional/defaulted\n' "$group_name"
    return
  fi

  printf 'real-release-gap: [%s] has %s invalid optional item(s)\n' "$group_name" "$invalid"
  for name in "$@"; do
    value="${!name:-}"
    if [[ -n "$value" ]] && ! [[ "$value" =~ ^[0-9]+$ ]]; then
      state="$(var_state "$name")"
      printf '  - %s: %s\n' "$name" "$state"
    fi
  done
}

print_optional_value_group() {
  local group_name="$1"
  shift
  local name=""
  local state=""
  local missing=0

  for name in "$@"; do
    state="$(var_state "$name")"
    if [[ "$state" == "placeholder" ]]; then
      missing=$((missing + 1))
    fi
  done

  if [[ "$missing" -eq 0 ]]; then
    printf 'real-release-gap: [%s] optional/defaulted\n' "$group_name"
    return
  fi

  printf 'real-release-gap: [%s] has %s invalid optional item(s)\n' "$group_name" "$missing"
  for name in "$@"; do
    state="$(var_state "$name")"
    if [[ "$state" == "placeholder" ]]; then
      printf '  - %s: %s\n' "$name" "$state"
    fi
  done
}

bool_is_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

print_conditional_group() {
  local group_name="$1"
  local flag_name="$2"
  shift 2

  if ! bool_is_true "${!flag_name:-0}"; then
    printf 'real-release-gap: [%s] optional/disabled\n' "$group_name"
    return
  fi

  print_group "$group_name" "$@"
}

print_default_on_group() {
  local group_name="$1"
  local flag_name="$2"
  shift 2

  case "${!flag_name:-1}" in
    0|false|FALSE|no|NO|off|OFF)
      printf 'real-release-gap: [%s] optional/disabled\n' "$group_name"
      return
      ;;
  esac

  print_group "$group_name" "$@"
}

matrix_mode() {
  case "${REAL_RELEASE_MATRIX_MODE:-relay}" in
    direct) printf 'direct-diagnostic' ;;
    *) printf 'relay' ;;
  esac
}

print_protocol_group() {
  local group_name="$1"
  local client_proxy_name="$2"
  shift 2

  if [[ "$(matrix_mode)" == "direct-diagnostic" ]]; then
    print_group "$group_name" "$@" "$client_proxy_name"
    return
  fi

  print_group "$group_name" "$@"
}

echo "real-release-gap: env_file=${ENV_FILE_STATUS}"
# 真实发布门禁变量显式清单，供同步检查脚本确认模板、文档和
# readiness 报告没有漂移：
# ADMIN_ACCESS_TOKEN RUN_REAL_STABILITY_UAT RUN_REAL_STABILITY_UAT_24H UAT_AGENT_API_MODE
# UAT_PROFILE UAT_DURATION_SECONDS UAT_ACCESS_NODE_IDS UAT_ACCESS_LINE_IDS UAT_EXIT_ENDPOINT_IDS
# UAT_CLIENT_PROXY_URLS UAT_EXPECTED_EXIT_IPS UAT_USER_ACCESS_TOKENS
# RUN_ACCESS_TRAFFIC_BACKLOG_UAT BACKLOG_VALIDATE_ONLY BACKLOG_TARGET_INDEX
# BACKLOG_INSTALL_DIR BACKLOG_WAIT_SECONDS BACKLOG_TRAFFIC_ROUNDS BACKLOG_SKIP_AGENT_RESTART
# RUN_REAL_ACCESS_INBOUND_MATRIX_E2E RUN_REAL_RUNTIME_RESTORE_DEPLOY
# REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS
# RUN_RUNTIME_LOADTEST RUNTIME_LOADTEST_DATABASE_URL RUNTIME_LOADTEST_PROFILE
# RUNTIME_HTTP_LOADTEST_PROFILE RUNTIME_HTTP_LOADTEST_REQUIRE_FULL
# RUN_AUTH_HA_UAT RUN_AUTH_HA_STABILITY_UAT AUTH_HA_STABILITY_PROFILE AUTH_HA_STABILITY_DURATION_SECONDS
# RUN_OPS_MISTAKE_RECOVERY_UAT RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT
# OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL
# RUN_REAL_MULTI_USER_TRAFFIC_UAT XRAYC_REAL_MULTI_USER_COUNT
# XRAYC_REAL_MULTI_USER_DURATION_SECONDS XRAYC_REAL_MULTI_USER_ROUNDS
# XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES
# RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC XRAYC_MIHOMO_IMAGE
print_control_group
print_any_group "订阅凭据" SUB_TOKEN SUBSCRIPTION_URL
if [[ "$(matrix_mode)" == "direct-diagnostic" ]]; then
  print_group "接入节点与运行观测" \
    AGENT_TOKEN ACCESS_NODE_ID ACCESS_LINE_ID EXIT_ENDPOINT_ID \
    XRAYC_REAL_E2E_INVENTORY XRAYC_REAL_E2E_TARGET \
    CLIENT_PROXY_URL EXPECTED_EXIT_IP USER_ACCESS_TOKEN EXPECTED_ACCESS_SERVERS
else
  print_group "接入节点与运行观测" \
    XRAYC_REAL_E2E_INVENTORY XRAYC_REAL_E2E_TARGET \
    USER_ACCESS_TOKEN ADMIN_LOGIN_ACCOUNT ADMIN_LOGIN_PASSWORD
fi
print_group "真实 Playwright no-mock 账号" \
  E2E_USER_ACCOUNT E2E_USER_PASSWORD E2E_ADMIN_ACCOUNT E2E_ADMIN_PASSWORD
print_protocol_group "SOCKS 出口" CLIENT_PROXY_URL_SOCKS \
  THIRD_PARTY_SOCKS_HOST THIRD_PARTY_SOCKS_PORT THIRD_PARTY_SOCKS_RAW_URL \
  EXPECTED_EXIT_IP_SOCKS EXIT_ENDPOINT_ID_SOCKS
print_protocol_group "HTTP 出口" CLIENT_PROXY_URL_HTTP \
  THIRD_PARTY_HTTP_HOST THIRD_PARTY_HTTP_PORT THIRD_PARTY_HTTP_RAW_URL \
  THIRD_PARTY_HTTP_USERNAME THIRD_PARTY_HTTP_PASSWORD THIRD_PARTY_HTTP_KEY \
  EXPECTED_EXIT_IP_HTTP EXIT_ENDPOINT_ID_HTTP
print_any_group "VLESS 出口主机" THIRD_PARTY_HOST THIRD_PARTY_VLESS_HOST
print_protocol_group "VLESS 出口" CLIENT_PROXY_URL_VLESS \
  THIRD_PARTY_VLESS_PORT THIRD_PARTY_VLESS_RAW_URL THIRD_PARTY_UUID THIRD_PARTY_PUBLIC_KEY \
  EXPECTED_EXIT_IP_VLESS EXIT_ENDPOINT_ID_VLESS
print_protocol_group "Trojan 出口" CLIENT_PROXY_URL_TROJAN \
  THIRD_PARTY_TROJAN_HOST THIRD_PARTY_TROJAN_PORT THIRD_PARTY_TROJAN_RAW_URL THIRD_PARTY_TROJAN_KEY \
  EXPECTED_EXIT_IP_TROJAN EXIT_ENDPOINT_ID_TROJAN
print_protocol_group "Shadowsocks 出口" CLIENT_PROXY_URL_SHADOWSOCKS \
  THIRD_PARTY_SHADOWSOCKS_HOST THIRD_PARTY_SHADOWSOCKS_PORT THIRD_PARTY_SHADOWSOCKS_RAW_URL \
  THIRD_PARTY_SHADOWSOCKS_PASSWORD THIRD_PARTY_SHADOWSOCKS_KEY \
  EXPECTED_EXIT_IP_SHADOWSOCKS EXIT_ENDPOINT_ID_SHADOWSOCKS
print_protocol_group "Hysteria/HY2 出口" CLIENT_PROXY_URL_HY2 \
  THIRD_PARTY_HY2_HOST THIRD_PARTY_HY2_PORT THIRD_PARTY_HY2_RAW_URL \
  THIRD_PARTY_HY2_KEY \
  EXPECTED_EXIT_IP_HY2 EXIT_ENDPOINT_ID_HY2
print_optional_default_group "运行观测轮询" \
  REAL_RUNTIME_OBSERVATION_TIMEOUT_SECONDS REAL_RUNTIME_OBSERVATION_POLL_INTERVAL_SECONDS
print_default_on_group "用户入站协议矩阵 E2E" RUN_REAL_ACCESS_INBOUND_MATRIX_E2E \
  REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS
print_default_on_group "真实部署恢复" RUN_REAL_RUNTIME_RESTORE_DEPLOY \
  XRAYC_REAL_E2E_INVENTORY XRAYC_REAL_E2E_TARGET
print_optional_default_group "用户入站协议矩阵出口断言" \
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_VLESS \
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_TROJAN \
  REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_SHADOWSOCKS
print_default_on_group "10m 长稳 UAT" RUN_REAL_STABILITY_UAT \
  UAT_PROFILE UAT_DURATION_SECONDS UAT_ACCESS_NODE_IDS UAT_ACCESS_LINE_IDS UAT_EXIT_ENDPOINT_IDS \
  UAT_CLIENT_PROXY_URLS UAT_EXPECTED_EXIT_IPS UAT_USER_ACCESS_TOKENS UAT_AGENT_API_MODE
print_default_on_group "access-agent 流量积压恢复 UAT" RUN_ACCESS_TRAFFIC_BACKLOG_UAT \
  CLIENT_PROXY_URL ACCESS_NODE_ID ACCESS_LINE_ID XRAY_USER_KEY XRAYC_REAL_E2E_INVENTORY
print_optional_default_group "access-agent 流量积压恢复 UAT 参数" \
  BACKLOG_TARGET_INDEX BACKLOG_WAIT_SECONDS BACKLOG_TRAFFIC_ROUNDS
print_optional_value_group "access-agent 流量积压恢复 UAT 远端目录" \
  BACKLOG_INSTALL_DIR
print_conditional_group "24h 长稳 UAT" RUN_REAL_STABILITY_UAT_24H \
  UAT_ACCESS_NODE_IDS UAT_ACCESS_LINE_IDS UAT_EXIT_ENDPOINT_IDS \
  UAT_CLIENT_PROXY_URLS UAT_EXPECTED_EXIT_IPS UAT_USER_ACCESS_TOKENS
print_default_on_group "数据库运行压测" RUN_RUNTIME_LOADTEST \
  RUNTIME_LOADTEST_DATABASE_URL RUNTIME_LOADTEST_PROFILE
print_group "HTTP/API 运行压测" \
  RUNTIME_HTTP_LOADTEST_PROFILE RUNTIME_HTTP_LOADTEST_REQUIRE_FULL
print_default_on_group "多副本鉴权 UAT" RUN_AUTH_HA_UAT \
  E2E_ADMIN_ACCOUNT E2E_ADMIN_PASSWORD
print_default_on_group "多副本鉴权长稳 UAT" RUN_AUTH_HA_STABILITY_UAT \
  AUTH_HA_STABILITY_PROFILE AUTH_HA_STABILITY_DURATION_SECONDS E2E_ADMIN_ACCOUNT E2E_ADMIN_PASSWORD
print_default_on_group "误操作恢复 UAT" RUN_OPS_MISTAKE_RECOVERY_UAT \
  BASE_URL DATABASE_URL
print_default_on_group "大数据误操作恢复 UAT" RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT \
  OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL
print_default_on_group "多用户真实流量 UAT" RUN_REAL_MULTI_USER_TRAFFIC_UAT \
  XRAYC_REAL_MULTI_USER_COUNT XRAYC_REAL_MULTI_USER_DURATION_SECONDS \
  XRAYC_REAL_MULTI_USER_ROUNDS XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES \
  XRAYC_REAL_E2E_INVENTORY
print_default_on_group "订阅客户端兼容 Gate" RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC \
  SUB_TOKEN SUBSCRIPTION_URL XRAYC_MIHOMO_IMAGE

echo "real-release-gap: values are intentionally hidden; fill private env, then run make check-real-release-env"
