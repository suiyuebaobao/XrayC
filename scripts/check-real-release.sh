#!/usr/bin/env bash
# 中文说明：真实发布门禁入口，负责加载环境并串联真实 UAT 检查。
# 中文说明：环境清单和变量校验拆到 helper；完整发布门禁只接受 relay 矩阵。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

# shellcheck source=real-e2e-lib.sh
source "${SCRIPT_DIR}/real-e2e-lib.sh"
# shellcheck source=lib/check-real-release/env.sh
source "${SCRIPT_DIR}/lib/check-real-release/env.sh"

step() {
  printf 'gate: %s\n' "$1"
}

reload_real_release_env() {
  if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
    set -a
    # shellcheck disable=SC1090
    . "$REAL_RELEASE_ENV_FILE"
    set +a
  fi
}

usage() {
  cat <<'EOF'
usage: bash scripts/check-real-release.sh [--print-required-env]

Options:
  --print-required-env  Print the private environment variables required by the real release gate.

The script automatically loads .env.real-release when it exists. Set
XRAYC_REAL_RELEASE_ENV_FILE to use another private env file.
EOF
}

if [[ "${1:-}" == "--print-required-env" ]]; then
  print_required_env
  exit 0
fi

if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
reload_real_release_env

GATE_STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
compat_prepare_manifest="$(mktemp)"

cleanup_real_release_compat_prepare() {
  if [[ -f "$compat_prepare_manifest" ]]; then
    bash scripts/real-access-inbound-matrix-db-prepare.sh cleanup "$compat_prepare_manifest" >/dev/null 2>&1 || true
    rm -f "$compat_prepare_manifest" "$compat_prepare_manifest".*
  fi
}

trap cleanup_real_release_compat_prepare EXIT

step "release-env"
bash scripts/validate-real-release-env.sh
REAL_RELEASE_MATRIX_MODE="${REAL_RELEASE_MATRIX_MODE:-relay}"
case "$REAL_RELEASE_MATRIX_MODE" in
  relay) ;;
  direct)
    echo "REAL_RELEASE_MATRIX_MODE=direct is diagnostic only; full real release gate requires relay mode for disable/quota eviction coverage" >&2
    config_error
    ;;
  *)
    echo "REAL_RELEASE_MATRIX_MODE must be relay for full real release gate" >&2
    config_error
    ;;
esac
REAL_PROTOCOL_MATRIX_PROTOCOLS=socks,http,vless,trojan,shadowsocks,hy2 \
bash scripts/check-real-protocol-matrix-env.sh --env-file "$REAL_RELEASE_ENV_FILE" --mode "$REAL_RELEASE_MATRIX_MODE" --allow-missing-endpoints
step "real-test-server-assets"
bash scripts/check-real-test-servers.sh
require_env XRAYC_ENV
require_env SEED_DEMO_DATA
require_env JWT_SECRET
require_env JWT_EXPIRES_IN
require_env JWT_REFRESH_EXPIRES_IN
require_env BASE_URL
require_env DEPLOY_ARTIFACT_TOKEN
require_any_env SUB_TOKEN SUBSCRIPTION_URL
require_env XRAYC_REAL_E2E_INVENTORY
require_env XRAYC_REAL_E2E_TARGET
require_env USER_ACCESS_TOKEN
require_env DATABASE_URL
require_env E2E_USER_ACCOUNT
require_env E2E_USER_PASSWORD
require_env E2E_ADMIN_ACCOUNT
require_env E2E_ADMIN_PASSWORD
require_gate_enabled RUN_REAL_ACCESS_INBOUND_MATRIX_E2E
require_gate_enabled RUN_REAL_RUNTIME_RESTORE_DEPLOY
require_gate_enabled RUN_REAL_STABILITY_UAT
require_gate_enabled RUN_ACCESS_TRAFFIC_BACKLOG_UAT
require_gate_enabled RUN_RUNTIME_LOADTEST
require_gate_enabled RUN_AUTH_HA_UAT
require_gate_enabled RUN_AUTH_HA_STABILITY_UAT
require_gate_enabled RUN_OPS_MISTAKE_RECOVERY_UAT
require_gate_enabled RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT
require_gate_enabled RUN_REAL_MULTI_USER_TRAFFIC_UAT
require_gate_enabled RUN_REAL_SUBSCRIPTION_CLIENT_COMPAT
RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC="${RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC:-1}"
require_gate_enabled RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC
require_env ADMIN_LOGIN_ACCOUNT
require_env ADMIN_LOGIN_PASSWORD
require_matrix_protocol_env

