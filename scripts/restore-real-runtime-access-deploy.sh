#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
set +x

# 真实发布长稳前的中转部署恢复脚本。
# 第三方协议矩阵会使用同一批测试服务器创建临时中转，本脚本负责把
# 长稳 UAT 需要的 access-agent / Xray 部署恢复到当前私有 env 指定的
# access_node、access_line 和 exit_endpoint，并做一次真实流量上报预检。

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
# shellcheck source=real-e2e-lib.sh
. "$BASE_DIR/scripts/real-e2e-lib.sh"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
# shellcheck source=scripts/lib/restore-real-runtime-access-deploy/functions.sh
. "$BASE_DIR/scripts/lib/restore-real-runtime-access-deploy/functions.sh"
# shellcheck source=scripts/lib/restore-real-runtime-access-deploy/db-assignments.sh
. "$BASE_DIR/scripts/lib/restore-real-runtime-access-deploy/db-assignments.sh"
trap 'die "failed at line $LINENO"' ERR
# shellcheck source=scripts/lib/restore-real-runtime-access-deploy/remote-client.sh
. "$BASE_DIR/scripts/lib/restore-real-runtime-access-deploy/remote-client.sh"

require_env BASE_URL
require_env DATABASE_URL
require_env XRAYC_REAL_E2E_INVENTORY
require_env ACCESS_NODE_ID
require_env ACCESS_LINE_ID
require_env EXIT_ENDPOINT_ID
require_env AGENT_TOKEN
require_env DEPLOY_ARTIFACT_TOKEN
require_any_env "SUB_TOKEN or SUBSCRIPTION_URL" SUB_TOKEN SUBSCRIPTION_URL
require_env EXPECTED_EXIT_IP
if [[ -z "${USER_ACCESS_TOKEN:-}" ]]; then
  [[ -n "${USER_LOGIN_ACCOUNT:-${E2E_USER_ACCOUNT:-}}" && -n "${USER_LOGIN_PASSWORD:-${E2E_USER_PASSWORD:-}}" ]] \
    || die "USER_ACCESS_TOKEN or user login credential is required"
fi

restore_exit_endpoint_id="${REAL_RUNTIME_RESTORE_EXIT_ENDPOINT_ID:-${EXIT_ENDPOINT_ID_SOCKS:-$EXIT_ENDPOINT_ID}}"
restore_expected_exit_ip="${REAL_RUNTIME_RESTORE_EXPECTED_EXIT_IP:-${EXPECTED_EXIT_IP_SOCKS:-$EXPECTED_EXIT_IP}}"
asset_env="$TMP_DIR/restore-runtime-assets.env"
resolve_restore_runtime_assets "$asset_env"
resolved_access_node_id="$(value_from_env_file "$asset_env" ACCESS_NODE_ID)"
resolved_access_line_id="$(value_from_env_file "$asset_env" ACCESS_LINE_ID)"
resolved_exit_endpoint_id="$(value_from_env_file "$asset_env" EXIT_ENDPOINT_ID)"
restore_exit_endpoint_protocol="$(value_from_env_file "$asset_env" RESTORE_EXIT_ENDPOINT_PROTOCOL)"
resolved_expected_access_servers="$(value_from_env_file "$asset_env" EXPECTED_ACCESS_SERVERS)"
[[ -n "$resolved_access_node_id" && -n "$resolved_access_line_id" && -n "$resolved_exit_endpoint_id" ]] \
  || die "restore runtime assets were not found"
ACCESS_NODE_ID="$resolved_access_node_id"
ACCESS_LINE_ID="$resolved_access_line_id"
restore_exit_endpoint_id="$resolved_exit_endpoint_id"
case "$restore_exit_endpoint_id" in
  "${REAL_RUNTIME_RESTORE_EXIT_ENDPOINT_ID:-__none__}")
    restore_expected_exit_ip="${REAL_RUNTIME_RESTORE_EXPECTED_EXIT_IP:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_SOCKS:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_SOCKS:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_HTTP:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_HTTP:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_VLESS:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_VLESS:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_TROJAN:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_TROJAN:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_SHADOWSOCKS:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_SHADOWSOCKS:-$restore_expected_exit_ip}"
    ;;
  "${EXIT_ENDPOINT_ID_HY2:-__none__}")
    restore_expected_exit_ip="${EXPECTED_EXIT_IP_HY2:-$restore_expected_exit_ip}"
    ;;
