#!/usr/bin/env bash
# 此脚本是真实三节点接入完整性 E2E 的主入口。
# 它负责加载私有环境、初始化全局变量，并编排 helper 中的校验流程。
set -euo pipefail
IFS=$'\n\t'

if [[ $- == *x* ]]; then
  set +x
  echo "real-access-3node-integrity-e2e: disabled shell xtrace to avoid printing private values" >&2
fi

umask 077

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://control-plane.example" \
  DATABASE_URL="postgresql://..." \
  SUB_TOKEN="<private-subscription-token>" \
  USER_ACCESS_TOKEN="<private-user-jwt>" \
  CLIENT_PROXY_URL="socks5h://127.0.0.1:7890" \
  EXPECTED_EXIT_IPS="<expected-public-egress-ip>[,<backup-ip>]" \
  EXPECTED_ACCESS_SERVERS="<relay-host:port>" \
  XRAYC_REAL_E2E_INVENTORY="/private/remote-inventory.json" \
  XRAYC_REAL_E2E_TARGET="<relay-alias>" \
  XRAYC_REAL_3NODE_ALLOW_MUTATION=1 \
  bash scripts/real-access-3node-integrity-e2e.sh

Purpose:
  Verify one real client, one relay/access node, and at least one assignable
  exit-pool member. The script rejects placeholder/mock inputs, verifies the
  relay Docker deployment, confirms the subscription exposes only relay ingress
  addresses, sends real client traffic through CLIENT_PROXY_URL, verifies exit
  pool assignment and access_line ledger billing, then temporarily disables the
  test user and exhausts the unified quota to confirm eviction.

Required:
  BASE_URL
  DATABASE_URL
  SUB_TOKEN or SUBSCRIPTION_URL, or USER_LOGIN_ACCOUNT + USER_LOGIN_PASSWORD
  USER_ACCESS_TOKEN, or USER_LOGIN_ACCOUNT + USER_LOGIN_PASSWORD
  CLIENT_PROXY_URL
  EXPECTED_EXIT_IP or EXPECTED_EXIT_IPS
  EXPECTED_ACCESS_SERVERS
  XRAYC_REAL_E2E_INVENTORY + XRAYC_REAL_E2E_TARGET,
    or XRAYC_REAL_3NODE_ACCESS_SSH_HOST + XRAYC_REAL_3NODE_ACCESS_SSH_USER
  XRAYC_REAL_3NODE_ALLOW_MUTATION=1

Optional:
  ACCESS_LINE_ID, ACCESS_NODE_ID, EXIT_ENDPOINT_ID
  USER_LOGIN_ACCOUNT, USER_LOGIN_PASSWORD
  XRAYC_REAL_3NODE_ACCESS_SSH_HOST, XRAYC_REAL_3NODE_ACCESS_SSH_USER
  XRAYC_REAL_3NODE_ACCESS_SSH_PORT, XRAYC_REAL_3NODE_ACCESS_SSH_PASSWORD_FILE
  XRAYC_REAL_3NODE_ACCESS_SSH_IDENTITY_FILE
  XRAYC_REAL_3NODE_ACCESS_INSTALL_DIR, XRAYC_REAL_3NODE_EXPECTED_LISTEN_PORTS
  PUBLIC_IP_URL                    Default: https://api.ipify.org
  REAL_TRAFFIC_URL                 Default: PUBLIC_IP_URL
  XRAYC_REAL_RELEASE_ENV_FILE      Private env file to source, default .env.real-release
  XRAYC_REAL_3NODE_LOAD_ENV_FILE=0 Disable private env loading
  XRAYC_REAL_3NODE_RUN_DEPLOY=1    Run real-remote-access-deploy-e2e first
  XRAYC_REAL_3NODE_ALLOW_LOCAL_BASE_URL=1
  XRAYC_REAL_3NODE_ALLOW_INSECURE_HTTP=1
  XRAYC_REAL_3NODE_ALLOW_TEST_DOMAINS=1
  XRAYC_REAL_3NODE_TRAFFIC_ATTEMPTS, XRAYC_REAL_3NODE_TRAFFIC_INTERVAL_SECONDS
  XRAYC_REAL_3NODE_EVICTION_POLL_SECONDS, XRAYC_REAL_3NODE_EVICTION_POLL_INTERVAL_SECONDS

The script prints only coarse progress and assertion results. It does not print
passwords, bearer tokens, subscription URLs, proxy URLs, database URLs, SSH
hosts, or public egress IP values.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"
SCRIPT_DIR="${BASE_DIR}/scripts"
LIB_DIR="${SCRIPT_DIR}/lib/real-access-3node-integrity"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ "${XRAYC_REAL_3NODE_LOAD_ENV_FILE:-1}" == "1" && -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"

