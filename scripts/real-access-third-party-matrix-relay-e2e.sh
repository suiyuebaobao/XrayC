#!/usr/bin/env bash
# 用途：执行真实第三方协议矩阵经中继接入节点的端到端验证。
# 本入口只保留环境加载、临时资源编排和核心验证流程。
# 函数实现拆到 scripts/lib/real-access-third-party-matrix-relay/functions.sh。
# 私有 inventory 与 matrix env 会被读取，但第三方 endpoint 敏感材料不能输出。
set -euo pipefail
IFS=$'\n\t'
set +x

umask 077

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
EXPLICIT_BASE_URL="${BASE_URL:-}"
EXPLICIT_ADMIN_ACCOUNT="${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-}"
EXPLICIT_ADMIN_PASSWORD="${XRAYC_REAL_E2E_ADMIN_PASSWORD:-}"
EXPLICIT_USER_ACCOUNT="${XRAYC_REAL_E2E_USER_ACCOUNT:-}"
EXPLICIT_USER_PASSWORD="${XRAYC_REAL_E2E_USER_PASSWORD:-}"
# shellcheck disable=SC1091
. "$BASE_DIR/scripts/real-e2e-lib.sh"

MATRIX_ENV_FILE="${XRAYC_REAL_PROTOCOL_MATRIX_ENV_FILE:-${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}}"
if [[ -f "$MATRIX_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$MATRIX_ENV_FILE"
  set +a
fi
[[ -n "$EXPLICIT_BASE_URL" ]] && BASE_URL="$EXPLICIT_BASE_URL"
[[ -n "$EXPLICIT_ADMIN_ACCOUNT" ]] && XRAYC_REAL_E2E_ADMIN_ACCOUNT="$EXPLICIT_ADMIN_ACCOUNT"
[[ -n "$EXPLICIT_ADMIN_PASSWORD" ]] && XRAYC_REAL_E2E_ADMIN_PASSWORD="$EXPLICIT_ADMIN_PASSWORD"
[[ -n "$EXPLICIT_USER_ACCOUNT" ]] && XRAYC_REAL_E2E_USER_ACCOUNT="$EXPLICIT_USER_ACCOUNT"
[[ -n "$EXPLICIT_USER_PASSWORD" ]] && XRAYC_REAL_E2E_USER_PASSWORD="$EXPLICIT_USER_PASSWORD"
MATRIX_ENV_FILE="${XRAYC_REAL_PROTOCOL_MATRIX_ENV_FILE:-${XRAYC_REAL_RELEASE_ENV_FILE:-$MATRIX_ENV_FILE}}"
if [[ -f "$MATRIX_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$MATRIX_ENV_FILE"
  set +a
fi
[[ -n "$EXPLICIT_BASE_URL" ]] && BASE_URL="$EXPLICIT_BASE_URL"
[[ -n "$EXPLICIT_ADMIN_ACCOUNT" ]] && XRAYC_REAL_E2E_ADMIN_ACCOUNT="$EXPLICIT_ADMIN_ACCOUNT"
[[ -n "$EXPLICIT_ADMIN_PASSWORD" ]] && XRAYC_REAL_E2E_ADMIN_PASSWORD="$EXPLICIT_ADMIN_PASSWORD"
[[ -n "$EXPLICIT_USER_ACCOUNT" ]] && XRAYC_REAL_E2E_USER_ACCOUNT="$EXPLICIT_USER_ACCOUNT"
[[ -n "$EXPLICIT_USER_PASSWORD" ]] && XRAYC_REAL_E2E_USER_PASSWORD="$EXPLICIT_USER_PASSWORD"

INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-文档/私有/remote-e2e-inventory.generated.json}"
DEPLOY_TOKEN_FILE="${XRAYC_DEPLOY_ARTIFACT_TOKEN_FILE:-文档/私有/deploy-artifact-token.env}"
LISTEN_PORT="${XRAYC_REAL_MATRIX_RELAY_LISTEN_PORT:-25443}"
CLIENT_SOCKS_PORT="${XRAYC_REAL_MATRIX_RELAY_CLIENT_SOCKS_PORT:-32080}"
XRAY_API_PORT="${XRAYC_REAL_MATRIX_RELAY_XRAY_API_PORT:-11085}"
RUN_ID="$(date +%Y%m%d%H%M%S)-$$"
TMP_DIR="$(mktemp -d)"

access_node_id=""
access_line_id=""
pool_id=""
group_id=""
user_id=""
user_xray_key=""
sub_token=""
line_name=""
created_pool=0
original_user_disabled=""
original_sub_active=""
original_sub_expires_at=""
original_sub_used_bytes=""
original_sub_limit_bytes=""
original_user_snapshot_saved=0
original_subscription_snapshot_saved=0

protocol_names=()
protocol_suffixes=()
protocol_endpoint_ids=()
protocol_outbound_types=()
protocol_expected_ips=()
endpoint_host_literals=()
endpoint_sensitive_literals=()
keep_debug_artifacts="${XRAYC_REAL_MATRIX_RELAY_KEEP_DEBUG:-0}"
completed_ok=0
endpoint_hosts_file=""

# shellcheck disable=SC1091
. "$BASE_DIR/scripts/lib/real-access-third-party-matrix-relay/functions.sh"
trap 'on_error "$LINENO"' ERR
trap cleanup EXIT

require_command curl
require_command python3
require_command ssh
require_file "$INVENTORY"
require_file "$MATRIX_ENV_FILE"

DB_URL="${DATABASE_URL:-postgres://xrayc:change-me@127.0.0.1:15432/xrayc}"
ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}}"
ADMIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${XRAYC_REAL_E2E_ADMIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}}"
USER_ACCOUNT="${USER_LOGIN_ACCOUNT:-${XRAYC_REAL_E2E_USER_ACCOUNT:-${E2E_USER_ACCOUNT:-}}}"
USER_PASSWORD="${USER_LOGIN_PASSWORD:-${XRAYC_REAL_E2E_USER_PASSWORD:-${E2E_USER_PASSWORD:-}}}"
PROTOCOLS="${REAL_PROTOCOL_MATRIX_PROTOCOLS:-socks,http,vless,trojan,shadowsocks,hy2}"
PUBLIC_IP_URL="${XRAYC_REAL_E2E_PUBLIC_IP_URL:-https://api.ipify.org}"
PUBLIC_IP_URLS_CSV="${XRAYC_REAL_E2E_PUBLIC_IP_URLS:-${PUBLIC_IP_URL},https://ifconfig.me/ip,https://icanhazip.com}"
IFS=',' read -r -a PUBLIC_IP_URLS <<< "$PUBLIC_IP_URLS_CSV"
if [[ "${XRAYC_REAL_RELEASE:-0}" == "1" ]]; then
  require_release_value DATABASE_URL
  require_release_value ADMIN_ACCOUNT
  require_release_value ADMIN_PASSWORD
  require_release_value USER_ACCOUNT
  require_release_value USER_PASSWORD