esac
case "$restore_exit_endpoint_protocol" in
  socks) restore_expected_exit_ip="${EXPECTED_EXIT_IP_SOCKS:-$restore_expected_exit_ip}" ;;
  http) restore_expected_exit_ip="${EXPECTED_EXIT_IP_HTTP:-$restore_expected_exit_ip}" ;;
  vless) restore_expected_exit_ip="${EXPECTED_EXIT_IP_VLESS:-$restore_expected_exit_ip}" ;;
  trojan) restore_expected_exit_ip="${EXPECTED_EXIT_IP_TROJAN:-$restore_expected_exit_ip}" ;;
  shadowsocks) restore_expected_exit_ip="${EXPECTED_EXIT_IP_SHADOWSOCKS:-$restore_expected_exit_ip}" ;;
  hysteria) restore_expected_exit_ip="${EXPECTED_EXIT_IP_HY2:-$restore_expected_exit_ip}" ;;
esac
[[ "$restore_exit_endpoint_id" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
  || die "restore exit endpoint id is invalid"
[[ -n "$restore_expected_exit_ip" ]] || die "restore expected exit IP is required"
export EXIT_ENDPOINT_ID="$restore_exit_endpoint_id"
export EXPECTED_EXIT_IP="$restore_expected_exit_ip"
export ACCESS_NODE_ID ACCESS_LINE_ID
upsert_env_value ACCESS_NODE_ID "$ACCESS_NODE_ID"
upsert_env_value ACCESS_LINE_ID "$ACCESS_LINE_ID"
upsert_env_value EXIT_ENDPOINT_ID "$restore_exit_endpoint_id"
upsert_env_value EXPECTED_EXIT_IP "$restore_expected_exit_ip"
upsert_env_value EXPECTED_ACCESS_SERVERS "$resolved_expected_access_servers"
upsert_env_value UAT_ACCESS_LINE_IDS "$ACCESS_LINE_ID"
upsert_env_value UAT_EXIT_ENDPOINT_IDS "$restore_exit_endpoint_id"
upsert_env_value UAT_EXPECTED_EXIT_IPS "$restore_expected_exit_ip"

admin_account="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}"
admin_password="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}"
if [[ -n "$admin_account" && -n "$admin_password" ]]; then
  admin_token="$(login_token "$admin_account" "$admin_password")"
  upsert_env_value ADMIN_ACCESS_TOKEN "$admin_token"
elif [[ -n "${ADMIN_ACCESS_TOKEN:-}" ]]; then
  admin_token="$ADMIN_ACCESS_TOKEN"
else
  die "admin login credential or ADMIN_ACCESS_TOKEN is required"
fi

user_account="${USER_LOGIN_ACCOUNT:-${E2E_USER_ACCOUNT:-}}"
user_password="${USER_LOGIN_PASSWORD:-${E2E_USER_PASSWORD:-}}"
if [[ -n "$user_account" && -n "$user_password" ]]; then
  user_token="$(login_token "$user_account" "$user_password")"
  USER_ACCESS_TOKEN="$user_token"
  export USER_ACCESS_TOKEN
  upsert_env_value USER_ACCESS_TOKEN "$USER_ACCESS_TOKEN"
elif [[ -n "${USER_ACCESS_TOKEN:-}" ]]; then
  user_token="$USER_ACCESS_TOKEN"
else
  die "user login credential or USER_ACCESS_TOKEN is required"
fi
upsert_env_value UAT_USER_ACCESS_TOKENS "$USER_ACCESS_TOKEN"

IFS=$'\t' read -r restore_node_name restore_public_host < <(
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -F $'\t' -v ON_ERROR_STOP=1 \
    -v "access_node_id=$ACCESS_NODE_ID" \
    -v "access_line_id=$ACCESS_LINE_ID" <<'SQL'
SELECT n.name,
       CASE
         WHEN NULLIF(n.public_host, '') IS NOT NULL
          AND (
            NULLIF(l.listen_host, '') IS NULL
            OR lower(l.listen_host) LIKE '%example%'
            OR lower(l.listen_host) LIKE '%test%'
            OR lower(l.listen_host) LIKE '%placeholder%'
          )
           THEN n.public_host
         ELSE COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))
       END
FROM access_lines l
JOIN access_nodes n ON n.id = l.access_node_id
WHERE l.id = :'access_line_id'::uuid
  AND n.id = :'access_node_id'::uuid;
SQL
)
[[ -n "${restore_node_name:-}" && -n "${restore_public_host:-}" ]] \
  || die "restore access line target is missing"

