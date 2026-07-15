#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
set +x
# 真实 V2 主链路验收脚本。
# 使用最新私有测试服务器清单动态分配客户端、中转和出口宿主角色；
# 第二出口优先使用下一台非 Cloudflare 可用服务器；当前私有角色中
# server_4 是 Cloudflare 入口验证目标，因此遇到该角色时复用出口宿主的不同端口。
# 远端 SOCKS 仅作为出口宿主 readiness sidecar，真实流量走 self_hosted VLESS/TCP。

umask 077

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/real-e2e-lib.sh"

INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-文档/私有/remote-e2e-inventory.generated.json}"
DEPLOY_TOKEN_FILE="${XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE:-文档/私有/deploy-artifact-token.env}"
DB_URL="${DATABASE_URL:-postgres://xrayc:change-me@127.0.0.1:15432/xrayc}"
ADMIN_ACCOUNT="${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-admin@example.test}"
ADMIN_PASSWORD="${XRAYC_REAL_E2E_ADMIN_PASSWORD:-admin123456}"
USER_ACCOUNT="${XRAYC_REAL_E2E_USER_ACCOUNT:-demo@example.test}"
USER_PASSWORD="${XRAYC_REAL_E2E_USER_PASSWORD:-demo123456}"
LISTEN_PORT="${XRAYC_REAL_E2E_LISTEN_PORT:-24443}"
EXIT_A_PORT="${XRAYC_REAL_E2E_EXIT_A_PORT:-31881}"
EXIT_B_PORT="${XRAYC_REAL_E2E_EXIT_B_PORT:-31882}"
EXIT_A_SS_PORT="${XRAYC_REAL_E2E_EXIT_A_SS_PORT:-31883}"
EXIT_B_SS_PORT="${XRAYC_REAL_E2E_EXIT_B_SS_PORT:-31884}"
EXIT_SS_METHOD="${XRAYC_REAL_E2E_EXIT_SS_METHOD:-aes-128-gcm}"
EXIT_A_VLESS_PORT="${XRAYC_REAL_E2E_EXIT_A_VLESS_PORT:-31885}"
EXIT_B_VLESS_PORT="${XRAYC_REAL_E2E_EXIT_B_VLESS_PORT:-31886}"
ACCESS_SS_METHOD="${XRAYC_REAL_E2E_ACCESS_SS_METHOD:-2022-blake3-aes-128-gcm}"
ACCESS_SS_PASSWORD="${XRAYC_REAL_E2E_ACCESS_SS_PASSWORD:-MDEyMzQ1Njc4OWFiY2RlZg==}"
CLIENT_SOCKS_PORT="${XRAYC_REAL_E2E_CLIENT_SOCKS_PORT:-31080}"
CLIENT_UDP_PORT="${XRAYC_REAL_E2E_CLIENT_UDP_PORT:-31081}"
UDP_TARGET_PORT="${XRAYC_REAL_E2E_UDP_TARGET_PORT:-31991}"
TCP_TARGET_PORT="${XRAYC_REAL_E2E_TCP_TARGET_PORT:-31992}"
RUN_ID="$(date +%Y%m%d%H%M%S)-$$"
EXIT_A_USERNAME="xrayc_a_${RUN_ID//[^A-Za-z0-9]/_}"
EXIT_B_USERNAME="xrayc_b_${RUN_ID//[^A-Za-z0-9]/_}"
EXIT_A_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
EXIT_B_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
EXIT_A_SS_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
EXIT_B_SS_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
EXIT_A_VLESS_UUID="$(python3 -c 'import uuid; print(uuid.uuid4())')"
EXIT_B_VLESS_UUID="$(python3 -c 'import uuid; print(uuid.uuid4())')"
TMP_DIR="$(mktemp -d)"
access_node_id=""
access_line_id=""
access_line_id_a=""
access_line_id_b=""
pool_id=""
pool_id_a=""
pool_id_b=""
group_id=""
exit_endpoint_a=""
exit_endpoint_b=""
user_id=""
user_xray_key=""
user_plan_id=""
original_user_disabled=""
original_sub_active=""
original_sub_expires_at=""
original_sub_used_bytes=""
original_sub_limit_bytes=""
original_user_snapshot_saved=0
original_subscription_snapshot_saved=0
completed_ok=0
keep_success_artifacts="${XRAYC_REAL_E2E_KEEP_SUCCESS:-0}"
runtime_env_out="${XRAYC_REAL_E2E_RUNTIME_ENV_OUT:-}"
local_tunnel_port="${XRAYC_REAL_E2E_LOCAL_TUNNEL_PORT:-33180}"
ssh_tunnel_pid=""
remote_cleanup_enabled=0

# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/functions.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/rebind.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/client.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/subscription.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/long-data.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/runtime-env.sh"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-v2-relay-pool/targets.sh"
trap cleanup EXIT

require_file "$INVENTORY"
require_file "$DEPLOY_TOKEN_FILE"

# shellcheck disable=SC1090
. "$DEPLOY_TOKEN_FILE"
DEPLOY_TOKEN="${DEPLOY_ARTIFACT_TOKEN:-}"
[[ -n "$DEPLOY_TOKEN" ]] || die "deploy artifact token is missing"

write_relay_pool_targets_env

# shellcheck disable=SC1090
. "$TMP_DIR/targets.env"
BASE_URL="${BASE_URL%/}"
[[ -n "$BASE_URL" ]] || die "control plane URL is missing"
remote_cleanup_enabled=1

admin_token="$(login_token "$ADMIN_ACCOUNT" "$ADMIN_PASSWORD" admin)"
user_token="$(login_token "$USER_ACCOUNT" "$USER_PASSWORD" user)"
user_id="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_email="$USER_ACCOUNT" <<'SQL'
SELECT id FROM users WHERE email = :'user_email' LIMIT 1;
SQL
)"
[[ -n "$user_id" ]] || die "test user is missing"
user_xray_key="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT xray_user_key FROM users WHERE id = :'user_id'::uuid LIMIT 1;
SQL
)"
[[ -n "$user_xray_key" ]] || die "test user runtime key is missing"
user_plan_id="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT plan_id::text FROM user_subscriptions WHERE user_id = :'user_id'::uuid LIMIT 1;
SQL
)"
[[ -n "$user_plan_id" ]] || die "test subscription plan is missing"
original_user_disabled="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT disabled::text FROM users WHERE id = :'user_id'::uuid LIMIT 1;
SQL
)"
[[ -n "$original_user_disabled" ]] || die "test user snapshot failed"
original_user_snapshot_saved=1
IFS=$'\t' read -r original_sub_active original_sub_expires_at original_sub_used_bytes original_sub_limit_bytes < <(
  psql_db -XAtq -F $'\t' -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT active::text,
       expires_at::text,
       used_bytes::text,
       limit_bytes::text
FROM user_subscriptions
WHERE user_id = :'user_id'::uuid
LIMIT 1;
SQL
)
[[ -n "$original_sub_active" && -n "$original_sub_expires_at" ]] || die "test subscription snapshot failed"
original_subscription_snapshot_saved=1

psql_db -Xq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
UPDATE users SET disabled = FALSE WHERE id = :'user_id'::uuid;
UPDATE user_subscriptions
SET active = TRUE,
    expires_at = now() + interval '30 days',
    used_bytes = 0,
    limit_bytes = GREATEST(limit_bytes, 10737418240)
WHERE user_id = :'user_id'::uuid;
SQL

subscription_body="$TMP_DIR/subscription.json"
api_auth_get /api/user/subscription "$subscription_body" "$user_token"
sub_token="$(json_value "$subscription_body" token)"
echo "real_v2_relay_pool_e2e: auth_and_subscription_ok"