step "running-runtime-current"
bash scripts/check-running-images-current.sh

step "real-e2e-auth-accounts"
bash scripts/check-real-e2e-auth-accounts.sh

step "real-smoke"
DEPLOY_ARTIFACT_FULL=1 \
ALLOW_INSECURE_HTTP_E2E="${ALLOW_INSECURE_HTTP_E2E:-${XRAYC_REMOTE_E2E_ALLOW_INSECURE_HTTP:-0}}" \
REQUIRE_SUBSCRIPTION_DOWNLOAD=0 \
DISABLE_SYNTHETIC_AGENT_POSTS=1 \
bash scripts/real-smoke.sh >/dev/null 2>&1

step "real-subscription-client-compat"
REAL_SUBSCRIPTION_CLIENT_COMPAT_PREPARE_MANIFEST="$compat_prepare_manifest" \
REAL_SUBSCRIPTION_CLIENT_COMPAT_KEEP_PREPARE=1 \
bash scripts/check-real-subscription-client-compat.sh >/dev/null 2>&1

step "real-playwright-no-mock"
playwright_report="$(mktemp)"
(
  cd frontend
  XRAYC_REAL_RELEASE=1 \
  E2E_REQUIRE_NO_SKIP=1 \
  E2E_BASE_URL="$BASE_URL" \
  E2E_USER_ACCOUNT="$E2E_USER_ACCOUNT" \
  E2E_USER_PASSWORD="$E2E_USER_PASSWORD" \
  E2E_ADMIN_ACCOUNT="$E2E_ADMIN_ACCOUNT" \
  E2E_ADMIN_PASSWORD="$E2E_ADMIN_PASSWORD" \
  npx playwright test e2e/runtime-no-mock.spec.ts --reporter=json >"$playwright_report" 2>/dev/null
)
python3 - "$playwright_report" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    report = json.load(fh)
skipped = int(report.get("stats", {}).get("skipped", 0) or 0)
if skipped:
    raise SystemExit("real Playwright no-mock skipped tests")
PY
rm -f "$playwright_report"

step "real-worker-smoke"
WORKER_SMOKE_BUILD=0 \
bash scripts/real-worker-smoke.sh >/dev/null 2>&1

step "agent-install-cleanup-contract"
bash scripts/check-agent-install-cleanup.sh >/dev/null 2>&1

step "real-agent-first-nodes-e2e"
bash scripts/real-agent-first-nodes-e2e.sh >/dev/null 2>&1

step "real-access-inbound-matrix-e2e"
REAL_ACCESS_INBOUND_MATRIX_FORCE_PREPARE=1 \
REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS=vless,trojan,shadowsocks \
bash scripts/real-access-inbound-matrix-e2e.sh >/dev/null 2>&1

step "prepare-real-protocol-matrix-endpoints"
bash scripts/prepare-real-protocol-matrix-endpoints.sh --env-file "$REAL_RELEASE_ENV_FILE" --protocols socks,http,vless,trojan,shadowsocks,hy2 >/dev/null 2>&1
reload_real_release_env

step "real-protocol-matrix-env-after-prepare"
REAL_PROTOCOL_MATRIX_PROTOCOLS=socks,http,vless,trojan,shadowsocks,hy2 \
bash scripts/check-real-protocol-matrix-env.sh --env-file "$REAL_RELEASE_ENV_FILE" --mode "$REAL_RELEASE_MATRIX_MODE"

step "real-access-third-party-matrix-relay-e2e"
XRAYC_REAL_RELEASE=1 \
XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE=1 \
REQUIRE_LEDGER_SOURCE_CHECK=true \
DISABLE_SYNTHETIC_AGENT_POSTS=1 \
REQUIRE_SUBSCRIPTION_DOWNLOAD=1 \
bash scripts/real-access-third-party-matrix-relay-e2e.sh >/dev/null 2>&1