RESTORE_PUBLIC_HOST="$restore_public_host" \
RESTORE_TARGET_INDEX="${XRAYC_RUNTIME_RESTORE_TARGET_INDEX:-}" \
python3 - "$XRAYC_REAL_E2E_INVENTORY" >"$TMP_DIR/target.env" <<'PY'
import json
import os
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
if not targets:
    raise SystemExit("inventory requires at least one target")

restore_public_host = os.environ.get("RESTORE_PUBLIC_HOST", "").strip()
target_index = os.environ.get("RESTORE_TARGET_INDEX", "").strip()
target = None
selected_index = None
if target_index:
    try:
        requested_index = int(target_index)
    except ValueError:
        raise SystemExit("restore target index is invalid")
    if requested_index > 0:
        index = requested_index - 1
        try:
            target = targets[index]
            selected_index = index
        except IndexError:
            raise SystemExit("restore target index is invalid")
if target is None:
    for index, item in enumerate(targets):
        candidates = {
            str(item.get("public_domain", "")).strip(),
            str(item.get("public_host", "")).strip(),
            str(item.get("ssh_host", "")).strip(),
        }
        if restore_public_host in candidates:
            target = item
            selected_index = index
            break
if target is None:
    raise SystemExit("restore target host is not present in inventory")

public_host = (
    str(target.get("public_domain", "")).strip()
    or str(target.get("public_host", "")).strip()
    or str(target.get("ssh_host", "")).strip()
)
values = {
    "TRANSIT_SSH_HOST": target.get("ssh_host", ""),
    "TRANSIT_SSH_USER": target.get("ssh_user", "root"),
    "TRANSIT_SSH_PORT": str(target.get("ssh_port", 22)),
    "TRANSIT_SSH_PASSWORD_FILE": target.get("ssh_password_file", ""),
    "TRANSIT_PUBLIC_HOST": public_host,
    "TRANSIT_TARGET_INDEX": str((selected_index or 0) + 1),
}
for key, value in values.items():
    if not str(value).strip():
        raise SystemExit(f"inventory target missing {key}")
    print(f"{key}={shlex.quote(str(value))}")
PY
# shellcheck disable=SC1090
. "$TMP_DIR/target.env"
export TRANSIT_SSH_HOST TRANSIT_SSH_USER TRANSIT_SSH_PORT TRANSIT_SSH_PASSWORD_FILE TRANSIT_PUBLIC_HOST
upsert_env_value BACKLOG_TARGET_INDEX "$TRANSIT_TARGET_INDEX"

resolved_target_access_node_id="$(
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
    -v "public_host=$TRANSIT_PUBLIC_HOST" \
    -v "ssh_host=$TRANSIT_SSH_HOST" <<'SQL'
SELECT id::text
FROM access_nodes
WHERE lower(public_host) = lower(:'public_host')
   OR lower(name) = lower(:'public_host')
   OR lower(public_host) = lower(:'ssh_host')
   OR lower(name) = lower(:'ssh_host')
ORDER BY last_heartbeat_at DESC NULLS LAST, created_at DESC NULLS LAST
LIMIT 1;
SQL
)"
[[ -n "$resolved_target_access_node_id" ]] || die "restore target access node is missing"
ACCESS_NODE_ID="$resolved_target_access_node_id"
export ACCESS_NODE_ID
upsert_env_value ACCESS_NODE_ID "$ACCESS_NODE_ID"
upsert_env_value UAT_ACCESS_NODE_IDS "$ACCESS_NODE_ID"
restore_agent_token="${XRAYC_RUNTIME_RESTORE_AGENT_TOKEN:-$(python3 - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
)}"
restore_agent_token_hash="$(sha256_text "$restore_agent_token")"
rotate_restore_agent_token_sql="$TMP_DIR/rotate-restore-agent-token.sql"
cat >"$rotate_restore_agent_token_sql" <<'SQL'
UPDATE access_nodes
SET agent_token_hash = :'agent_token_hash',
    config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'restore_real_runtime_agent_token'
WHERE id = :'access_node_id'::uuid;
SQL
run_psql "$rotate_restore_agent_token_sql" \
  -v "access_node_id=$ACCESS_NODE_ID" \
  -v "agent_token_hash=$restore_agent_token_hash"