else
  ADMIN_ACCOUNT="${ADMIN_ACCOUNT:-admin@example.test}"
  ADMIN_PASSWORD="${ADMIN_PASSWORD:-admin123456}"
  USER_ACCOUNT="${USER_ACCOUNT:-demo@example.test}"
  USER_PASSWORD="${USER_PASSWORD:-demo123456}"
fi

BASE_URL="${BASE_URL:-${XRAYC_CONTROL_PLANE_URL:-}}"
[[ -n "$BASE_URL" ]] || die "BASE_URL is required in private matrix env"
BASE_URL="${BASE_URL%/}"
DATABASE_URL="${DB_URL}"
export DATABASE_URL

if value_is_placeholder "${DEPLOY_ARTIFACT_TOKEN:-}" && [[ -f "$DEPLOY_TOKEN_FILE" ]]; then
  # shellcheck disable=SC1090
  . "$DEPLOY_TOKEN_FILE"
fi
DEPLOY_TOKEN="${DEPLOY_ARTIFACT_TOKEN:-${XRAYC_DEPLOY_ARTIFACT_TOKEN:-}}"
value_is_placeholder "$DEPLOY_TOKEN" && die "deploy artifact token is missing"

require_matrix_protocols
require_full_release_protocol_matrix

python3 - "$INVENTORY" "${XRAYC_REAL_TEST_SERVER_COUNT:-}" > "$TMP_DIR/targets.env" <<'PY'
import json
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
requested_raw = sys.argv[2].strip()
requested = int(requested_raw) if requested_raw else len(targets)
if requested < 3:
    raise SystemExit("inventory must contain at least three targets")
