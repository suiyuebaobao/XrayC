#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

SERVER_ACCOUNT_FILE="${XRAYC_SERVER_ACCOUNT_FILE:-}"
PRIVATE_DIR="${XRAYC_PRIVATE_DIR:-${BASE_DIR}/文档/私有}"

usage() {
  cat <<'EOF'
usage: bash scripts/prepare-real-release-env.sh [--template-only] [--hints-only]

Generate a safe real-release env checklist/template. The script never prints
secret values, private file contents, subscription URLs, proxy URLs, passwords,
or tokens.

Options:
  --template-only  Print only the env template.
  --hints-only     Print only local/private-file and Compose Postgres hints.
EOF
}

print_status() {
  printf '%-34s %s\n' "$1" "$2"
}

print_template() {
  cat <<'EOF'
# Real release private env template.
# Save filled values in an untracked private file such as .env.real-release.
# Do not commit real values. Sensitive tokens/credentials must be copied from
# private files or the admin backend; this template intentionally contains only
# variable names and placeholders.
# After filling BASE_URL and login account variables, run
# scripts/prepare-real-e2e-auth-env.sh to write USER_ACCESS_TOKEN/SUB_TOKEN
# into the private env without printing those values.
# Default relay mode uses the current private inventory E2E flow to create the real client
# proxy at runtime. Direct mode requires CLIENT_PROXY_URL* to be filled before
# running the gate.

BASE_URL="https://<real-control-plane-host>"
XRAYC_ENV="production"
SEED_DEMO_DATA="false"
JWT_SECRET="<copy-from-private-kms-or-secret-store>"
JWT_EXPIRES_IN="30m"
JWT_REFRESH_EXPIRES_IN="7d"
REAL_RELEASE_MATRIX_MODE="relay"
# Only for private HTTP E2E before TLS is ready. Public release must use HTTPS.
# XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP="1"
DEPLOY_ARTIFACT_TOKEN="<copy-from-private-file-or-admin-backend>"
SUB_TOKEN="<copy-from-private-file-or-admin-backend>"
# SUBSCRIPTION_URL="https://<real-control-plane-host>/sub/<copy-from-private-file-or-admin-backend>"

AGENT_TOKEN="<copy-from-private-file-or-admin-backend>"
ACCESS_NODE_ID="<copy-from-compose-postgres-id-hints-or-admin-backend>"
ACCESS_LINE_ID="<copy-from-compose-postgres-id-hints-or-admin-backend>"
EXIT_ENDPOINT_ID="<copy-from-compose-postgres-id-hints-or-admin-backend>"
XRAY_USER_KEY="<copy-from-private-file-or-admin-backend>"
XRAYC_REAL_E2E_INVENTORY="<private-inventory-path>"
XRAYC_REAL_E2E_TARGET="<private-target-name>"
CLIENT_PROXY_URL="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP="<copy-from-private-file-or-client-runtime>"
USER_ACCESS_TOKEN="<copy-from-private-file-or-admin-backend>"
# Optional. Written by prepare-real-e2e-auth-env when ADMIN_LOGIN_PASSWORD is set.
# ADMIN_ACCESS_TOKEN="<copy-from-private-file-or-admin-backend>"
EXPECTED_ACCESS_SERVERS="<copy-from-private-file-or-admin-backend>"
DATABASE_URL="postgres://<user>:<password>@<host>:<port>/<database>"
# Optional for local Compose release checks when DATABASE_URL uses a host-only address.
# XRAYC_REAL_RELEASE_COMPOSE_DATABASE_URL="postgres://<user>:<password>@postgres:5432/<database>"
# Only for single-server Docker production checks where PostgreSQL is bound to loopback.
# XRAYC_REAL_RELEASE_ALLOW_LOCAL_DATABASE="1"
E2E_USER_ACCOUNT="<real-user-login-account>"
E2E_USER_PASSWORD="<real-user-login-password>"
E2E_ADMIN_ACCOUNT="<real-admin-login-account>"
E2E_ADMIN_PASSWORD="<real-admin-login-password>"
ADMIN_LOGIN_ACCOUNT="<real-admin-login-account>"
ADMIN_LOGIN_PASSWORD="<real-admin-login-password>"
USER_LOGIN_ACCOUNT="<real-user-login-account>"
USER_LOGIN_PASSWORD="<real-user-login-password>"

THIRD_PARTY_SOCKS_HOST="<upstream-host>"
THIRD_PARTY_SOCKS_PORT="<upstream-port>"
THIRD_PARTY_SOCKS_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_SOCKS_USERNAME="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_SOCKS_PASSWORD="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_SOCKS_KEY="<copy-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_SOCKS="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_SOCKS="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_SOCKS="<copy-from-compose-postgres-id-hints-or-admin-backend>"

THIRD_PARTY_HTTP_HOST="<upstream-host>"
THIRD_PARTY_HTTP_PORT="<upstream-port>"
THIRD_PARTY_HTTP_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_HTTP_USERNAME="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_HTTP_PASSWORD="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_HTTP_KEY="<copy-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_HTTP="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_HTTP="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_HTTP="<copy-from-compose-postgres-id-hints-or-admin-backend>"

THIRD_PARTY_HOST="<vless-upstream-host>"
THIRD_PARTY_VLESS_HOST="<vless-upstream-host>"
THIRD_PARTY_VLESS_PORT="<vless-upstream-port>"
THIRD_PARTY_VLESS_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_UUID="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_PUBLIC_KEY="<copy-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_VLESS="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_VLESS="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_VLESS="<copy-from-compose-postgres-id-hints-or-admin-backend>"

THIRD_PARTY_TROJAN_HOST="<upstream-host>"
THIRD_PARTY_TROJAN_PORT="<upstream-port>"
THIRD_PARTY_TROJAN_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_TROJAN_PASSWORD="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_TROJAN_KEY="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_TROJAN_SECURITY="tls"
THIRD_PARTY_TROJAN_SNI="<sni-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_TROJAN="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_TROJAN="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_TROJAN="<copy-from-compose-postgres-id-hints-or-admin-backend>"

THIRD_PARTY_SHADOWSOCKS_HOST="<upstream-host>"
THIRD_PARTY_SHADOWSOCKS_PORT="<upstream-port>"
THIRD_PARTY_SHADOWSOCKS_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_SHADOWSOCKS_PASSWORD="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_SHADOWSOCKS_KEY="<copy-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_SHADOWSOCKS="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_SHADOWSOCKS="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_SHADOWSOCKS="<copy-from-compose-postgres-id-hints-or-admin-backend>"

THIRD_PARTY_HY2_HOST="<upstream-host>"
THIRD_PARTY_HY2_PORT="<upstream-port>"
THIRD_PARTY_HY2_RAW_URL="<copy-from-private-file-or-provider-console>"
THIRD_PARTY_HY2_KEY="<copy-from-private-file-or-provider-console>"
CLIENT_PROXY_URL_HY2="<copy-from-private-file-or-client-runtime>"
EXPECTED_EXIT_IP_HY2="<copy-from-private-file-or-client-runtime>"
EXIT_ENDPOINT_ID_HY2="<copy-from-compose-postgres-id-hints-or-admin-backend>"

REAL_RUNTIME_OBSERVATION_TIMEOUT_SECONDS="180"
REAL_RUNTIME_OBSERVATION_POLL_INTERVAL_SECONDS="5"

# Required real user inbound protocol matrix gate. Uses XRAYC_REAL_E2E_INVENTORY.
# Current private role convention: server_1 is direct/non-CF for ordinary
# client-side checks; server_4 is Cloudflare/orange-cloud for CDN entry checks.
# The ordinary high-port matrix rejects server_4; Cloudflare entries must use
# the dedicated 443 TLS WebSocket/gRPC validation path.
# Verifies subscription Xray VLESS/Trojan/Shadowsocks entries by sending traffic
# through those user-side inbounds.
RUN_REAL_ACCESS_INBOUND_MATRIX_E2E="1"
RUN_REAL_RUNTIME_RESTORE_DEPLOY="1"
REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS="vless,trojan,shadowsocks"
# Optional real-debug tuning. Defaults are 3 remote retries and 20 traffic
# attempts per protocol; keep defaults for release gates, lower them only when
# collecting a fast timeout diagnostic.
REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES="3"
REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_ATTEMPTS="20"
# Ordinary inbound matrix ignores THIRD_PARTY_SOCKS_* by default. Set this only
# when intentionally creating a temporary SOCKS upstream for the matrix.
REAL_ACCESS_INBOUND_MATRIX_ENABLE_THIRD_PARTY_SOCKS="0"
# REAL_ACCESS_INBOUND_MATRIX_EXIT_ENDPOINT_ID="<explicit-healthy-endpoint-id>"
# Optional strict egress assertions for the inbound matrix. Leave commented to
# require traffic success without printing or comparing public IP values.
# REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_VLESS="<copy-from-private-client-observation>"
# REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_TROJAN="<copy-from-private-client-observation>"
# REAL_ACCESS_INBOUND_MATRIX_EXPECTED_EXIT_IP_SHADOWSOCKS="<copy-from-private-client-observation>"

# Required 10m long-stability UAT gate for real release.
RUN_REAL_STABILITY_UAT="1"
RUN_REAL_STABILITY_UAT_24H="0"
UAT_PROFILE="10m"
UAT_DURATION_SECONDS="600"
UAT_ACCESS_NODE_IDS="<comma-separated-real-access-node-ids>"
UAT_ACCESS_LINE_IDS="<comma-separated-real-access-line-ids>"
UAT_EXIT_ENDPOINT_IDS="<comma-separated-real-exit-endpoint-ids>"
UAT_CLIENT_PROXY_URLS="<comma-separated-real-client-proxy-urls>"
UAT_EXPECTED_EXIT_IPS="<comma-separated-real-egress-ips>"
UAT_USER_ACCESS_TOKENS="<comma-separated-real-user-access-tokens>"

# Required access-agent traffic backlog recovery UAT gate for real release.
# The UAT validates env/remote access with BACKLOG_VALIDATE_ONLY=1, then the
# full gate blocks control-plane connectivity, persists backlog, restarts the
# agent, restores connectivity, and verifies ledger replay.
RUN_ACCESS_TRAFFIC_BACKLOG_UAT="1"
BACKLOG_TARGET_INDEX="2"
BACKLOG_INSTALL_DIR="/opt/xrayc-real-relay"
BACKLOG_WAIT_SECONDS="180"
BACKLOG_TRAFFIC_ROUNDS="2"
# Manual preflight only. Do not set for the full real release gate.
# BACKLOG_VALIDATE_ONLY="1"
# BACKLOG_SKIP_AGENT_RESTART="0"

# Required large runtime loadtest gate. Use an isolated PostgreSQL database,
# never the production release database.
RUN_RUNTIME_LOADTEST="1"
RUNTIME_LOADTEST_DATABASE_URL="postgres://<user>:<password>@<host>:<port>/<isolated-loadtest-database>"
RUNTIME_LOADTEST_PROFILE="large"

# Required isolated HTTP/API runtime loadtest gate. It starts temporary
# PostgreSQL + API Compose with demo seed data and does not write production DB.
RUNTIME_HTTP_LOADTEST_PROFILE="large"
RUNTIME_HTTP_LOADTEST_REQUIRE_FULL="1"

# Required multi-replica auth and operator recovery UAT gates.
RUN_AUTH_HA_UAT="1"
RUN_AUTH_HA_STABILITY_UAT="1"
AUTH_HA_STABILITY_PROFILE="10m"
AUTH_HA_STABILITY_DURATION_SECONDS="600"
RUN_OPS_MISTAKE_RECOVERY_UAT="1"
RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT="1"
OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL="postgres://<user>:<password>@<host>:<port>/<isolated-loadtest-database>"

# Required multi-user real traffic UAT gate. Defaults match the full release
# requirement: 10 real client containers, 40 minutes per round, 2 rounds.
RUN_REAL_MULTI_USER_TRAFFIC_UAT="1"
XRAYC_REAL_MULTI_USER_COUNT="10"
XRAYC_REAL_MULTI_USER_DURATION_SECONDS="2400"
XRAYC_REAL_MULTI_USER_ROUNDS="2"
XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES="3"

# Required real subscription client compatibility gate.
RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT="1"
RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC="1"
XRAYC_MIHOMO_IMAGE="metacubex/mihomo:latest"
EOF
}

