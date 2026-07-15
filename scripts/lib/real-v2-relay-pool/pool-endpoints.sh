create_pool_exit() {
  local host="$1"
  local port="$2"
  local label="$3"
  local _weight="$4"
  local username password
  if [[ "$label" == "a" ]]; then
    username="$EXIT_A_USERNAME"
    password="$EXIT_A_PASSWORD"
  else
    username="$EXIT_B_USERNAME"
    password="$EXIT_B_PASSWORD"
  fi
  local resource_payload="$TMP_DIR/exit-${label}-resource.json"
  local resource_body="$TMP_DIR/exit-${label}-resource-body.json"
  local endpoint_payload="$TMP_DIR/exit-${label}-endpoint.json"
  local endpoint_body="$TMP_DIR/exit-${label}-endpoint-body.json"
  local resource_id
  RUN_ID_VALUE="$RUN_ID" LABEL_VALUE="$label" \
    write_json "$resource_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"name": f"real-exit-{label}-{rid}", "region_code": "US", "provider_name": "real-e2e", "ownership": "third_party", "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-resources" "$resource_payload" "$resource_body" "$admin_token"
  resource_id="$(json_value "$resource_body" id)"
  RUN_ID_VALUE="$RUN_ID" HOST_VALUE="$host" PORT_VALUE="$port" LABEL_VALUE="$label" RESOURCE_ID_VALUE="$resource_id" \
    SOCKS_USERNAME="$username" SOCKS_PASSWORD="$password" \
    write_json "$endpoint_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"exit_resource_id": os.environ["RESOURCE_ID_VALUE"], "name": f"real-socks-{label}-{rid}", "outbound_type": "socks", "host": os.environ["HOST_VALUE"], "port": int(os.environ["PORT_VALUE"]), "outbound_config": {"username": os.environ["SOCKS_USERNAME"], "password": os.environ["SOCKS_PASSWORD"]}, "stream_config": {}, "probe_config": {}, "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-endpoints" "$endpoint_payload" "$endpoint_body" "$admin_token"
  json_value "$endpoint_body" id
}

create_pool_shadowsocks_exit() {
  local host="$1"
  local port="$2"
  local label="$3"
  local _weight="$4"
  local password
  if [[ "$label" == "a" ]]; then
    password="$EXIT_A_SS_PASSWORD"
  else
    password="$EXIT_B_SS_PASSWORD"
  fi
  local resource_payload="$TMP_DIR/exit-${label}-resource.json"
  local resource_body="$TMP_DIR/exit-${label}-resource-body.json"
  local endpoint_payload="$TMP_DIR/exit-${label}-endpoint.json"
  local endpoint_body="$TMP_DIR/exit-${label}-endpoint-body.json"
  local resource_id
  RUN_ID_VALUE="$RUN_ID" LABEL_VALUE="$label" \
    write_json "$resource_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"name": f"real-exit-{label}-{rid}", "region_code": "US", "provider_name": "real-e2e", "ownership": "third_party", "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-resources" "$resource_payload" "$resource_body" "$admin_token"
  resource_id="$(json_value "$resource_body" id)"
  RUN_ID_VALUE="$RUN_ID" HOST_VALUE="$host" PORT_VALUE="$port" LABEL_VALUE="$label" RESOURCE_ID_VALUE="$resource_id" \
    SHADOWSOCKS_METHOD="$EXIT_SS_METHOD" SHADOWSOCKS_PASSWORD="$password" \
    write_json "$endpoint_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"exit_resource_id": os.environ["RESOURCE_ID_VALUE"], "name": f"real-shadowsocks-{label}-{rid}", "outbound_type": "shadowsocks", "host": os.environ["HOST_VALUE"], "port": int(os.environ["PORT_VALUE"]), "outbound_config": {"method": os.environ["SHADOWSOCKS_METHOD"], "password": os.environ["SHADOWSOCKS_PASSWORD"]}, "stream_config": {}, "probe_config": {}, "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-endpoints" "$endpoint_payload" "$endpoint_body" "$admin_token"
  json_value "$endpoint_body" id
}

create_pool_vless_exit() {
  local host="$1"
  local port="$2"
  local label="$3"
  local _weight="$4"
  local uuid
  if [[ "$label" == "a" ]]; then
    uuid="$EXIT_A_VLESS_UUID"
  else
    uuid="$EXIT_B_VLESS_UUID"
  fi
  local resource_payload="$TMP_DIR/exit-${label}-resource.json"
  local resource_body="$TMP_DIR/exit-${label}-resource-body.json"
  local endpoint_payload="$TMP_DIR/exit-${label}-endpoint.json"
  local endpoint_body="$TMP_DIR/exit-${label}-endpoint-body.json"
  local resource_id
  RUN_ID_VALUE="$RUN_ID" LABEL_VALUE="$label" \
    write_json "$resource_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"name": f"real-exit-{label}-{rid}", "region_code": "US", "provider_name": "real-e2e", "ownership": "third_party", "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-resources" "$resource_payload" "$resource_body" "$admin_token"
  resource_id="$(json_value "$resource_body" id)"
  RUN_ID_VALUE="$RUN_ID" HOST_VALUE="$host" PORT_VALUE="$port" LABEL_VALUE="$label" RESOURCE_ID_VALUE="$resource_id" VLESS_UUID="$uuid" \
    write_json "$endpoint_payload" 'rid = os.environ["RUN_ID_VALUE"]; label = os.environ["LABEL_VALUE"]; print(json.dumps({"exit_resource_id": os.environ["RESOURCE_ID_VALUE"], "name": f"real-vless-{label}-{rid}", "outbound_type": "vless", "host": os.environ["HOST_VALUE"], "port": int(os.environ["PORT_VALUE"]), "outbound_config": {"uuid": os.environ["VLESS_UUID"], "security": "none"}, "stream_config": {"udp_packet_encoding": "xudp"}, "probe_config": {}, "enabled": True}, ensure_ascii=False))'
  api_json POST "/api/admin/exit-endpoints" "$endpoint_payload" "$endpoint_body" "$admin_token"
  json_value "$endpoint_body" id
}