if len(targets) != requested:
    raise SystemExit("inventory target count must match requested target count")
hosts = [str(target.get("ssh_host", "")).strip() for target in targets[:requested]]
if len(set(hosts)) != requested:
    raise SystemExit("inventory targets must be distinct hosts")
print(f"TARGET_COUNT={requested}")
for index, target in enumerate(targets[:requested], 1):
    public_host = (
        str(target.get("public_domain", "")).strip()
        or str(target.get("public_host", "")).strip()
        or str(target.get("ssh_host", "")).strip()
    )
    values = {
        f"A{index}": target.get("alias", ""),
        f"H{index}": target.get("ssh_host", ""),
        f"PH{index}": public_host,
        f"U{index}": target.get("ssh_user", "root"),
        f"P{index}": str(target.get("ssh_port", 22)),
        f"PF{index}": target.get("ssh_password_file", ""),
        f"IF{index}": target.get("ssh_identity_file", ""),
    }
    if values[f"A{index}"] != f"server_{index}":
        raise SystemExit("inventory aliases must be sequential server_N labels")
    if index == 1:
        values["INVENTORY_BASE_URL"] = target.get("control_plane_url", "")
    if not values[f"PF{index}"] and not values[f"IF{index}"]:
        raise SystemExit(f"inventory target server_{index} is missing SSH auth")
    for key, value in values.items():
        if key == "INVENTORY_BASE_URL" or key.startswith("PF") or key.startswith("IF"):
            pass
        elif not str(value).strip():
            raise SystemExit(f"inventory target missing {key}")
        print(f"{key}={shlex.quote(str(value))}")
PY
# shellcheck disable=SC1090
. "$TMP_DIR/targets.env"
for idx in $(seq 1 "$TARGET_COUNT"); do
  eval "passfile=\${PF${idx}:-}" "identity_file=\${IF${idx}:-}"
  if [[ -z "$passfile" && -z "$identity_file" ]]; then
    die "server_${idx} SSH auth file is missing"
  fi
  if [[ -n "$passfile" ]]; then
    require_command sshpass
    [[ -f "$passfile" ]] || die "server_${idx} SSH password file is missing"
  fi
  if [[ -n "$identity_file" ]]; then
    [[ -f "$identity_file" ]] || die "server_${idx} SSH identity file is missing"
  fi
done
if [[ -z "${BASE_URL:-}" && -n "${INVENTORY_BASE_URL:-}" ]]; then
  BASE_URL="${INVENTORY_BASE_URL%/}"
fi

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
original_user_disabled="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT disabled::text FROM users WHERE id = :'user_id'::uuid LIMIT 1;
SQL
)"
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
    limit_bytes = GREATEST(limit_bytes, 10737418240),
    updated_at = now()
WHERE user_id = :'user_id'::uuid;
SQL

subscription_body="$TMP_DIR/subscription.json"
api_auth_get /api/user/subscription "$subscription_body" "$user_token"
sub_token="$(json_value "$subscription_body" token)"
status "auth_and_subscription_ok"

subscription_group_name="real-matrix-relay-group-${RUN_ID}"

endpoint_id_csv="$(IFS=,; printf '%s' "${protocol_endpoint_ids[*]}")"
endpoint_hosts_file="$TMP_DIR/endpoint-hosts.tsv"
: > "$endpoint_hosts_file"
while IFS=$'\t' read -r endpoint_id host port outbound_type; do
  [[ -n "$host" && -n "$port" ]] && printf '%s\t%s\n' "$host" "$port" >> "$endpoint_hosts_file"
  case ",${endpoint_id_csv}," in
    *",${endpoint_id},"*) ;;
    *) die "endpoint lookup mismatch" ;;
  esac
  [[ -n "$outbound_type" ]] || die "endpoint outbound type lookup failed"
done < <(
  psql_db -XAtq -F $'\t' -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_id_csv" <<'SQL'
SELECT id::text, host, port::text, outbound_type::text
FROM exit_endpoints
WHERE id = ANY(string_to_array(:'endpoint_ids', ',')::uuid[])
  AND enabled = TRUE;
SQL
)