AGENT_TOKEN="$restore_agent_token"
export AGENT_TOKEN
upsert_env_value AGENT_TOKEN "$AGENT_TOKEN"
upsert_env_value UAT_AGENT_TOKENS "$AGENT_TOKEN"
restore_listen_port="${XRAYC_RUNTIME_RESTORE_ACCESS_PORT:-35443}"
[[ "$restore_listen_port" =~ ^[1-9][0-9]*$ && "$restore_listen_port" -le 65535 ]] \
  || die "XRAYC_RUNTIME_RESTORE_ACCESS_PORT must be a TCP port"

sync_restore_host_sql="$TMP_DIR/sync-restore-host.sql"
cat >"$sync_restore_host_sql" <<'SQL'
UPDATE access_lines
SET access_node_id = :'access_node_id'::uuid,
    listen_host = :'public_host',
    listen_port = :'listen_port'::integer,
    server_name = CASE
      WHEN lower(protocol) IN ('vless', 'trojan')
       AND (
         NULLIF(server_name, '') IS NULL
         OR lower(server_name) LIKE '%example%'
         OR lower(server_name) LIKE '%test%'
         OR lower(server_name) LIKE '%placeholder%'
       )
        THEN :'public_host'
      ELSE server_name
    END
WHERE id = :'access_line_id'::uuid;
SQL
run_psql "$sync_restore_host_sql" \
  -v "access_node_id=$ACCESS_NODE_ID" \
  -v "access_line_id=$ACCESS_LINE_ID" \
  -v "public_host=$TRANSIT_PUBLIC_HOST" \
  -v "listen_port=$restore_listen_port"

[[ -f "$TRANSIT_SSH_PASSWORD_FILE" ]] || die "transit SSH password file is missing"

install_dir="${XRAYC_RUNTIME_RESTORE_INSTALL_DIR:-/opt/xrayc-real-relay}"
compose_project="${XRAYC_RUNTIME_RESTORE_COMPOSE_PROJECT:-xrayc-real-relay}"
xray_api_port="${XRAYC_RUNTIME_RESTORE_XRAY_API_PORT:-10085}"
xray_api_server="${XRAYC_RUNTIME_RESTORE_XRAY_API_SERVER:-127.0.0.1:${xray_api_port}}"
restore_client_init

# 清理当前测试项目自己的旧容器、状态和 backlog；不触碰 SSH 进程和非本项目资源。
sshpass -f "$TRANSIT_SSH_PASSWORD_FILE" ssh -p "$TRANSIT_SSH_PORT" \
  -o ConnectTimeout=15 -o ServerAliveInterval=15 \
  -o StrictHostKeyChecking=accept-new -o BatchMode=no \
  "$TRANSIT_SSH_USER@$TRANSIT_SSH_HOST" \
  "docker compose -p '$compose_project' -f '$install_dir/docker-compose.yml' down --remove-orphans >/dev/null 2>&1 || true; docker rm -f \$(docker ps -aq --filter label=com.docker.compose.project='$compose_project') >/dev/null 2>&1 || true; rm -rf '$install_dir'" >/dev/null

cleanup_sql="$TMP_DIR/cleanup.sql"
cat >"$cleanup_sql" <<'SQL'
DELETE FROM access_exit_probes
WHERE access_node_id = :'access_node_id'::uuid
  AND status = 'queued';
SQL
run_psql "$cleanup_sql" -v "access_node_id=$ACCESS_NODE_ID"

restore_runtime_sync_subscription_assignment

IFS=$'\t' read -r restore_line_protocol restore_line_port < <(
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -F $'\t' -v ON_ERROR_STOP=1 \
    -v "access_line_id=$ACCESS_LINE_ID" <<'SQL'
SELECT lower(protocol), listen_port::text
FROM access_lines
WHERE id = :'access_line_id'::uuid;
SQL
)
[[ -n "${restore_line_protocol:-}" && -n "${restore_line_port:-}" ]] || die "access line is missing"
case "$restore_line_protocol" in
  ss) restore_line_protocol="shadowsocks" ;;
esac
case "$restore_line_protocol" in
  vless|trojan|shadowsocks) ;;
  *) die "restore client does not support current access line protocol" ;;
esac
[[ "$restore_line_port" =~ ^[1-9][0-9]*$ ]] || die "access line port is invalid"
upsert_env_value EXPECTED_ACCESS_SERVERS "${TRANSIT_PUBLIC_HOST}:${restore_line_port}"