BASE_URL="${BASE_URL:-}"
DATABASE_URL="${DATABASE_URL:-}"
SUB_TOKEN="${SUB_TOKEN:-}"
SUBSCRIPTION_URL="${SUBSCRIPTION_URL:-}"
USER_ACCESS_TOKEN="${USER_ACCESS_TOKEN:-}"
USER_LOGIN_ACCOUNT="${USER_LOGIN_ACCOUNT:-}"
USER_LOGIN_PASSWORD="${USER_LOGIN_PASSWORD:-}"
CLIENT_PROXY_URL="${CLIENT_PROXY_URL:-}"
EXPECTED_EXIT_IP="${EXPECTED_EXIT_IP:-}"
EXPECTED_EXIT_IPS="${EXPECTED_EXIT_IPS:-${EXPECTED_EXIT_IP}}"
EXPECTED_ACCESS_SERVERS="${EXPECTED_ACCESS_SERVERS:-}"
EXPECTED_CLIENT_PROXY_ENDPOINTS="${EXPECTED_CLIENT_PROXY_ENDPOINTS:-}"
REQUIRE_CLIENT_PROXY_ENDPOINT_CHECK="${XRAYC_REAL_3NODE_REQUIRE_CLIENT_PROXY_ENDPOINT_CHECK:-1}"
PUBLIC_IP_URL="${PUBLIC_IP_URL:-https://api.ipify.org}"
REAL_TRAFFIC_URL="${REAL_TRAFFIC_URL:-$PUBLIC_IP_URL}"
ACCESS_LINE_ID="${ACCESS_LINE_ID:-}"
ACCESS_NODE_ID="${ACCESS_NODE_ID:-}"
EXIT_ENDPOINT_ID="${EXIT_ENDPOINT_ID:-}"
XRAY_USER_KEY="${XRAY_USER_KEY:-}"
INVENTORY="${XRAYC_REAL_E2E_INVENTORY:-}"
ACCESS_TARGET="${XRAYC_REAL_3NODE_ACCESS_TARGET:-${XRAYC_REAL_E2E_TARGET:-}}"
DIRECT_ACCESS_SSH_HOST="${XRAYC_REAL_3NODE_ACCESS_SSH_HOST:-}"
DIRECT_ACCESS_SSH_USER="${XRAYC_REAL_3NODE_ACCESS_SSH_USER:-}"
DIRECT_ACCESS_SSH_PORT="${XRAYC_REAL_3NODE_ACCESS_SSH_PORT:-22}"
DIRECT_ACCESS_SSH_PASSWORD_FILE="${XRAYC_REAL_3NODE_ACCESS_SSH_PASSWORD_FILE:-}"
DIRECT_ACCESS_SSH_IDENTITY_FILE="${XRAYC_REAL_3NODE_ACCESS_SSH_IDENTITY_FILE:-}"
DIRECT_ACCESS_INSTALL_DIR="${XRAYC_REAL_3NODE_ACCESS_INSTALL_DIR:-/opt/xrayc/access-agent}"
DIRECT_ACCESS_COMPOSE_PROJECT="${XRAYC_REAL_3NODE_ACCESS_COMPOSE_PROJECT:-xrayc-access}"
DIRECT_ACCESS_EXPECTED_LISTEN_PORTS="${XRAYC_REAL_3NODE_EXPECTED_LISTEN_PORTS:-}"
ALLOW_MUTATION="${XRAYC_REAL_3NODE_ALLOW_MUTATION:-0}"
RUN_REMOTE_DEPLOY="${XRAYC_REAL_3NODE_RUN_DEPLOY:-0}"
TRAFFIC_ATTEMPTS="${XRAYC_REAL_3NODE_TRAFFIC_ATTEMPTS:-3}"
TRAFFIC_INTERVAL_SECONDS="${XRAYC_REAL_3NODE_TRAFFIC_INTERVAL_SECONDS:-2}"
EVICTION_POLL_SECONDS="${XRAYC_REAL_3NODE_EVICTION_POLL_SECONDS:-120}"
EVICTION_POLL_INTERVAL_SECONDS="${XRAYC_REAL_3NODE_EVICTION_POLL_INTERVAL_SECONDS:-5}"
MIN_EXIT_MEMBERS="${XRAYC_REAL_3NODE_MIN_EXIT_MEMBERS:-1}"
MIN_RELAY_COUNT="${XRAYC_REAL_3NODE_MIN_RELAY_COUNT:-1}"
CURL_TIMEOUT="${CURL_TIMEOUT:-30}"