endpoint_count="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_id_csv" <<'SQL'
SELECT COUNT(*)::text
FROM exit_endpoints
WHERE id = ANY(string_to_array(:'endpoint_ids', ',')::uuid[])
  AND enabled = TRUE;
SQL
)"
[[ "$endpoint_count" == "${#protocol_endpoint_ids[@]}" ]] || die "one or more matrix endpoints are missing or disabled"
refresh_protocol_matrix_endpoint_health "$endpoint_id_csv"
assert_protocol_matrix_endpoints_subscription_eligible "$endpoint_id_csv"
for index in "${!protocol_names[@]}"; do
  assert_protocol_endpoint_reachable_from_relay "${protocol_names[$index]}" "${protocol_endpoint_ids[$index]}"
done
initial_endpoint_id="$(select_initial_relay_endpoint_id "$endpoint_id_csv")"
[[ -n "$initial_endpoint_id" ]] || die "no subscription-eligible matrix endpoint is available for initial relay binding"

while IFS= read -r literal; do
  [[ -n "$literal" ]] && endpoint_sensitive_literals+=("$literal")
done < <(
  psql_db -XAtq -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_id_csv" <<'SQL'
SELECT DISTINCT value
FROM exit_endpoints
CROSS JOIN LATERAL jsonb_each_text(COALESCE(outbound_config, '{}'::jsonb)) AS secret(key, value)
WHERE id = ANY(string_to_array(:'endpoint_ids', ',')::uuid[])
  AND key IN (
    'username', 'user', 'password', 'pass', 'auth', 'key', 'uuid',
    'public_key', 'private_key', 'short_id', 'server_name', 'sni',
    'obfs_password'
  )
  AND value IS NOT NULL
  AND length(value) >= 4
  AND lower(value) NOT IN ('real', 'tcp', 'udp', 'tls', 'reality', 'chrome', 'auto', 'stream-one', 'true', 'false', 'none');
SQL
)

transit_auth_kind=""
transit_auth_value=""
if [[ -n "${PF2:-}" ]]; then
  transit_auth_kind="password"
  transit_auth_value="$PF2"
elif [[ -n "${IF2:-}" ]]; then
  transit_auth_kind="identity_file"
  transit_auth_value="$IF2"
else
  die "server_2 SSH auth file is missing"
fi
agent_token="$(python3 - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
)"
node_payload="$TMP_DIR/access-node.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_PUBLIC_HOST="$PH2" LISTEN_PORT_VALUE="$LISTEN_PORT" \
  AGENT_TOKEN_VALUE="$agent_token" \
  write_json "$node_payload" 'rid = os.environ["RUN_ID_VALUE"]; print(json.dumps({"name": f"real-matrix-relay-{rid}", "public_host": os.environ["TRANSIT_PUBLIC_HOST"], "public_port": int(os.environ["LISTEN_PORT_VALUE"]), "agent_token": os.environ["AGENT_TOKEN_VALUE"], "remark": "real protocol matrix relay e2e"}, ensure_ascii=False))'
node_body="$TMP_DIR/access-node-body.json"
api_json POST /api/admin/access-nodes "$node_payload" "$node_body" "$admin_token"
access_node_id="$(json_value "$node_body" id)"
[[ -n "$access_node_id" ]] || die "relay access node creation failed"
guide_payload="$TMP_DIR/agent-install-guide.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_PUBLIC_HOST="$PH2" XRAY_API_PORT_VALUE="$XRAY_API_PORT" BASE_URL_VALUE="$BASE_URL" \
  ACCESS_NODE_ID_VALUE="$access_node_id" \
  write_json "$guide_payload" 'import re; xray_api_port = int(os.environ["XRAY_API_PORT_VALUE"]); transit_host = os.environ["TRANSIT_PUBLIC_HOST"].strip(); payload = {"access_node_id": os.environ["ACCESS_NODE_ID_VALUE"], "install_dir": "/opt/xrayc-real-matrix-relay", "compose_project_name": "xrayc-real-matrix-relay", "panel_url": os.environ["BASE_URL_VALUE"], "xray_api_server": f"127.0.0.1:{xray_api_port}", "xray_api_port": xray_api_port, "heartbeat_interval_seconds": 5, "traffic_interval_seconds": 5, "session_idle_seconds": 30, "expected_listen_ports": [], "disable_legacy_systemd_units": True, "force_reinstall": True}; payload.update({"tls_cert_domains": [transit_host]} if transit_host and not re.fullmatch(r"[0-9]+(\.[0-9]+){3}", transit_host) else {}); print(json.dumps(payload, ensure_ascii=False))'