setup_exit exit_a "$EXIT_A_REMOTE_INDEX" xrayc-real-exit-a "$EXIT_A_PORT" "$EXIT_A_USERNAME" "$EXIT_A_PASSWORD"
setup_exit exit_b "$EXIT_B_REMOTE_INDEX" xrayc-real-exit-b "$EXIT_B_PORT" "$EXIT_B_USERNAME" "$EXIT_B_PASSWORD"
setup_udp_target
setup_tcp_rate_target
wait_udp_target_reachable
wait_tcp_rate_target_reachable
exit_a_ip="$(remote_exec "$EXIT_A_REMOTE_INDEX" "curl -fsS --max-time 10 https://api.ipify.org" | tr -d '[:space:]')"
exit_b_ip="$(remote_exec "$EXIT_B_REMOTE_INDEX" "curl -fsS --max-time 10 https://api.ipify.org" | tr -d '[:space:]')"
for _ in $(seq 1 20); do
  if remote_exec 2 "timeout 5 bash -lc '</dev/tcp/${EXIT_A_PUBLIC_HOST}/${EXIT_A_PORT}'" >/dev/null 2>&1 \
    && remote_exec 2 "timeout 5 bash -lc '</dev/tcp/${EXIT_B_PUBLIC_HOST}/${EXIT_B_PORT}'" >/dev/null 2>&1; then
    echo "real_v2_relay_pool_e2e: transit_to_exits_ok=2"
    break
  fi
  sleep 1
done
remote_exec 2 "timeout 5 bash -lc '</dev/tcp/${EXIT_A_PUBLIC_HOST}/${EXIT_A_PORT}'" >/dev/null 2>&1 \
  || die "transit cannot reach first exit port"
remote_exec 2 "timeout 5 bash -lc '</dev/tcp/${EXIT_B_PUBLIC_HOST}/${EXIT_B_PORT}'" >/dev/null 2>&1 \
  || die "transit cannot reach second exit port"
for _ in $(seq 1 20); do
  if remote_exec 2 "curl --fail --silent --show-error --max-time 12 --socks5-hostname '${EXIT_A_USERNAME}:${EXIT_A_PASSWORD}@${EXIT_A_PUBLIC_HOST}:${EXIT_A_PORT}' https://api.ipify.org >/dev/null" >/dev/null 2>&1 \
    && remote_exec 2 "curl --fail --silent --show-error --max-time 12 --socks5-hostname '${EXIT_B_USERNAME}:${EXIT_B_PASSWORD}@${EXIT_B_PUBLIC_HOST}:${EXIT_B_PORT}' https://api.ipify.org >/dev/null" >/dev/null 2>&1; then
    echo "real_v2_relay_pool_e2e: transit_to_exit_aux_socks_ready=2"
    break
  fi
  sleep 1
done
remote_exec 2 "curl --fail --silent --show-error --max-time 12 --socks5-hostname '${EXIT_A_USERNAME}:${EXIT_A_PASSWORD}@${EXIT_A_PUBLIC_HOST}:${EXIT_A_PORT}' https://api.ipify.org >/dev/null" >/dev/null 2>&1 \
  || die "transit cannot use first auxiliary exit SOCKS readiness sidecar"
remote_exec 2 "curl --fail --silent --show-error --max-time 12 --socks5-hostname '${EXIT_B_USERNAME}:${EXIT_B_PASSWORD}@${EXIT_B_PUBLIC_HOST}:${EXIT_B_PORT}' https://api.ipify.org >/dev/null" >/dev/null 2>&1 \
  || die "transit cannot use second auxiliary exit SOCKS readiness sidecar"
echo "real_v2_relay_pool_e2e: exits_ready=2"

agent_token="$(python3 - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
)"
node_payload="$TMP_DIR/access-node.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_PUBLIC_HOST="$PH2" LISTEN_PORT_VALUE="$LISTEN_PORT" \
  AGENT_TOKEN_VALUE="$agent_token" \
  write_json "$node_payload" 'rid = os.environ["RUN_ID_VALUE"]; print(json.dumps({"name": f"real-relay-{rid}", "public_host": os.environ["TRANSIT_PUBLIC_HOST"], "public_port": int(os.environ["LISTEN_PORT_VALUE"]), "agent_token": os.environ["AGENT_TOKEN_VALUE"], "remark": "real relay pool e2e"}, ensure_ascii=False))'