step "real-v2-relay-pool-e2e"
XRAYC_REAL_RELEASE=1 \
XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE=1 \
XRAYC_REAL_E2E_ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}" \
XRAYC_REAL_E2E_ADMIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}" \
XRAYC_REAL_E2E_USER_ACCOUNT="${USER_LOGIN_ACCOUNT:-${E2E_USER_ACCOUNT:-}}" \
XRAYC_REAL_E2E_USER_PASSWORD="${USER_LOGIN_PASSWORD:-${E2E_USER_PASSWORD:-}}" \
bash scripts/real-v2-relay-pool-e2e.sh >/dev/null 2>&1

step "restore-real-runtime-access-deploy"
bash scripts/restore-real-runtime-access-deploy.sh >/dev/null 2>&1
reload_real_release_env

step "real-runtime-stability-uat"
XRAYC_REAL_RUNTIME_FORCE_VALIDATE_ONLY=0 \
XRAYC_REAL_RUNTIME_FORCE_PROFILE=10m \
XRAYC_REAL_RUNTIME_FORCE_DURATION_SECONDS="${UAT_10M_DURATION_SECONDS:-600}" \
bash scripts/real-runtime-stability-uat.sh >/dev/null 2>&1

step "real-access-traffic-backlog-uat"
BACKLOG_VALIDATE_ONLY=0 \
BACKLOG_SKIP_AGENT_RESTART=0 \
BACKLOG_WAIT_SECONDS=300 \
bash scripts/real-access-traffic-backlog-uat.sh >/dev/null 2>&1

if xrayc_real_e2e_bool_is_true "${RUN_REAL_STABILITY_UAT_24H:-0}"; then
  step "real-runtime-stability-uat-24h"
  XRAYC_REAL_RUNTIME_FORCE_VALIDATE_ONLY=0 \
  XRAYC_REAL_RUNTIME_FORCE_PROFILE=24h \
  XRAYC_REAL_RUNTIME_FORCE_DURATION_SECONDS="${UAT_24H_DURATION_SECONDS:-86400}" \
  bash scripts/real-runtime-stability-uat.sh >/dev/null 2>&1
fi

require_env RUNTIME_LOADTEST_DATABASE_URL
real_release_database_url="$DATABASE_URL"
step "runtime-loadtest-database"
RUNTIME_LOADTEST_FORBID_DATABASE_URL="$real_release_database_url" \
RUNTIME_LOADTEST_ALLOW_UNSAFE_DATABASE= \
bash scripts/ensure-runtime-loadtest-database.sh
step "runtime-loadtest-seed"
DATABASE_URL="$RUNTIME_LOADTEST_DATABASE_URL" \
RUNTIME_LOADTEST_FORBID_DATABASE_URL="$real_release_database_url" \
XRAYC_ENV=loadtest \
bash scripts/prepare-runtime-loadtest-seed.sh
step "runtime-loadtest"
DATABASE_URL="$RUNTIME_LOADTEST_DATABASE_URL" \
RUNTIME_LOADTEST_FORBID_DATABASE_URL="$real_release_database_url" \
RUNTIME_LOADTEST_PROFILE="${RUNTIME_LOADTEST_PROFILE:-large}" \
RUNTIME_LOADTEST_VALIDATE_ONLY=0 \
RUNTIME_LOADTEST_REQUIRE_LARGE=1 \
XRAYC_ENV=loadtest \
bash scripts/runtime-loadtest.sh

step "runtime-http-loadtest-real"
for attempt in $(seq 1 12); do
  if curl --fail --silent --show-error --max-time 10 "${BASE_URL%/}/health" >/dev/null; then
    break
  fi
  if [[ "$attempt" -eq 12 ]]; then
    echo "runtime HTTP loadtest preflight health failed" >&2
    exit 1
  fi
  sleep 5
done
RUNTIME_HTTP_LOADTEST_PROFILE=large \
RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1 \
RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY=0 \
RUNTIME_HTTP_LOADTEST_CONNECT_TIMEOUT_SECONDS="${RUNTIME_HTTP_LOADTEST_CONNECT_TIMEOUT_SECONDS:-10}" \
BASE_URL="$BASE_URL" \
bash scripts/runtime-http-loadtest.sh