guide_body="$TMP_DIR/agent-install-guide-body.json"
api_json POST /api/admin/access-nodes/install-guide "$guide_payload" "$guide_body" "$admin_token"
guide_access_node_id="$(json_value "$guide_body" access_node_id)"
[[ "$guide_access_node_id" == "$access_node_id" ]] || die "agent install guide returned unexpected relay node"
install_inventory="$TMP_DIR/agent-install-inventory.json"
RUN_ID_VALUE="$RUN_ID" TRANSIT_SSH_HOST="$H2" TRANSIT_PUBLIC_HOST="$PH2" TRANSIT_USER="$U2" TRANSIT_PORT="$P2" \
  XRAY_API_PORT_VALUE="$XRAY_API_PORT" TRANSIT_AUTH_KIND="$transit_auth_kind" TRANSIT_AUTH_VALUE="$transit_auth_value" \
  BASE_URL_VALUE="$BASE_URL" ACCESS_NODE_ID_VALUE="$access_node_id" AGENT_TOKEN_VALUE="$agent_token" DEPLOY_TOKEN_VALUE="$DEPLOY_TOKEN" \
  write_json "$install_inventory" 'import re; rid = os.environ["RUN_ID_VALUE"]; xray_api_port = int(os.environ["XRAY_API_PORT_VALUE"]); transit_host = os.environ["TRANSIT_PUBLIC_HOST"].strip(); target = {"alias": f"real-matrix-relay-{rid}", "ssh_host": os.environ["TRANSIT_SSH_HOST"], "ssh_user": os.environ["TRANSIT_USER"], "ssh_port": int(os.environ["TRANSIT_PORT"]), "control_plane_url": os.environ["BASE_URL_VALUE"], "deploy_artifact_token": os.environ["DEPLOY_TOKEN_VALUE"], "node_id": os.environ["ACCESS_NODE_ID_VALUE"], "node_token": os.environ["AGENT_TOKEN_VALUE"], "install_dir": "/opt/xrayc-real-matrix-relay", "compose_project": "xrayc-real-matrix-relay", "xray_api_server": f"127.0.0.1:{xray_api_port}", "xray_api_listen_host": "127.0.0.1", "xray_api_listen_port": xray_api_port, "heartbeat_interval_seconds": 5, "traffic_interval_seconds": 5, "session_idle_seconds": 30, "expected_listen_ports": []}; target.update({"tls_cert_domains": [transit_host]} if transit_host and not re.fullmatch(r"[0-9]+(\.[0-9]+){3}", transit_host) else {}); kind = os.environ["TRANSIT_AUTH_KIND"]; value = os.environ["TRANSIT_AUTH_VALUE"]; target["ssh_password_file" if kind == "password" else "ssh_identity_file"] = value; print(json.dumps({"targets": [target]}, ensure_ascii=False))'
transit_auth_value=""
remote_allow_insecure=0
case "$BASE_URL" in
  http://*) remote_allow_insecure=1 ;;
esac
XRAYC_REAL_E2E_INVENTORY="$install_inventory" \
XRAYC_REAL_E2E_TARGET="real-matrix-relay-${RUN_ID}" \
XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP="$remote_allow_insecure" \
XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1 \
XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE=1 \
  bash scripts/real-remote-access-deploy-e2e.sh
status "agent_install=manual_script_succeeded"

entry_payload="$TMP_DIR/access-entry.json"
line_name="real-matrix-relay-line-${RUN_ID}"
group_id="$(python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
)"
psql_db -Xq -v ON_ERROR_STOP=1 \
  -v group_id="$group_id" \
  -v group_name="$subscription_group_name" \
  -v endpoint_ids="$endpoint_id_csv" -v user_id="$user_id" <<'SQL'