node_body="$TMP_DIR/access-node-body.json"
api_json POST /api/admin/access-nodes "$node_payload" "$node_body" "$admin_token"
access_node_id="$(json_value "$node_body" id)"
[[ -n "$access_node_id" ]] || die "access node creation failed"
guide_payload="$TMP_DIR/agent-install-guide.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_PUBLIC_HOST="$PH2" BASE_URL_VALUE="$BASE_URL" \
  ACCESS_NODE_ID_VALUE="$access_node_id" \
  write_json "$guide_payload" 'print(json.dumps({"access_node_id": os.environ["ACCESS_NODE_ID_VALUE"], "install_dir": "/opt/xrayc-real-relay", "compose_project_name": "xrayc-real-relay", "panel_url": os.environ["BASE_URL_VALUE"], "xray_api_server": "127.0.0.1:10085", "xray_api_port": 10085, "heartbeat_interval_seconds": 5, "traffic_interval_seconds": 5, "session_idle_seconds": 30, "expected_listen_ports": [], "disable_legacy_systemd_units": True, "force_reinstall": True}, ensure_ascii=False))'
guide_body="$TMP_DIR/agent-install-guide-body.json"
api_json POST /api/admin/access-nodes/install-guide "$guide_payload" "$guide_body" "$admin_token"
guide_access_node_id="$(json_value "$guide_body" access_node_id)"
[[ "$guide_access_node_id" == "$access_node_id" ]] || die "agent install guide returned unexpected access node"
install_inventory="$TMP_DIR/agent-install-inventory.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_SSH_HOST="$H2" TRANSIT_PUBLIC_HOST="$PH2" TRANSIT_USER="$U2" TRANSIT_PORT="$P2" \
  TRANSIT_PASSWORD_FILE="$PF2" BASE_URL_VALUE="$BASE_URL" ACCESS_NODE_ID_VALUE="$access_node_id" \
  AGENT_TOKEN_VALUE="$agent_token" DEPLOY_TOKEN_VALUE="$DEPLOY_TOKEN" \
  write_json "$install_inventory" 'rid = os.environ["RUN_ID_VALUE"]; print(json.dumps({"targets": [{"alias": f"real-relay-{rid}", "ssh_host": os.environ["TRANSIT_SSH_HOST"], "ssh_user": os.environ["TRANSIT_USER"], "ssh_port": int(os.environ["TRANSIT_PORT"]), "ssh_password_file": os.environ["TRANSIT_PASSWORD_FILE"], "control_plane_url": os.environ["BASE_URL_VALUE"], "deploy_artifact_token": os.environ["DEPLOY_TOKEN_VALUE"], "node_id": os.environ["ACCESS_NODE_ID_VALUE"], "node_token": os.environ["AGENT_TOKEN_VALUE"], "install_dir": "/opt/xrayc-real-relay", "compose_project": "xrayc-real-relay", "xray_api_server": "127.0.0.1:10085", "xray_api_listen_host": "127.0.0.1", "xray_api_listen_port": 10085, "heartbeat_interval_seconds": 5, "traffic_interval_seconds": 5, "session_idle_seconds": 30, "expected_listen_ports": []}]}, ensure_ascii=False))'
remote_allow_insecure=0
case "$BASE_URL" in
  http://*) remote_allow_insecure=1 ;;
esac
XRAYC_REAL_E2E_INVENTORY="$install_inventory" \
XRAYC_REAL_E2E_TARGET="real-relay-${RUN_ID}" \
XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1 \
XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP="$remote_allow_insecure" \
XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE=1 \
XRAYC_DEPLOY_REUSE_EXISTING_IMAGES="${XRAYC_REAL_E2E_DEPLOY_REUSE_EXISTING_IMAGES:-0}" \
XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS=false \
  bash scripts/real-remote-access-deploy-e2e.sh
echo "real_v2_relay_pool_e2e: agent_install=manual_script_succeeded"