tmp_dir="$(mktemp -d)"
MUTATION_STATE_CAPTURED=0
RESTORE_ON_EXIT=0
TARGET_USER_ID=""
TARGET_ACCESS_LINE_ID=""
TARGET_ACCESS_NODE_ID=""
TARGET_EXIT_POOL_ID=""
EXIT_POOL_HOSTS_FILE=""
TARGET_STATS_EMAIL=""
ORIGINAL_ASSIGNMENT_EXIT_ENDPOINT_ID=""
ORIGINAL_ASSIGNMENT_FAILOVER_REASON=""
ORIGINAL_USER_DISABLED=""
ORIGINAL_SUBSCRIPTION_ACTIVE=""
ORIGINAL_SUBSCRIPTION_EXPIRES_AT=""
ORIGINAL_USED_BYTES=""
ORIGINAL_LIMIT_BYTES=""

# shellcheck source=scripts/lib/real-access-3node-integrity/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/real-access-3node-integrity/subscription-traffic.sh
source "${LIB_DIR}/subscription-traffic.sh"
# shellcheck source=scripts/lib/real-access-3node-integrity/db-mutation.sh
source "${LIB_DIR}/db-mutation.sh"
# shellcheck source=scripts/lib/real-access-3node-integrity/remote.sh
source "${LIB_DIR}/remote.sh"

trap cleanup EXIT

echo "Preparing real 3-node integrity context"
validate_required_inputs
login_user_if_needed
discover_subscription_if_needed
validate_auth_inputs

subscription_file="$tmp_dir/subscription.yaml"
subscription_url="$(subscription_download_url)"
xrayc_real_e2e_fetch_url_to_file "$subscription_url" "$subscription_file" "subscription download failed"
assert_subscription_shape "$subscription_file"

sub_token_hash="$(sha256_literal "$SUB_TOKEN")"
discover_access_line_id_if_needed
discover_db_context "$sub_token_hash"
assert_subscription_hides_exit_hosts "$subscription_file"
if xrayc_real_e2e_bool_is_true "$REQUIRE_CLIENT_PROXY_ENDPOINT_CHECK" \
  && [[ -z "$EXPECTED_CLIENT_PROXY_ENDPOINTS" ]]; then
  fail "EXPECTED_CLIENT_PROXY_ENDPOINTS is required for real 3node client proxy validation"
fi
xrayc_real_e2e_assert_client_proxy_endpoint "$CLIENT_PROXY_URL"

setup_ssh
run_remote_deploy_if_requested
check_remote_docker_deployment
wait_remote_user_present

echo "Running control-plane smoke without synthetic agent posts"
BASE_URL="$BASE_URL" \
SUB_TOKEN="$SUB_TOKEN" \
SUBSCRIPTION_URL="$SUBSCRIPTION_URL" \
REQUIRE_SUBSCRIPTION_DOWNLOAD=1 \
DISABLE_SYNTHETIC_AGENT_POSTS=1 \
bash scripts/real-smoke.sh

billing_before="$(xrayc_real_e2e_capture_billing_baseline "$BASE_URL" "$USER_ACCESS_TOKEN" "$tmp_dir")"
ledger_before="$(xrayc_real_e2e_capture_ledger_baseline "$TARGET_ACCESS_LINE_ID" "$ASSIGNED_EXIT_ENDPOINT_ID" "$ASSIGNED_OUTBOUND_TYPE")"
send_real_client_traffic
xrayc_real_e2e_wait_billing_increase "$BASE_URL" "$USER_ACCESS_TOKEN" "$billing_before" "$tmp_dir"
xrayc_real_e2e_wait_ledger_source_increase "$ledger_before" "$TARGET_ACCESS_LINE_ID" "$ASSIGNED_EXIT_ENDPOINT_ID" "$ASSIGNED_OUTBOUND_TYPE"

capture_mutation_state

echo "Checking disabled-user eviction"
mutate_user_disabled
wait_subscription_rejected "disabled-user"
wait_remote_user_absent "disabled-user"
wait_client_proxy_blocked "disabled-user"
restore_account_state
wait_subscription_target_present
wait_remote_user_present

echo "Checking over-quota eviction"
mutate_subscription_quota_exhausted
wait_subscription_target_absent "quota-exhausted"
wait_remote_user_absent "quota-exhausted"
wait_client_proxy_blocked "quota-exhausted"
restore_account_state
wait_subscription_target_present
wait_remote_user_present

echo "real 3-node access integrity e2e completed"