BEGIN;
INSERT INTO line_groups (id, name, sort_weight, enabled)
VALUES (:'group_id'::uuid, :'group_name', -10000, TRUE)
ON CONFLICT (id) DO NOTHING;
INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
SELECT DISTINCT plan_id, :'group_id'::uuid, 1.000
FROM (
    SELECT id AS plan_id
    FROM plans
    WHERE is_default = TRUE
    ORDER BY created_at ASC
    LIMIT 1
) default_plan
UNION
SELECT DISTINCT plan_id, :'group_id'::uuid, 1.000
FROM user_subscriptions
WHERE user_id = :'user_id'::uuid
ON CONFLICT (plan_id, line_group_id)
DO UPDATE SET billing_multiplier = EXCLUDED.billing_multiplier;
COMMIT;
SQL
client_network_mode="${XRAYC_REAL_MATRIX_RELAY_CLIENT_NETWORK_MODE:-tcp}"
case "$client_network_mode" in
  tcp|udp|xhttp|xudp)
    ;;
  *)
    die "XRAYC_REAL_MATRIX_RELAY_CLIENT_NETWORK_MODE must be tcp, udp, xhttp, or xudp"
    ;;
esac
line_name="${line_name}-${client_network_mode}"
subscription_proxy_name="$line_name"
LINE_NAME_VALUE="$line_name" EXIT_ENDPOINT_ID_VALUE="$initial_endpoint_id" ACCESS_NODE_ID_VALUE="$access_node_id" \
  TRANSIT_PUBLIC_HOST="$PH2" LISTEN_PORT_VALUE="$LISTEN_PORT" CLIENT_NETWORK_MODE_VALUE="$client_network_mode" \
  write_json "$entry_payload" 'mode = os.environ["CLIENT_NETWORK_MODE_VALUE"]; security = "reality" if mode == "tcp" else ""; print(json.dumps({"access_node_id": os.environ["ACCESS_NODE_ID_VALUE"], "name": os.environ["LINE_NAME_VALUE"], "listen_host": os.environ["TRANSIT_PUBLIC_HOST"], "listen_port": int(os.environ["LISTEN_PORT_VALUE"]), "protocol": "vless", "transport": mode, "security": security, "enabled": True, "sort_weight": 100}, ensure_ascii=False))'
entry_body="$TMP_DIR/access-entry-body.json"
api_json POST "/api/admin/access-entries" "$entry_payload" "$entry_body" "$admin_token"
access_entry_id="$(json_value "$entry_body" id)"
[[ -n "$access_entry_id" ]] || die "access entry creation failed"
binding_payload="$TMP_DIR/access-entry-binding.json"
LINE_NAME_VALUE="$line_name" EXIT_ENDPOINT_ID_VALUE="$initial_endpoint_id" \
  write_json "$binding_payload" 'print(json.dumps({"exit_endpoint_id": os.environ["EXIT_ENDPOINT_ID_VALUE"], "name": os.environ["LINE_NAME_VALUE"], "enabled": True, "sort_weight": 100}, ensure_ascii=False))'
binding_body="$TMP_DIR/access-entry-binding-body.json"
api_json POST "/api/admin/access-entries/${access_entry_id}/exit-bindings" "$binding_payload" "$binding_body" "$admin_token"
access_line_id="$(json_value "$binding_body" id)"
pool_id="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v access_line_id="$access_line_id" <<'SQL'
SELECT exit_pool_id::text FROM access_lines WHERE id = :'access_line_id'::uuid LIMIT 1;
SQL
)"
[[ -n "$pool_id" ]] || die "access entry binding did not create runtime pool"
psql_db -Xq -v ON_ERROR_STOP=1 -v pool_id="$pool_id" -v endpoint_ids="$endpoint_id_csv" <<'SQL'
WITH selected AS (
  SELECT unnest(string_to_array(:'endpoint_ids', ',')::uuid[]) AS endpoint_id
)
INSERT INTO exit_pool_members (
  exit_pool_id, exit_endpoint_id, weight, status, priority, allow_new_assignments, updated_at
)
SELECT :'pool_id'::uuid, endpoint_id, 100, 'healthy', 100, TRUE, now()
FROM selected
ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
  weight = EXCLUDED.weight,
  status = EXCLUDED.status,
  priority = EXCLUDED.priority,
  allow_new_assignments = TRUE,
  updated_at = now();