local_exit_payload="$TMP_DIR/local-exit-lines.json"
local_exit_body="$TMP_DIR/local-exit-lines-body.json"
RUN_ID_VALUE="$RUN_ID" \
  EXIT_A_HOST="$EXIT_A_PUBLIC_HOST" EXIT_A_PORT_VALUE="$EXIT_A_VLESS_PORT" EXIT_A_UUID="$EXIT_A_VLESS_UUID" \
  EXIT_B_HOST="$EXIT_B_PUBLIC_HOST" EXIT_B_PORT_VALUE="$EXIT_B_VLESS_PORT" EXIT_B_UUID="$EXIT_B_VLESS_UUID" \
  write_json "$local_exit_payload" 'rid = os.environ["RUN_ID_VALUE"]; print(json.dumps({"lines": [{"resource_name": f"real-local-exit-a-{rid}", "endpoint_name": f"real-local-vless-a-{rid}", "region_code": "US", "outbound_type": "vless", "network_mode": "xudp", "host": os.environ["EXIT_A_HOST"], "port": int(os.environ["EXIT_A_PORT_VALUE"]), "outbound_config": {"uuid": os.environ["EXIT_A_UUID"], "security": "none"}, "stream_config": {"udp_packet_encoding": "xudp"}, "probe_config": {}, "enabled": True}, {"resource_name": f"real-local-exit-b-{rid}", "endpoint_name": f"real-local-vless-b-{rid}", "region_code": "US", "outbound_type": "vless", "network_mode": "xudp", "host": os.environ["EXIT_B_HOST"], "port": int(os.environ["EXIT_B_PORT_VALUE"]), "outbound_config": {"uuid": os.environ["EXIT_B_UUID"], "security": "none"}, "stream_config": {"udp_packet_encoding": "xudp"}, "probe_config": {}, "enabled": True}]}, ensure_ascii=False))'
api_json POST "/api/admin/access-nodes/${access_node_id}/local-exit-lines" "$local_exit_payload" "$local_exit_body" "$admin_token"
IFS=$'\t' read -r exit_endpoint_a exit_endpoint_b < <(python3 - "$local_exit_body" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
lines = data.get("created_lines", [])
if len(lines) != 2:
    raise SystemExit(1)
print(f"{lines[0].get('exit_endpoint_id', '')}\t{lines[1].get('exit_endpoint_id', '')}")
PY
)
[[ -n "$exit_endpoint_a" && -n "$exit_endpoint_b" ]] || die "local exit line creation failed"
local_exit_count="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
  -v access_node_id="$access_node_id" \
  -v exit_endpoint_a="$exit_endpoint_a" \
  -v exit_endpoint_b="$exit_endpoint_b" <<'SQL'
SELECT COUNT(*)::text
FROM exit_endpoints ee
JOIN exit_resources er ON er.id = ee.exit_resource_id
WHERE ee.id IN (:'exit_endpoint_a'::uuid, :'exit_endpoint_b'::uuid)
  AND er.ownership = 'self_hosted'
  AND er.access_node_id = :'access_node_id'::uuid
  AND ee.outbound_type = 'vless'
  AND ee.enabled = TRUE;
SQL
)"
[[ "$local_exit_count" == "2" ]] || die "local exit endpoints were not created as self-hosted lines"
echo "real_v2_relay_pool_e2e: local_exit_lines_created count=2 ownership=self_hosted"

line_name="real-relay-line-${RUN_ID}"
subscription_proxy_name="$line_name"
group_payload="$TMP_DIR/line-group.json"
group_body="$TMP_DIR/line-group-body.json"
RUN_ID_VALUE="$RUN_ID" \
  write_json "$group_payload" 'rid = os.environ["RUN_ID_VALUE"]; print(json.dumps({"name": f"real-e2e-default-{rid}", "country_code": "US", "sort_weight": -100000, "enabled": True}, ensure_ascii=False))'
api_json POST "/api/admin/line-groups" "$group_payload" "$group_body" "$admin_token"
group_id="$(json_value "$group_body" id)"
[[ -n "$group_id" ]] || die "line group creation failed"