guide_payload="$TMP_DIR/agent-install-guide.json"
INSTALL_DIR_VALUE="$install_dir" \
COMPOSE_PROJECT_VALUE="$compose_project" \
XRAY_API_SERVER_VALUE="$xray_api_server" \
XRAY_API_PORT_VALUE="$xray_api_port" \
RESTORE_LINE_PORT_VALUE="$restore_line_port" \
  write_json "$guide_payload" 'print(json.dumps({
    "access_node_id": os.environ["ACCESS_NODE_ID"],
    "install_dir": os.environ["INSTALL_DIR_VALUE"],
    "compose_project_name": os.environ["COMPOSE_PROJECT_VALUE"],
    "panel_url": os.environ["BASE_URL"],
    "xray_api_server": os.environ["XRAY_API_SERVER_VALUE"],
    "xray_api_port": int(os.environ["XRAY_API_PORT_VALUE"]),
    "heartbeat_interval_seconds": 5,
    "traffic_interval_seconds": 5,
    "session_idle_seconds": 30,
    "tls_cert_domains": ([] if __import__("re").fullmatch(r"\d+(\.\d+){3}", os.environ["TRANSIT_PUBLIC_HOST"]) else [os.environ["TRANSIT_PUBLIC_HOST"]]),
    "expected_listen_ports": [int(os.environ["RESTORE_LINE_PORT_VALUE"])],
    "disable_legacy_systemd_units": True,
    "force_reinstall": True,
}, ensure_ascii=False))'
guide_body="$TMP_DIR/agent-install-guide-body.json"
api_json POST /api/admin/access-nodes/install-guide "$guide_payload" "$guide_body" "$admin_token"
guide_access_node_id="$(json_value "$guide_body" access_node_id)"
[[ "$guide_access_node_id" == "$ACCESS_NODE_ID" ]] || die "install guide returned unexpected access node"

install_inventory="$TMP_DIR/agent-install-inventory.json"
INSTALL_DIR_VALUE="$install_dir" \
COMPOSE_PROJECT_VALUE="$compose_project" \
XRAY_API_SERVER_VALUE="$xray_api_server" \
XRAY_API_PORT_VALUE="$xray_api_port" \
RESTORE_LINE_PORT_VALUE="$restore_line_port" \
  write_json "$install_inventory" 'print(json.dumps({"targets": [{
    "alias": "restore-runtime-access",
    "ssh_host": os.environ["TRANSIT_SSH_HOST"],
    "ssh_user": os.environ["TRANSIT_SSH_USER"],
    "ssh_port": int(os.environ["TRANSIT_SSH_PORT"]),
    "ssh_password_file": os.environ["TRANSIT_SSH_PASSWORD_FILE"],
    "control_plane_url": os.environ["BASE_URL"],
    "deploy_artifact_token": os.environ["DEPLOY_ARTIFACT_TOKEN"],
    "node_id": os.environ["ACCESS_NODE_ID"],
    "node_token": os.environ["AGENT_TOKEN"],
    "install_dir": os.environ["INSTALL_DIR_VALUE"],
    "compose_project": os.environ["COMPOSE_PROJECT_VALUE"],
    "xray_api_server": os.environ["XRAY_API_SERVER_VALUE"],
    "xray_api_listen_host": "127.0.0.1",
    "xray_api_listen_port": int(os.environ["XRAY_API_PORT_VALUE"]),
    "heartbeat_interval_seconds": 5,
    "traffic_interval_seconds": 5,
    "session_idle_seconds": 30,
    "tls_cert_domains": ([] if __import__("re").fullmatch(r"\d+(\.\d+){3}", os.environ["TRANSIT_PUBLIC_HOST"]) else [os.environ["TRANSIT_PUBLIC_HOST"]]),
    "expected_listen_ports": [int(os.environ["RESTORE_LINE_PORT_VALUE"])]
  }]}, ensure_ascii=False))'
remote_allow_insecure=0
case "$BASE_URL" in
  http://*) remote_allow_insecure=1 ;;
esac
XRAYC_REAL_E2E_INVENTORY="$install_inventory" \
XRAYC_REAL_E2E_TARGET="restore-runtime-access" \
XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP="$remote_allow_insecure" \
XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1 \
XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE="${XRAYC_RUNTIME_RESTORE_CLEANUP_ON_FAILURE:-1}" \
  bash scripts/real-remote-access-deploy-e2e.sh