docker_compose_available() {
  command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1
}

compose_postgres_running() {
  docker compose ps --status running --services 2>/dev/null | grep -Fxq postgres
}

run_psql() {
  docker compose exec -T postgres \
    psql -U "${POSTGRES_USER:-xrayc}" -d "${POSTGRES_DB:-xrayc}" -XAtq -v ON_ERROR_STOP=1 "$@"
}

print_private_coverage() {
  echo "# Private source coverage"
  if [[ -n "$SERVER_ACCOUNT_FILE" && -e "$SERVER_ACCOUNT_FILE" ]]; then
    print_status "server account file" "configured and present"
  elif [[ -n "$SERVER_ACCOUNT_FILE" ]]; then
    print_status "server account file" "configured but missing"
  else
    print_status "server account file" "not configured"
  fi

  if [[ -d "$PRIVATE_DIR" ]]; then
    local private_file_count
    private_file_count="$(find "$PRIVATE_DIR" -mindepth 1 -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d '[:space:]')"
    if [[ "$private_file_count" -gt 0 ]]; then
      print_status "private docs directory" "present (${private_file_count} file(s))"
    else
      print_status "private docs directory" "present (0 file(s))"
    fi
  else
    print_status "private docs directory" "missing"
  fi
  echo
}

print_compose_postgres_hints() {
  echo "# Compose Postgres non-sensitive hints"
  if ! docker_compose_available; then
    print_status "compose postgres" "skipped (docker compose unavailable)"
    echo
    return
  fi

  if ! compose_postgres_running; then
    print_status "compose postgres" "skipped (postgres service not running)"
    echo
    return
  fi

  print_status "compose postgres" "running"
  if ! run_psql <<'SQL'
WITH table_counts AS (
  SELECT 'access_nodes' AS name, COUNT(*)::text AS value FROM access_nodes
  UNION ALL SELECT 'access_lines', COUNT(*)::text FROM access_lines
  UNION ALL SELECT 'exit_endpoints', COUNT(*)::text FROM exit_endpoints
  UNION ALL SELECT 'subscription_tokens', COUNT(*)::text FROM subscription_tokens
  UNION ALL SELECT 'users', COUNT(*)::text FROM users
  UNION ALL SELECT 'user_subscriptions', COUNT(*)::text FROM user_subscriptions
), runtime_supported_endpoints AS (
  SELECT e.id, e.outbound_type, e.created_at
  FROM exit_endpoints e
  WHERE e.enabled = TRUE
    AND (
      trim(COALESCE(e.host, '')) <> ''
      AND e.port IS NOT NULL
      AND e.port > 0
      AND (
        e.outbound_type IN ('socks', 'http')
        OR (e.outbound_type = 'vless' AND trim(COALESCE(e.outbound_config->>'uuid', e.outbound_config->>'id', '')) <> '')
        OR (e.outbound_type = 'trojan' AND trim(COALESCE(e.outbound_config->>'password', '')) <> '')
        OR (
          e.outbound_type = 'shadowsocks'
          AND trim(COALESCE(e.outbound_config->>'method', e.outbound_config->>'cipher', '')) <> ''
          AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
        )
        OR (e.outbound_type = 'hysteria' AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'auth', '')) <> '')
      )
    )
), runtime_ready_lines AS (
  SELECT al.id
  FROM access_lines al
  JOIN exit_pool_members epm ON epm.exit_pool_id = al.exit_pool_id
  JOIN runtime_supported_endpoints rse ON rse.id = epm.exit_endpoint_id
  WHERE al.enabled = TRUE
    AND (al.exit_endpoint_id IS NULL OR rse.id = al.exit_endpoint_id)
    AND COALESCE(epm.status, 'healthy') IN ('healthy', 'unknown', '')
    AND COALESCE(epm.allow_new_assignments, TRUE) = TRUE
  GROUP BY al.id
), subscription_access_lines AS (
  SELECT DISTINCT al.id AS access_line_id
  FROM user_subscriptions us
  JOIN plan_line_groups plg ON plg.plan_id = us.plan_id
  JOIN access_lines al ON (
    (
      al.exit_endpoint_id IS NOT NULL
      AND EXISTS (
        SELECT 1
        FROM line_group_exit_endpoints lgee
        WHERE lgee.line_group_id = plg.line_group_id
          AND lgee.exit_endpoint_id = al.exit_endpoint_id
      )
    )
    OR (
      al.exit_endpoint_id IS NULL
      AND al.line_group_id = plg.line_group_id
    )
  )
  WHERE us.active = TRUE
    AND (us.expires_at IS NULL OR us.expires_at > now())
    AND al.enabled = TRUE
)
SELECT line
FROM (
  SELECT 10 AS sort_key, name AS sub_key, format('%-34s %s', name || ' count', value) AS line
  FROM table_counts
  UNION ALL
  SELECT 20, 'runtime-ready access lines', format('%-34s %s', 'runtime-ready access lines', COUNT(*)::text)
  FROM runtime_ready_lines
  UNION ALL
  SELECT 21, 'subscription runtime-ready lines', format('%-34s %s', 'subscription runtime-ready lines', COUNT(*)::text)
  FROM runtime_ready_lines rrl
  JOIN subscription_access_lines sal ON sal.access_line_id = rrl.id
  UNION ALL
  SELECT 30, id::text, format('%-34s %s', 'ACCESS_NODE_ID candidate', id::text)
  FROM (
    SELECT id, created_at FROM access_nodes ORDER BY created_at DESC LIMIT 5
  ) nodes
  UNION ALL
  SELECT 40, id::text, format('%-34s %s', 'ACCESS_LINE_ID candidate', id::text)
  FROM (
    SELECT id FROM runtime_ready_lines ORDER BY id::text LIMIT 5
  ) lines
  UNION ALL
  SELECT 50, id::text, format('%-34s %s', 'EXIT_ENDPOINT_ID candidate', id::text)
  FROM (
    SELECT id, created_at FROM runtime_supported_endpoints ORDER BY created_at DESC LIMIT 10
  ) endpoints
  UNION ALL
  SELECT 60, outbound_type::text, format('%-34s %s=%s', 'exit endpoint protocol count', outbound_type::text, COUNT(*)::text)
  FROM exit_endpoints
  GROUP BY outbound_type
  UNION ALL
  SELECT 70, outbound_type::text, format('%-34s %s=%s', 'runtime-supported protocol count', outbound_type::text, COUNT(*)::text)
  FROM runtime_supported_endpoints
  GROUP BY outbound_type
) hints
ORDER BY sort_key, sub_key;
SQL
  then
    print_status "compose postgres hints" "skipped (query failed)"
    echo
    return
  fi
  echo
  cat <<'EOF'
# Sensitive values intentionally not printed:
# DEPLOY_ARTIFACT_TOKEN, SUB_TOKEN/SUBSCRIPTION_URL, AGENT_TOKEN,
# USER_ACCESS_TOKEN, XRAY_USER_KEY, DATABASE_URL password, private inventory
# contents, third-party raw URLs, third-party usernames/passwords/keys,
# CLIENT_PROXY_URL*, EXPECTED_EXIT_IP*, EXPECTED_ACCESS_SERVERS.
# Copy these from private files or the admin/provider backend.
# EXPECTED_EXIT_IP* can also be written by scripts/observe-real-client-env.sh
# after CLIENT_PROXY_URL* values are filled from a real client.
EOF
  echo
}

template=1
hints=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --template-only)
      template=1
      hints=0
      ;;
    --hints-only)
      template=0
      hints=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if [[ "$template" == "1" ]]; then
  print_template
  echo
fi

if [[ "$hints" == "1" ]]; then
  print_private_coverage
  print_compose_postgres_hints
fi