entry_payload="$TMP_DIR/access-entry.json"
LINE_NAME_VALUE="$line_name" EXIT_ENDPOINT_ID_VALUE="$exit_endpoint_a" ACCESS_NODE_ID_VALUE="$access_node_id" \
  TRANSIT_PUBLIC_HOST="$PH2" LISTEN_PORT_VALUE="$LISTEN_PORT" \
  write_json "$entry_payload" 'print(json.dumps({"access_node_id": os.environ["ACCESS_NODE_ID_VALUE"], "name": os.environ["LINE_NAME_VALUE"], "listen_host": os.environ["TRANSIT_PUBLIC_HOST"], "listen_port": int(os.environ["LISTEN_PORT_VALUE"]), "protocol": "vless", "transport": "tcp", "security": "", "enabled": True, "sort_weight": 100}, ensure_ascii=False))'
entry_body="$TMP_DIR/access-entry-body.json"
api_json POST "/api/admin/access-entries" "$entry_payload" "$entry_body" "$admin_token"
access_entry_id="$(json_value "$entry_body" id)"
[[ -n "$access_entry_id" ]] || die "access entry creation failed"
binding_payload="$TMP_DIR/access-entry-binding.json"
LINE_NAME_VALUE="$line_name" EXIT_ENDPOINT_ID_VALUE="$exit_endpoint_a" \
  write_json "$binding_payload" 'print(json.dumps({"exit_endpoint_id": os.environ["EXIT_ENDPOINT_ID_VALUE"], "name": os.environ["LINE_NAME_VALUE"], "enabled": True, "sort_weight": 100}, ensure_ascii=False))'
binding_body="$TMP_DIR/access-entry-binding-body.json"
api_json POST "/api/admin/access-entries/${access_entry_id}/exit-bindings" "$binding_payload" "$binding_body" "$admin_token"
access_line_id_a="$(json_value "$binding_body" id)"
access_line_id="$access_line_id_a"
pool_id_a="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v access_line_id="$access_line_id_a" <<'SQL'
SELECT exit_pool_id::text FROM access_lines WHERE id = :'access_line_id'::uuid LIMIT 1;
SQL
)"
pool_id="$pool_id_a"
[[ -n "$pool_id" ]] || die "access entry binding did not create runtime pool"
binding_b_payload="$TMP_DIR/access-entry-binding-b.json"
binding_b_body="$TMP_DIR/access-entry-binding-b-body.json"
LINE_NAME_VALUE="${line_name}-second" EXIT_ENDPOINT_ID_VALUE="$exit_endpoint_b" \
  write_json "$binding_b_payload" 'print(json.dumps({"exit_endpoint_id": os.environ["EXIT_ENDPOINT_ID_VALUE"], "name": os.environ["LINE_NAME_VALUE"], "enabled": True, "sort_weight": 110}, ensure_ascii=False))'
api_json POST "/api/admin/access-entries/${access_entry_id}/exit-bindings" "$binding_b_payload" "$binding_b_body" "$admin_token"
access_line_id_b="$(json_value "$binding_b_body" id)"
pool_id_b="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v access_line_id="$access_line_id_b" <<'SQL'
SELECT exit_pool_id::text FROM access_lines WHERE id = :'access_line_id'::uuid LIMIT 1;
SQL
)"
[[ -n "$access_line_id_b" && -n "$pool_id_b" ]] || die "second access entry binding did not create runtime pool"
activate_line_group_binding_node "$access_line_id_a" "$pool_id_a" "initial-first-exit"
echo "real_v2_relay_pool_e2e: line_group_bound_binding_node count=1"

psql_db -Xq -v ON_ERROR_STOP=1 \
  -v group_id="$group_id" -v access_node_id="$access_node_id" -v plan_id="$user_plan_id" <<'SQL'
BEGIN;
INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
VALUES (:'plan_id'::uuid, :'group_id'::uuid, 1)
ON CONFLICT (plan_id, line_group_id)
DO UPDATE SET billing_multiplier = EXCLUDED.billing_multiplier;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_e2e_bind_exit_endpoint'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL

subscription_yaml="$TMP_DIR/subscription.yaml"
xrayc_real_e2e_fetch_sensitive_url_to_file "${BASE_URL}/sub/${sub_token}" "$subscription_yaml" "subscription download failed"
grep -Fq "$subscription_proxy_name" "$subscription_yaml" || die "subscription does not include relay line"
grep -Fq "$PH2" "$subscription_yaml" || die "subscription does not include transit ingress"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_yaml" "$EXIT_A_PUBLIC_HOST" "subscription leaked first exit host"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_yaml" "$EXIT_B_PUBLIC_HOST" "subscription leaked second exit host"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_yaml" "${EXIT_A_PUBLIC_HOST}:${EXIT_A_VLESS_PORT}" "subscription leaked first exit endpoint"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_yaml" "${EXIT_B_PUBLIC_HOST}:${EXIT_B_VLESS_PORT}" "subscription leaked second exit endpoint"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_yaml"
subscription_after_body="$TMP_DIR/subscription-after.json"
api_auth_get /api/user/subscription "$subscription_after_body" "$user_token"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_after_body" "$EXIT_A_PUBLIC_HOST" "user subscription api leaked first exit host"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_after_body" "$EXIT_B_PUBLIC_HOST" "user subscription api leaked second exit host"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_after_body" "${EXIT_A_PUBLIC_HOST}:${EXIT_A_VLESS_PORT}" "user subscription api leaked first exit endpoint"
xrayc_real_e2e_assert_subscription_missing_literal "$subscription_after_body" "${EXIT_B_PUBLIC_HOST}:${EXIT_B_VLESS_PORT}" "user subscription api leaked second exit endpoint"
xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_after_body"
subscription_client_env="$TMP_DIR/subscription-client.env"
write_subscription_client_env "$subscription_yaml" "$subscription_proxy_name" "$PH2" "$EXIT_A_PUBLIC_HOST" "$EXIT_B_PUBLIC_HOST" "$subscription_client_env"
# shellcheck disable=SC1090
. "$subscription_client_env"
echo "real_v2_relay_pool_e2e: subscription_redaction_ok"

for _ in $(seq 1 90); do
  if remote_exec 1 "timeout 5 bash -lc '</dev/tcp/${H2}/${CLIENT_PORT}'" >/dev/null 2>&1; then
    echo "real_v2_relay_pool_e2e: ingress_runtime_port_open"
    break
  fi
  sleep 2
done
remote_exec 1 "timeout 5 bash -lc '</dev/tcp/${PH2}/${CLIENT_PORT}'" >/dev/null 2>&1 \
  || die "ingress runtime port did not open"

setup_client_runtime

wait_initial_client_exit_assignment

snapshot_seen=0
for _ in $(seq 1 24); do
  remote_exec 1 "curl --fail --silent --show-error --max-time 15 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org >/dev/null" >/dev/null 2>&1 || true
  snapshot_count="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
    -v line_id="$access_line_id" -v xray_user_key="$user_xray_key" <<'SQL'
SELECT COUNT(*)::text
FROM access_traffic_snapshots
WHERE access_line_id = :'line_id'::uuid
  AND xray_user_key = :'xray_user_key';
SQL
)"
  if [[ "${snapshot_count:-0}" =~ ^[0-9]+$ && "$snapshot_count" -gt 0 ]]; then
    snapshot_seen=1
    break
  fi
  sleep 5
done
[[ "$snapshot_seen" == "1" ]] || die "traffic snapshot baseline missing"
echo "real_v2_relay_pool_e2e: traffic_baseline_ok"

ledger_ok=0
ledger_count=0
ledger_billed=0
generated_traffic=0
for _ in $(seq 1 36); do
  IFS=$'\t' read -r ledger_count ledger_billed < <(psql_db -XAtq -F $'\t' -v ON_ERROR_STOP=1 \
    -v line_id="$access_line_id" -v user_id="$user_id" -v exit_endpoint_id="$assigned_exit_endpoint_id" <<'SQL'
SELECT COUNT(*)::text, COALESCE(SUM(billed_bytes),0)::text
FROM usage_ledgers
WHERE access_line_id = :'line_id'::uuid
  AND user_id = :'user_id'::uuid
  AND traffic_source = 'access_line'
  AND exit_endpoint_id = :'exit_endpoint_id'::uuid;
SQL
)
  if [[ "${ledger_billed:-0}" =~ ^[0-9]+$ && "$ledger_billed" -gt 0 ]]; then
    ledger_ok=1
    break
  fi
  if remote_exec 1 "curl --fail --silent --show-error --max-time 10 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org >/dev/null" >/dev/null 2>&1; then
    generated_traffic=1
  fi
  sleep 5
