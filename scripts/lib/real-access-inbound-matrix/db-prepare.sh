#!/usr/bin/env bash
# 用途：为真实 inbound 矩阵 DB 准备临时协议行。
# 由 scripts/real-access-inbound-matrix-db-prepare.sh source 使用，不直接执行。
# shellcheck source=scripts/lib/real-access-inbound-matrix/db-prepare-context.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/db-prepare-context.sh"
# shellcheck source=scripts/lib/real-access-inbound-matrix/db-prepare-sql.sh
source "${SCRIPT_DIR}/lib/real-access-inbound-matrix/db-prepare-sql.sh"
prepare_rows() {
  local protocols="$1"
  local manifest_file="$2"
  local normalized run_id token_hash_value port_base listen_host_override access_node_hint preferred_exit_endpoint_id tmp_output
  local third_party_socks_host third_party_socks_port third_party_socks_username third_party_socks_password
  prepare_db_matrix_context "$protocols"
  normalized="$DB_PREPARE_NORMALIZED"
  run_id="$DB_PREPARE_RUN_ID"
  token_hash_value="$DB_PREPARE_TOKEN_HASH"
  port_base="$DB_PREPARE_PORT_BASE"
  listen_host_override="$DB_PREPARE_LISTEN_HOST_OVERRIDE"
  access_node_hint="$DB_PREPARE_ACCESS_NODE_HINT"
  preferred_exit_endpoint_id="$DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID"
  third_party_socks_host="$DB_PREPARE_THIRD_PARTY_SOCKS_HOST"
  third_party_socks_port="$DB_PREPARE_THIRD_PARTY_SOCKS_PORT"
  third_party_socks_username="$DB_PREPARE_THIRD_PARTY_SOCKS_USERNAME"
  third_party_socks_password="$DB_PREPARE_THIRD_PARTY_SOCKS_PASSWORD"
  tmp_output="${manifest_file}.tmp"
  db_prepare_run_rows_sql "$tmp_output"

  chmod 600 "$tmp_output"
  mv "$tmp_output" "$manifest_file"
}
