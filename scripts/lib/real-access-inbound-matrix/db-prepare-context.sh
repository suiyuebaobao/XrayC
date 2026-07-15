#!/usr/bin/env bash
# 用途：准备真实 inbound 矩阵 DB 写入所需的环境上下文。
# 由 db-prepare.sh 在 prepare_rows 执行前调用。
# 本文件只做本地变量归一、占位检查和端口校验。
# 它不连接数据库，不写文件，也不输出敏感信息。
# 依赖函数由外层脚本提供，保持既有错误文案。
# 输出通过 DB_PREPARE_* 变量传回调用方。
# 私有代理字段只做存在性和格式校验，不打印原值。
# 该拆分用于控制脚本长度，避免把 SQL 主体拆散。
# 修改真实矩阵 env 时优先更新本文件和对应门禁。

prepare_db_matrix_context() {
  local protocols="$1"

  require_command python3
  require_command psql
  require_real_env DATABASE_URL
  require_real_env SUB_TOKEN
  if [[ -n "${ACCESS_LINE_ID:-}" ]]; then
    require_uuid_env ACCESS_LINE_ID
  fi
  xrayc_real_e2e_require_url_scheme DATABASE_URL "real inbound protocol matrix DB prepare"

  DB_PREPARE_NORMALIZED="$(normalize_protocol_csv "$protocols")" \
    || die_usage "missing protocol list is invalid"
  DB_PREPARE_RUN_ID="$(new_run_id)"
  DB_PREPARE_TOKEN_HASH="$(token_hash)"
  DB_PREPARE_PORT_BASE="${REAL_ACCESS_INBOUND_MATRIX_TEMP_PORT_BASE:-34200}"
  DB_PREPARE_LISTEN_HOST_OVERRIDE="${REAL_ACCESS_INBOUND_MATRIX_LISTEN_HOST:-}"
  DB_PREPARE_ACCESS_NODE_HINT="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_NODE_HINT:-}"
  DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID="${REAL_ACCESS_INBOUND_MATRIX_EXIT_ENDPOINT_ID:-}"
  case "${REAL_ACCESS_INBOUND_MATRIX_ENABLE_THIRD_PARTY_SOCKS:-0}" in
    1|true|TRUE|yes|YES|on|ON)
      DB_PREPARE_THIRD_PARTY_SOCKS_HOST="${THIRD_PARTY_SOCKS_HOST:-}"
      DB_PREPARE_THIRD_PARTY_SOCKS_PORT="${THIRD_PARTY_SOCKS_PORT:-}"
      DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME="${THIRD_PARTY_SOCKS_USERNAME:-}"
      DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD="${THIRD_PARTY_SOCKS_PASSWORD:-}"
      ;;
    *)
      DB_PREPARE_THIRD_PARTY_SOCKS_HOST=""
      DB_PREPARE_THIRD_PARTY_SOCKS_PORT=""
      DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME=""
      DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD=""
      ;;
  esac

  normalize_db_matrix_preferred_exit_endpoint
  validate_db_matrix_port_base
  normalize_db_matrix_third_party_socks
  validate_db_matrix_third_party_socks
}

normalize_db_matrix_preferred_exit_endpoint() {
  if [[ -n "$DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID" ]]; then
    if value_is_placeholder "$DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID" \
      || [[ ! "$DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]; then
      DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID=""
    fi
  fi
}

validate_db_matrix_port_base() {
  [[ "$DB_PREPARE_PORT_BASE" =~ ^[0-9]+$ && "$DB_PREPARE_PORT_BASE" -ge 1024 && "$DB_PREPARE_PORT_BASE" -le 65000 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_TEMP_PORT_BASE must be a TCP port from 1024 to 65000"
}

normalize_db_matrix_third_party_socks() {
  if value_is_placeholder "$DB_PREPARE_THIRD_PARTY_SOCKS_HOST"; then
    DB_PREPARE_THIRD_PARTY_SOCKS_HOST=""
  fi
  if value_is_placeholder "$DB_PREPARE_THIRD_PARTY_SOCKS_PORT"; then
    DB_PREPARE_THIRD_PARTY_SOCKS_PORT=""
  fi
  if value_is_placeholder "$DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME"; then
    DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME=""
  fi
  if value_is_placeholder "$DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD"; then
    DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD=""
  fi
}

validate_db_matrix_third_party_socks() {
  if [[ -n "$DB_PREPARE_THIRD_PARTY_SOCKS_HOST" || -n "$DB_PREPARE_THIRD_PARTY_SOCKS_PORT" ]]; then
    [[ -n "$DB_PREPARE_THIRD_PARTY_SOCKS_HOST" ]] \
      || die_usage "THIRD_PARTY_SOCKS_HOST must be set with THIRD_PARTY_SOCKS_PORT"
    [[ "$DB_PREPARE_THIRD_PARTY_SOCKS_PORT" =~ ^[0-9]+$ \
      && "$DB_PREPARE_THIRD_PARTY_SOCKS_PORT" -ge 1 \
      && "$DB_PREPARE_THIRD_PARTY_SOCKS_PORT" -le 65535 ]] \
      || die_usage "THIRD_PARTY_SOCKS_PORT must be a TCP port from 1 to 65535"
    if [[ -n "$DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME" || -n "$DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD" ]]; then
      [[ -n "$DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME" && -n "$DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD" ]] \
        || die_usage "THIRD_PARTY_SOCKS_USERNAME and THIRD_PARTY_SOCKS_PASSWORD must be set together"
    fi
  fi
}