done

if [[ "$ledger_ok" != "1" && "$generated_traffic" != "1" ]]; then
  die "client traffic generation failed"
fi
[[ "$ledger_ok" == "1" ]] || die "usage ledger did not increase"
echo "real_v2_relay_pool_e2e: ledger_ok count=${ledger_count} billed_bytes=${ledger_billed}"

activate_line_group_binding_node "$access_line_id_b" "$pool_id_b" "second-exit-only"
subscription_proxy_name="${line_name}-second"
refresh_assigned_exit_endpoint "second-exit-only"
[[ "$assigned_exit_endpoint_id" == "$exit_endpoint_b" ]] || die "second exit assignment did not bind to the second self-hosted endpoint"
refresh_subscription_client_runtime "second-exit-only"
wait_client_expected_egress "$exit_b_ip" "second_exit_egress_ok"
second_ledger_ok=0
second_ledger_billed=0
for _ in $(seq 1 36); do
  remote_exec 1 "curl --fail --silent --show-error --max-time 10 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org >/dev/null" >/dev/null 2>&1 || true
  second_ledger_billed="$(psql_db -XAtq -v ON_ERROR_STOP=1 \
    -v line_id="$access_line_id" -v user_id="$user_id" -v exit_endpoint_id="$exit_endpoint_b" <<'SQL'
SELECT COALESCE(SUM(billed_bytes),0)::text
FROM usage_ledgers
WHERE access_line_id = :'line_id'::uuid
  AND user_id = :'user_id'::uuid
  AND traffic_source = 'access_line'
  AND exit_endpoint_id = :'exit_endpoint_id'::uuid;
SQL
)"
  if [[ "${second_ledger_billed:-0}" =~ ^[0-9]+$ && "$second_ledger_billed" -gt 0 ]]; then
    second_ledger_ok=1
    break
  fi
  sleep 5
done
[[ "$second_ledger_ok" == "1" ]] || die "second exit usage ledger did not increase"
echo "real_v2_relay_pool_e2e: second_exit_ledger_ok billed_bytes=${second_ledger_billed}"
activate_line_group_binding_node "$access_line_id_a" "$pool_id_a" "first-exit-restored"
subscription_proxy_name="$line_name"
refresh_assigned_exit_endpoint "first-exit-restored"
refresh_subscription_client_runtime "first-exit-restored"

run_user_rate_limit_e2e
run_user_udp_rate_limit_e2e

disable_access_line_for_eviction
wait_subscription_line_absent "access-line-disabled"
wait_client_blocked "access-line-disabled"
restore_access_line_after_eviction
wait_subscription_line_present "access-line-restored"
refresh_subscription_client_runtime "access-line-restored"
wait_client_restored "access-line-restored"

disable_test_user_for_eviction
wait_subscription_line_absent "disabled-user"
wait_client_blocked "disabled-user"
restore_test_user_runtime_state
wait_subscription_line_present "disabled-user"
refresh_subscription_client_runtime "disabled-user"
wait_client_restored "disabled-user"

exhaust_test_user_quota
wait_subscription_line_absent "quota-exhausted"
wait_client_blocked "quota-exhausted"
restore_test_user_runtime_state
wait_subscription_line_present "quota-exhausted"
refresh_subscription_client_runtime "quota-exhausted"
wait_client_restored "quota-exhausted"

activate_line_group_binding_node "" "" "line-group-cleared"
wait_subscription_line_absent "line-group-cleared"
wait_client_blocked "line-group-cleared"
activate_line_group_binding_node "$access_line_id_a" "$pool_id_a" "line-group-restored"
wait_subscription_line_present "line-group-restored"
refresh_subscription_client_runtime "line-group-restored"
wait_client_restored "line-group-restored"

run_real_v2_long_data_uat_if_requested
run_real_v2_runtime_env_if_requested

completed_ok=1
echo "real_v2_relay_pool_e2e: completed"