step "runtime-http-loadtest-compose"
RUNTIME_HTTP_LOADTEST_COMPOSE_NO_BUILD=1 \
RUNTIME_HTTP_LOADTEST_PROFILE=large \
RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1 \
RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY=0 \
bash scripts/runtime-http-loadtest-compose.sh

step "real-auth-ha-uat"
XRAYC_AUTH_HA_FORCE_COMPOSE_START=1 \
XRAYC_AUTH_HA_FORCE_COMPOSE_NO_BUILD=1 \
XRAYC_AUTH_HA_FORCE_MIN_API_REPLICAS=2 \
XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY=0 \
bash scripts/real-auth-ha-uat.sh >/dev/null 2>&1

step "real-auth-ha-stability-uat"
XRAYC_AUTH_HA_FORCE_COMPOSE_START=1 \
XRAYC_AUTH_HA_FORCE_COMPOSE_NO_BUILD=1 \
XRAYC_AUTH_HA_FORCE_MIN_API_REPLICAS=2 \
XRAYC_AUTH_HA_FORCE_PROFILE=10m \
XRAYC_AUTH_HA_FORCE_DURATION_SECONDS="${AUTH_HA_STABILITY_10M_DURATION_SECONDS:-600}" \
XRAYC_AUTH_HA_FORCE_INTERVAL_SECONDS="${AUTH_HA_STABILITY_10M_INTERVAL_SECONDS:-120}" \
XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY=0 \
bash scripts/real-auth-ha-stability-uat.sh >/dev/null 2>&1

step "ops-mistake-recovery-uat"
OPS_MISTAKE_RECOVERY_UAT_VALIDATE_ONLY=0 \
bash scripts/ops-mistake-recovery-uat.sh >/dev/null 2>&1

step "ops-mistake-recovery-large-uat"
OPS_MISTAKE_RECOVERY_LARGE_UAT_VALIDATE_ONLY=0 \
bash scripts/ops-mistake-recovery-large-uat.sh >/dev/null 2>&1

step "real-multi-user-traffic-uat"
XRAYC_REAL_MULTI_USER_COUNT="${XRAYC_REAL_MULTI_USER_COUNT:-10}" \
XRAYC_REAL_MULTI_USER_DURATION_SECONDS="${XRAYC_REAL_MULTI_USER_DURATION_SECONDS:-2400}" \
XRAYC_REAL_MULTI_USER_ROUNDS="${XRAYC_REAL_MULTI_USER_ROUNDS:-2}" \
XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES="${XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES:-3}" \
bash scripts/real-multi-user-traffic-uat.sh >/dev/null 2>&1

step "running-runtime-current-final"
bash scripts/check-running-images-current.sh

GATE_FINISHED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'real-release-gate: status=0 started_at_utc=%s finished_at_utc=%s matrix_mode=%s inbound_matrix=%s runtime_restore=%s real_stability_profile=%s traffic_backlog=%s runtime_loadtest_large=%s http_runtime_loadtest_large=1 optional_24h=%s auth_ha=%s auth_ha_stability_profile=%s ops_recovery=%s ops_recovery_large=%s multi_user_count=%s multi_user_duration_seconds=%s multi_user_rounds=%s\n' \
  "$GATE_STARTED_AT" \
  "$GATE_FINISHED_AT" \
  "${REAL_RELEASE_MATRIX_MODE:-relay}" \
  "${RUN_REAL_ACCESS_INBOUND_MATRIX_E2E:-1}" \
  "${RUN_REAL_RUNTIME_RESTORE_DEPLOY:-1}" \
  "${UAT_PROFILE:-10m}" \
  "${RUN_ACCESS_TRAFFIC_BACKLOG_UAT:-1}" \
  "${RUN_RUNTIME_LOADTEST:-1}" \
  "${RUN_REAL_STABILITY_UAT_24H:-0}" \
  "${RUN_AUTH_HA_UAT:-1}" \
  "${AUTH_HA_STABILITY_PROFILE:-10m}" \
  "${RUN_OPS_MISTAKE_RECOVERY_UAT:-1}" \
  "${RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT:-1}" \
  "${XRAYC_REAL_MULTI_USER_COUNT:-10}" \
  "${XRAYC_REAL_MULTI_USER_DURATION_SECONDS:-2400}" \
  "${XRAYC_REAL_MULTI_USER_ROUNDS:-2}"
