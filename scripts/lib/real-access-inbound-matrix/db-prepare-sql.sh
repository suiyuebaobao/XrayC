#!/usr/bin/env bash

db_prepare_run_rows_sql() {
  local tmp_output="$1"
  run_psql_to_file "$tmp_output" \
    -XAtq \
    -v ON_ERROR_STOP=1 \
    -v "token_hash=${token_hash_value}" \
    -v "access_line_id=${ACCESS_LINE_ID:-}" \
    -v "run_id=${run_id}" \
    -v "protocols=${normalized}" \
    -v "listen_host_override=${listen_host_override}" \
    -v "access_node_hint=${access_node_hint}" \
    -v "preferred_exit_endpoint_id=${preferred_exit_endpoint_id}" \
    -v "third_party_socks_host=${third_party_socks_host}" \
    -v "third_party_socks_port=${third_party_socks_port}" \
    -v "third_party_socks_username=${third_party_socks_username}" \
    -v "third_party_socks_password=${third_party_socks_password}" \
    -v "port_base=${port_base}" \
    -f "${SCRIPT_DIR}/lib/real-access-inbound-matrix/db-prepare.psql"
}