subscription_url="${SUBSCRIPTION_URL:-}"
if [[ -z "$subscription_url" ]]; then
  subscription_url="${BASE_URL%/}/sub/${SUB_TOKEN}"
fi
subscription_file="$TMP_DIR/subscription.yaml"
xrayc_real_e2e_fetch_sensitive_url_to_file \
  "$subscription_url" \
  "$subscription_file" \
  "subscription download failed for restore client"

client_port="$(restore_client_config_port)"
client_config_dir="$TMP_DIR/restore-client"
python3 scripts/real-access-inbound-matrix-client-configs.py build \
  "$subscription_file" \
  "$client_config_dir" \
  "$client_port" \
  --preferred-port "${restore_line_protocol}=${restore_line_port}" \
  "$restore_line_protocol" >/dev/null
client_config_file="$client_config_dir/${restore_line_protocol}.json"
[[ -f "$client_config_file" ]] || die "restore client config was not generated"

restore_client_start "$client_config_file" "$client_port"

egress_ok=0
for _ in $(seq 1 18); do
  actual_exit_ip="$(curl --fail --silent --location \
    --connect-timeout 8 \
    --max-time 20 \
    --proxy "$CLIENT_PROXY_URL" \
    "${PUBLIC_IP_URL:-https://api.ipify.org}" 2>/dev/null || true)"
  if [[ -n "$actual_exit_ip" && "$actual_exit_ip" == "$EXPECTED_EXIT_IP" ]]; then
    egress_ok=1
    break
  fi
  sleep 5
done
[[ "$egress_ok" == "1" ]] || die "client egress preflight failed"

generate_client_traffic() {
  curl --fail --silent --location \
    --connect-timeout 8 \
    --max-time "${UAT_CLIENT_TRAFFIC_MAX_TIME_SECONDS:-25}" \
    --limit-rate "${UAT_CLIENT_TRAFFIC_RATE:-64k}" \
    --proxy "$CLIENT_PROXY_URL" \
    --output /dev/null \
    "${UAT_CLIENT_TRAFFIC_URL:-https://speed.cloudflare.com/__down?bytes=1048576}" 2>/dev/null
}

traffic_ok=0
for _ in $(seq 1 18); do
  if generate_client_traffic; then
    traffic_ok=1
    break
  fi
  sleep 5
done
[[ "$traffic_ok" == "1" ]] || die "client traffic preflight failed"

probe_sql="$TMP_DIR/probe.sql"
cat >"$probe_sql" <<'SQL'
WITH checks AS (
  SELECT 'heartbeat' AS name, EXISTS (
    SELECT 1 FROM access_nodes
    WHERE id = :'access_node_id'::uuid
      AND last_heartbeat_at >= now() - interval '2 minutes'
      AND config_dirty = FALSE
  ) AS ok
  UNION ALL
  SELECT 'metric', EXISTS (
    SELECT 1 FROM access_line_metric_snapshots
    WHERE access_node_id = :'access_node_id'::uuid
      AND access_line_id = :'access_line_id'::uuid
      AND collected_at >= now() - interval '2 minutes'
  )
  UNION ALL
  SELECT 'session', EXISTS (
    SELECT 1 FROM access_user_sessions
    WHERE access_node_id = :'access_node_id'::uuid
      AND access_line_id = :'access_line_id'::uuid
      AND last_seen_at >= now() - interval '2 minutes'
  )
  UNION ALL
  SELECT 'ledger', EXISTS (
    SELECT 1 FROM usage_ledgers
    WHERE access_line_id = :'access_line_id'::uuid
      AND recorded_at >= now() - interval '2 minutes'
      AND billed_bytes > 0
  )
)
SELECT COALESCE(string_agg(name, ',' ORDER BY name), '')
FROM checks
WHERE NOT ok;
SQL

missing=""
for _ in $(seq 1 36); do
  missing="$(xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -v ON_ERROR_STOP=1 \
    -v "access_node_id=$ACCESS_NODE_ID" \
    -v "access_line_id=$ACCESS_LINE_ID" \
    -f "$probe_sql")"
  [[ -z "$missing" ]] && break
  generate_client_traffic >/dev/null 2>&1 || true
  sleep 5
done

if [[ -n "$missing" ]]; then
  die "runtime preflight missing ${missing}"
fi

echo "restore-real-runtime-access-deploy: passed"