SQL
status "temporary_runtime_members_seeded members=${#protocol_endpoint_ids[@]}"
psql_db -Xq -v ON_ERROR_STOP=1 \
  -v group_id="$group_id" \
  -v group_name="$subscription_group_name" \
  -v access_line_id="$access_line_id" -v access_node_id="$access_node_id" -v user_id="$user_id" <<'SQL'
BEGIN;
INSERT INTO line_groups (id, name, sort_weight, enabled)
VALUES (:'group_id'::uuid, :'group_name', -10000, TRUE)
ON CONFLICT (id) DO NOTHING;
INSERT INTO line_group_binding_nodes (line_group_id, entry_exit_binding_id, position)
VALUES (:'group_id'::uuid, :'access_line_id'::uuid, 100)
ON CONFLICT DO NOTHING;
INSERT INTO plan_line_groups (plan_id, line_group_id, billing_multiplier)
SELECT DISTINCT plan_id, :'group_id'::uuid, 1.000
FROM (
    SELECT id AS plan_id
    FROM plans
    WHERE is_default = TRUE
    ORDER BY created_at ASC
    LIMIT 1
) default_plan
UNION
SELECT DISTINCT plan_id, :'group_id'::uuid, 1.000
FROM user_subscriptions
WHERE user_id = :'user_id'::uuid
ON CONFLICT (plan_id, line_group_id)
DO UPDATE SET billing_multiplier = EXCLUDED.billing_multiplier;
INSERT INTO user_access_line_assignments (user_id, line_group_id, access_line_id, assigned_at)
VALUES (:'user_id'::uuid, :'group_id'::uuid, :'access_line_id'::uuid, now())
ON CONFLICT (user_id, line_group_id, access_line_id)
DO UPDATE SET assigned_at = now();
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_matrix_relay_bind_line_group'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL

wait_access_node_applied_config "initial-relay-binding"
fetch_and_assert_subscription_redaction
extract_subscription_client_env "$subscription_yaml" "$subscription_proxy_name" "$TMP_DIR/subscription-client.env"
# shellcheck disable=SC1090
. "$TMP_DIR/subscription-client.env"
status "subscription_redaction_ok"

ingress_open_successes=0
for _ in $(seq 1 120); do
  if remote_exec 1 "timeout 5 bash -lc '</dev/tcp/${H2}/${LISTEN_PORT}'" >/dev/null 2>&1; then
    ingress_open_successes=$((ingress_open_successes + 1))
    if [[ "$ingress_open_successes" -ge 2 ]]; then
      status "ingress_port_open"
      break
    fi
  else
    ingress_open_successes=0
  fi
  sleep 2
done
[[ "$ingress_open_successes" -ge 2 ]] \
  || { dump_remote_relay_debug; die "ingress port did not open"; }

deploy_client_runtime
status "client_runtime_ready"

for index in "${!protocol_names[@]}"; do
  protocol="${protocol_names[$index]}"
  suffix="${protocol_suffixes[$index]}"
  endpoint_id="${protocol_endpoint_ids[$index]}"
  outbound_type="${protocol_outbound_types[$index]}"
  expected_ip="${protocol_expected_ips[$index]}"

  actual_outbound="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v endpoint_id="$endpoint_id" <<'SQL'
SELECT outbound_type::text FROM exit_endpoints WHERE id = :'endpoint_id'::uuid;
SQL
)"
  [[ "$actual_outbound" == "$outbound_type" ]] || die "protocol=${protocol} endpoint outbound type mismatch"

  before="$(ledger_sum_for_endpoint "$endpoint_id")"
  force_user_exit_assignment "$endpoint_id" "real_matrix_relay_force_${suffix}"
  wait_access_node_applied_config "protocol-${protocol}"
  wait_protocol_route_and_ledger "$protocol" "$endpoint_id" "$expected_ip" "$before"
  assert_protocol_eviction_cycle "$protocol" "$endpoint_id" "$expected_ip"
done

status "completed protocols=${#protocol_names[@]}"
completed_ok=1
