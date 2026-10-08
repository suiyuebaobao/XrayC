#!/usr/bin/env bash
# 用途：验证 v2 主线能力在前端 E2E、真实门禁和测试文档中有覆盖。
# 范围：执行静态覆盖断言，不启动服务、不访问数据库、不调用远端。
# 输入：读取 Playwright 用例、真实发布脚本、真实 E2E 库和测试文档。
# 输出：缺失覆盖时输出具体标签和文件路径，成功时静默完成。
# 依赖：通过 grep 字面量和正则检查关键能力、禁用项和真实门禁引用。
# 安全：只检查代码和文档中的固定片段，不读取私有环境变量。
# 约束：禁止 runtime no-mock 用例出现 page.route 模拟 API。
# 行为：一次列出全部缺失覆盖，最终仍以非零状态阻断，避免逐项重复运行。
# 失败：用于 CI 阶段阻断主线能力被误删或退化。
# 维护：新增 v2 主线要求时应添加对应 require_literal 或 require_regex。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

failures=0

fail() {
  printf 'v2-mainline-coverage: %s\n' "$1" >&2
  failures=$((failures + 1))
}

require_file() {
  local path="$1"
  [[ -f "$path" ]] || fail "missing required file: $path"
}

require_literal() {
  local path="$1"
  local literal="$2"
  local label="$3"

  grep -Fq -- "$literal" "$path" || fail "$label is not covered in $path"
}

require_regex() {
  local path="$1"
  local pattern="$2"
  local label="$3"

  grep -Eq -- "$pattern" "$path" || fail "$label is not covered in $path"
}

require_absent_regex() {
  local path="$1"
  local pattern="$2"
  local label="$3"

  if grep -Eq -- "$pattern" "$path"; then
    fail "$label must not appear in $path"
  fi
}

require_function_absent_literal() {
  local path="$1"
  local function_name="$2"
  local literal="$3"
  local label="$4"
  local status

  set +e
  awk -v fn="$function_name" -v needle="$literal" '
    $0 == fn "() {" {
      in_fn = 1
      found_fn = 1
      next
    }
    in_fn && index($0, needle) {
      found_needle = 1
    }
    in_fn && $0 == "}" {
      exit(found_needle ? 1 : 0)
    }
    END {
      if (!found_fn) {
        exit 2
      }
    }
  ' "$path"
  status=$?
  set -e
  case "$status" in
    0) ;;
    1) fail "$label must not appear in $function_name in $path" ;;
    2) fail "$function_name is not covered in $path" ;;
    *) fail "unable to inspect $function_name in $path" ;;
  esac
}

require_literal_order() {
  local path="$1"
  local first="$2"
  local second="$3"
  local third="$4"
  local label="$5"
  local first_line second_line third_line

  first_line="$(grep -nF -- "$first" "$path" | head -n 1 | cut -d: -f1 || true)"
  second_line="$(grep -nF -- "$second" "$path" | head -n 1 | cut -d: -f1 || true)"
  third_line="$(grep -nF -- "$third" "$path" | head -n 1 | cut -d: -f1 || true)"
  if [[ -z "$first_line" || -z "$second_line" || -z "$third_line" \
    || "$first_line" -ge "$second_line" || "$second_line" -ge "$third_line" ]]; then
    fail "$label is not covered in $path"
  fi
}

SMOKE_SPEC="frontend/e2e/smoke.spec.ts"
SMOKE_USER_SPEC="frontend/e2e/smoke/user.spec.ts"
SMOKE_ADMIN_SPEC="frontend/e2e/smoke/admin-core.spec.ts"
SMOKE_OPERATIONS_RUNTIME_SPEC="frontend/e2e/smoke/admin-operations-runtime.spec.ts"
SMOKE_ROUTING_SPEC="frontend/e2e/smoke/admin-routing.spec.ts"
NO_MOCK_SPEC="frontend/e2e/runtime-no-mock.spec.ts"
NO_MOCK_ASSERTIONS="frontend/e2e/runtime/runtime-assertions.ts"
MAKEFILE="Makefile"
REAL_RELEASE_ENV_EXAMPLE=".env.real-release.example"
REAL_GATE="scripts/check-real-release.sh"
REAL_E2E_AUTH_ACCOUNTS="scripts/check-real-e2e-auth-accounts.sh"
AGENT_INSTALL_CLEANUP="scripts/check-agent-install-cleanup.sh"
OPS_RECOVERY_UAT="scripts/ops-mistake-recovery-uat.sh"
OPS_RECOVERY_LARGE_UAT="scripts/ops-mistake-recovery-large-uat.sh"
REAL_TEST_SERVER_ASSETS="scripts/check-real-test-servers.sh"
REAL_RELEASE_GAP_REPORT="scripts/real-release-gap-report.sh"
REAL_RELEASE_ENV_RULES="scripts/lib/validate-real-release-env/rules.sh"
REAL_RELEASE_ENV_TEMPLATE="scripts/prepare-real-release-env.sh"
REAL_RELEASE_ENV_BOOTSTRAP="scripts/bootstrap-real-release-env.sh"
RUNTIME_LOADTEST_DB_BOOTSTRAP="scripts/ensure-runtime-loadtest-database.sh"
RUNTIME_HTTP_LOADTEST="scripts/runtime-http-loadtest.sh"
RUNTIME_HTTP_LOADTEST_COMPOSE="scripts/runtime-http-loadtest-compose.sh"
DB_AGENT_SESSIONS="crates/db/src/store/agent_sessions.rs"
REAL_AGENT_FIRST_NODES="scripts/real-agent-first-nodes-e2e.sh"
REAL_REMOTE="scripts/real-remote-access-deploy-e2e.sh"
RESTORE_RUNTIME_DEPLOY="scripts/restore-real-runtime-access-deploy.sh"
RESTORE_RUNTIME_ASSETS="scripts/lib/restore-real-runtime-access-deploy/runtime-assets.sh"
REAL_POOL="scripts/real-access-pool-e2e.sh"
REAL_MATRIX="scripts/real-access-third-party-matrix-e2e.sh"
REAL_RELAY_MATRIX="scripts/real-access-third-party-matrix-relay-e2e.sh"
REAL_RELAY_MATRIX_REDACTION="scripts/lib/real-access-third-party-matrix-relay/subscription-redaction.sh"
REAL_INBOUND_MATRIX_DB_PREPARE="scripts/real-access-inbound-matrix-db-prepare.sh"
REAL_INBOUND_MATRIX_DB_PREPARE_HELPER="scripts/lib/real-access-inbound-matrix/db-prepare.sh"
REAL_INBOUND_MATRIX_DB_PREPARE_CONTEXT="scripts/lib/real-access-inbound-matrix/db-prepare-context.sh"
REAL_INBOUND_MATRIX_DB_PREPARE_SQL="scripts/lib/real-access-inbound-matrix/db-prepare.psql"
REAL_INBOUND_MATRIX_INVENTORY_SSH="scripts/lib/real-access-inbound-matrix/inventory-ssh.sh"
REAL_INBOUND_MATRIX_CLIENT_CONFIGS="scripts/real-access-inbound-matrix-client-configs.py"
REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE="scripts/lib/real-access-inbound-matrix/subscription-prepare.sh"
REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS="scripts/lib/real-access-inbound-matrix/subscription-ports.sh"
REAL_SUBSCRIPTION_CLIENT_TRAFFIC="scripts/lib/real-subscription-client-compat/traffic.sh"
REAL_V2_RELAY_POOL="scripts/real-v2-relay-pool-e2e.sh"
REAL_V2_RELAY_POOL_FUNCTIONS="scripts/lib/real-v2-relay-pool/functions.sh"
REAL_V2_RELAY_POOL_REBIND="scripts/lib/real-v2-relay-pool/rebind.sh"
REAL_V2_RELAY_POOL_CLIENT="scripts/lib/real-v2-relay-pool/client.sh"
REAL_V2_RELAY_POOL_EXIT_HOSTS="scripts/lib/real-v2-relay-pool/exit-hosts.sh"
REAL_V2_RELAY_POOL_RATE_LIMIT="scripts/lib/real-v2-relay-pool/rate-limit.sh"
REAL_V2_RELAY_POOL_SUBSCRIPTION="scripts/lib/real-v2-relay-pool/subscription.sh"
REAL_V2_RELAY_POOL_TARGETS="scripts/lib/real-v2-relay-pool/targets.sh"
REAL_V2_RELAY_POOL_CLIENT_CONFIG_TEST="scripts/test-real-v2-shadowsocks-client-config.sh"
REAL_LIB="scripts/real-e2e-lib.sh"
REAL_LEDGER_LIB="scripts/lib/real-e2e/ledger.sh"
REAL_PROTOCOL_UPSTREAM_LAB_PAYLOAD="scripts/lib/real-protocol-upstream-lab/payload.sh"
REAL_PROTOCOL_MATRIX_ENV_VALIDATION="scripts/lib/check-real-protocol-matrix-env/validation.sh"
REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST="scripts/test-real-protocol-matrix-endpoint-payload.sh"
REAL_THIRD_PARTY_REDACTION_TEST="scripts/test-real-third-party-redaction.sh"
TEST_DOC="文档/测试/真实测试用例.md"
EXIT_POOL_HELPER="frontend/src/views/exit-pools/exitPoolsPageHelpers.ts"
LINE_POOL_PAGE="frontend/src/views/LinePoolPage.vue"
LOCAL_EXIT_DIALOG="frontend/src/views/access-lines/LocalExitLinesDialog.vue"
LOCAL_EXIT_ROW="frontend/src/views/access-lines/LocalExitLineRow.vue"
LOCAL_EXIT_HELPER="frontend/src/views/access-lines/localExitLines.ts"
ACCESS_ENTRIES_PAGE="frontend/src/views/AccessEntriesPage.vue"
ACCESS_LINES_PAGE="frontend/src/views/access-lines/useAccessLinesPage.ts"
ARCH_DOC="文档/架构/架构说明.md"
API_OVERVIEW_DOC="文档/接口/总览.md"
API_DRAFT_DOC="文档/接口/接口草案.md"
DEPLOY_DOC="文档/部署/部署说明.md"
PLAN_DOC="开发方案.md"
RATE_LIMIT_PLAN_DOC="docs/superpowers/plans/2026-06-03-user-rate-limit.md"
RATE_LIMIT_SPEC_DOC="docs/superpowers/specs/2026-06-03-user-rate-limit-design.md"
REMOVED_CLIENT_ENDPOINT_TEST="crates/api/src/removed_client_endpoint_tests.rs"

for path in "$SMOKE_SPEC" "$SMOKE_USER_SPEC" "$SMOKE_ADMIN_SPEC" "$SMOKE_OPERATIONS_RUNTIME_SPEC" "$SMOKE_ROUTING_SPEC" "$NO_MOCK_SPEC" "$NO_MOCK_ASSERTIONS" "$MAKEFILE" "$REAL_RELEASE_ENV_EXAMPLE" "$REAL_GATE" "$REAL_E2E_AUTH_ACCOUNTS" "$AGENT_INSTALL_CLEANUP" "$OPS_RECOVERY_UAT" "$OPS_RECOVERY_LARGE_UAT" "$REAL_TEST_SERVER_ASSETS" "$REAL_RELEASE_GAP_REPORT" "$REAL_RELEASE_ENV_RULES" "$REAL_RELEASE_ENV_TEMPLATE" "$REAL_RELEASE_ENV_BOOTSTRAP" "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "$RUNTIME_HTTP_LOADTEST_COMPOSE" "$DB_AGENT_SESSIONS" "$REAL_AGENT_FIRST_NODES" "$REAL_REMOTE" "$RESTORE_RUNTIME_DEPLOY" "$RESTORE_RUNTIME_ASSETS" "$REAL_POOL" "$REAL_MATRIX" "$REAL_RELAY_MATRIX" "$REAL_RELAY_MATRIX_REDACTION" "$REAL_INBOUND_MATRIX_DB_PREPARE" "$REAL_INBOUND_MATRIX_DB_PREPARE_HELPER" "$REAL_INBOUND_MATRIX_DB_PREPARE_CONTEXT" "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "$REAL_INBOUND_MATRIX_INVENTORY_SSH" "$REAL_INBOUND_MATRIX_CLIENT_CONFIGS" "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "$REAL_SUBSCRIPTION_CLIENT_TRAFFIC" "$REAL_V2_RELAY_POOL" "$REAL_V2_RELAY_POOL_FUNCTIONS" "$REAL_V2_RELAY_POOL_CLIENT" "$REAL_V2_RELAY_POOL_EXIT_HOSTS" "$REAL_V2_RELAY_POOL_RATE_LIMIT" "$REAL_V2_RELAY_POOL_SUBSCRIPTION" "$REAL_V2_RELAY_POOL_TARGETS" "$REAL_V2_RELAY_POOL_CLIENT_CONFIG_TEST" "$REAL_LIB" "$REAL_LEDGER_LIB" "$REAL_PROTOCOL_UPSTREAM_LAB_PAYLOAD" "$REAL_PROTOCOL_MATRIX_ENV_VALIDATION" "$REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST" "$REAL_THIRD_PARTY_REDACTION_TEST" "$TEST_DOC" "$EXIT_POOL_HELPER" "$LINE_POOL_PAGE" "$LOCAL_EXIT_DIALOG" "$LOCAL_EXIT_ROW" "$LOCAL_EXIT_HELPER" "$ACCESS_ENTRIES_PAGE" "$ACCESS_LINES_PAGE" "$ARCH_DOC" "$API_OVERVIEW_DOC" "$API_DRAFT_DOC" "$DEPLOY_DOC" "$PLAN_DOC" "$RATE_LIMIT_PLAN_DOC" "$RATE_LIMIT_SPEC_DOC" "$REMOVED_CLIENT_ENDPOINT_TEST"; do
  require_file "$path"
done

require_literal "$SMOKE_USER_SPEC" "香港分组" "Playwright smoke subscription line-group view"
require_literal "$SMOKE_USER_SPEC" "access.example.test:443" "Playwright smoke subscription access endpoint"
require_literal "$SMOKE_USER_SPEC" "third-party-exit.example.test" "Playwright smoke subscription exit leak fixture"
require_literal "$SMOKE_USER_SPEC" "secret-agent-token" "Playwright smoke subscription secret redaction fixture"
require_literal "$SMOKE_ADMIN_SPEC" "中转节点" "Playwright smoke admin transit-node navigation"
require_literal "$SMOKE_ROUTING_SPEC" "入口管理" "Playwright smoke access entry management"
require_literal "$SMOKE_ADMIN_SPEC" "分组" "Playwright smoke admin line-group navigation"
require_literal "$SMOKE_ROUTING_SPEC" "创建分组" "Playwright smoke line-group management"
require_literal "$SMOKE_OPERATIONS_RUNTIME_SPEC" "不是 0，而是未上报" "Playwright smoke runtime no-fake-zero state"
require_literal "$SMOKE_OPERATIONS_RUNTIME_SPEC" "fresh · 实时" "Playwright smoke runtime fresh state"
require_literal "$SMOKE_OPERATIONS_RUNTIME_SPEC" "stale · 旧快照" "Playwright smoke runtime stale state"

require_literal "$NO_MOCK_SPEC" "gotoAndWaitForApi" "runtime no-mock real API wait"
require_literal "$NO_MOCK_SPEC" "collectApiStatuses" "runtime no-mock API status collection"
require_literal "$NO_MOCK_SPEC" "assertSubscriptionYamlDownload" "runtime no-mock subscription YAML download"
require_literal "$NO_MOCK_ASSERTIONS" "assertNoSensitiveSubscriptionYamlLeak" "runtime no-mock subscription leak guard"
require_literal "$NO_MOCK_ASSERTIONS" "allowedServers.has" "runtime no-mock access-line allowlist"
require_literal "$NO_MOCK_ASSERTIONS" "assertNoFakeZeroRuntimeCards" "runtime no-mock no fake runtime zero"
require_absent_regex "$NO_MOCK_SPEC" "page\\.route\\(" "runtime no-mock route mocking"

require_literal "$EXIT_POOL_HELPER" '"security": "reality"' "VLESS line default Reality security"
require_literal "$EXIT_POOL_HELPER" '"public_key": ""' "VLESS line default Reality public key placeholder"
require_literal "$EXIT_POOL_HELPER" '"short_id": ""' "VLESS line default Reality short id placeholder"
require_literal "$EXIT_POOL_HELPER" "vlessSecurityOptions" "VLESS security selection helper"
require_literal "$EXIT_POOL_HELPER" "setVlessOutboundSecurity" "VLESS security config mutation helper"
require_literal "$LINE_POOL_PAGE" "VLESS 安全模式" "line pool VLESS security selector"
require_literal "$LOCAL_EXIT_ROW" "VLESS 安全模式" "local exit VLESS security selector"
require_literal "$LOCAL_EXIT_ROW" "multiple" "local exit network multi-select"
require_literal "$LOCAL_EXIT_HELPER" "localExitTransportOptionsFor" "protocol/security-aware local exit transport filtering"
require_literal "$LOCAL_EXIT_HELPER" "localExitCarriageOptionsFor" "protocol/security-aware local exit carriage filtering"
require_literal "$LOCAL_EXIT_HELPER" "crypto.randomUUID" "local exit VLESS UUID default generation"
require_literal "$LOCAL_EXIT_HELPER" "randomCredential" "local exit generated protocol credentials"
require_literal "$ACCESS_ENTRIES_PAGE" "入口管理" "access entry management page"
require_literal "$ACCESS_ENTRIES_PAGE" "createAccessEntry(payload)" "access entry creation API"
require_literal "frontend/src/views/access-entries/useEntryExitBinding.ts" "createAccessEntryExitBinding" "access entry exit binding API"
require_literal "$ACCESS_LINES_PAGE" "expandLocalExitLinePayloads" "local exit multi-select expansion"
require_literal "$DEPLOY_DOC" "VLESS 线路表单默认使用 Reality" "deployment docs VLESS default Reality"
require_literal "$DEPLOY_DOC" "网络模式支持多选并按协议与 VLESS security 过滤" "deployment docs multi network mode filtering"
require_literal "$PLAN_DOC" "线路表单默认使用 `security=reality`" "plan docs VLESS default Reality"
require_literal "$PLAN_DOC" "网络模式支持多选，并按协议与 VLESS security 筛选可选项" "plan docs multi network mode filtering"
require_absent_regex "$ARCH_DOC" "本机出口服务使用 SOCKS5/HTTP" "legacy local-exit-only-SOCKS-HTTP architecture wording"
require_absent_regex "$API_OVERVIEW_DOC" "中转节点可创建 SOCKS5/HTTP 本机出口服务" "legacy local-exit-only-SOCKS-HTTP API overview wording"
require_absent_regex "$API_DRAFT_DOC" "本机出口服务主路径生成 SOCKS5/HTTP" "legacy local-exit-only-SOCKS-HTTP API draft wording"
require_absent_regex "$API_DRAFT_DOC" '本机出口服务主路径只使用 `socks` 或 `http`' "legacy local-exit-only-SOCKS-HTTP API create wording"

require_literal "$REAL_GATE" "real-agent-first-nodes-e2e" "real release agent-first nodes E2E"
require_literal "$REAL_GATE" 'step "real-test-server-assets"' "real release checks test server assets before long real tests"
require_literal "$REAL_TEST_SERVER_ASSETS" "real-test-server-assets: passed targets=" "real test server asset preflight summary"
require_literal "$REAL_TEST_SERVER_ASSETS" "first inventory targets must be distinct" "real test server preflight distinct-host guard"
require_literal "$REAL_TEST_SERVER_ASSETS" 'target_${index} ssh failed' "real test server preflight SSH guard"
require_literal "$REAL_TEST_SERVER_ASSETS" "target_{index} alias must be {expected_alias}" "real test server preflight enforces server_N alias order"
require_literal "$REAL_TEST_SERVER_ASSETS" "server_1 must be direct/non-CF" "real test server preflight rejects Cloudflare marker on server_1"
require_literal "$REAL_TEST_SERVER_ASSETS" "server_4 must be marked Cloudflare/orange-cloud" "real test server preflight requires Cloudflare marker on server_4"
require_literal "$REAL_RELEASE_GAP_REPORT" 'if [[ "$(matrix_mode)" == "direct-diagnostic" ]]' "gap report direct diagnostic branch matches normalized matrix mode"
require_literal_order "$REAL_GATE" 'step "real-agent-first-nodes-e2e"' 'step "real-access-inbound-matrix-e2e"' 'step "real-access-third-party-matrix-relay-e2e"' "real release installs agents before inbound matrix"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "INSERT INTO access_entries (" "real inbound matrix writes temporary entries to current entry model"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "INSERT INTO access_entry_exit_bindings (" "real inbound matrix writes temporary bindings to current binding model"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "INSERT INTO line_group_binding_nodes (" "real inbound matrix attaches temporary bindings to flat groups"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE" "DELETE FROM access_entry_exit_bindings" "real inbound matrix cleanup removes temporary binding rows"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE" "DELETE FROM access_entries" "real inbound matrix cleanup removes temporary entry rows"
require_literal "$REAL_AGENT_FIRST_NODES" 'installing ${TARGET_COUNT} test access agents' "real agent-first installs configured inventory nodes"
require_literal "$REAL_AGENT_FIRST_NODES" 'waiting for ${TARGET_COUNT} platform heartbeats' "real agent-first waits for online heartbeats"
require_literal "$REAL_GATE" "REAL_RELEASE_MATRIX_MODE=direct is diagnostic only" "real release forbids direct mode as full gate"
require_absent_regex "$REAL_GATE" 'if \[\[ "\$REAL_RELEASE_MATRIX_MODE" == "direct" \]\]' "real release gate must not keep unreachable direct-mode branches"
require_absent_regex "$REAL_GATE" "real-remote-access-deploy-e2e|real-access-pool-e2e|real-access-third-party-matrix-e2e|assert_recent_runtime_observation" "real release gate must not keep unreachable direct-mode E2E calls"
require_literal "$REAL_GATE" "real-access-third-party-matrix-relay-e2e" "real release relay matrix E2E"
require_literal "$REAL_GATE" "real-v2-relay-pool-e2e" "real release self-hosted local exit relay pool E2E"
require_literal "scripts/prepare-real-protocol-matrix-endpoints.sh" "refresh_existing_endpoint_health" "real protocol matrix endpoint updates restore resource health"
require_literal "$REAL_GATE" 'XRAYC_REAL_E2E_ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}"' "real release self-hosted E2E admin account forwarding"
require_literal "$REAL_GATE" 'XRAYC_REAL_E2E_ADMIN_PASSWORD="${ADMIN_LOGIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}"' "real release self-hosted E2E admin password forwarding"
require_literal "$REAL_GATE" 'XRAYC_REAL_E2E_USER_ACCOUNT="${USER_LOGIN_ACCOUNT:-${E2E_USER_ACCOUNT:-}}"' "real release self-hosted E2E user account forwarding"
require_literal "$REAL_GATE" 'XRAYC_REAL_E2E_USER_PASSWORD="${USER_LOGIN_PASSWORD:-${E2E_USER_PASSWORD:-}}"' "real release self-hosted E2E user password forwarding"
require_literal "$REAL_V2_RELAY_POOL" "XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1" "real V2 relay pool agent install keeps pre-created node credentials"
require_literal "$REAL_GATE" "ensure-runtime-loadtest-database.sh" "real release runtime loadtest database bootstrap"
require_literal "$REAL_GATE" "prepare-real-protocol-matrix-endpoints.sh" "real release recreates missing protocol matrix endpoints before relay matrix"
require_literal "$REAL_GATE" "--allow-missing-endpoints" "real release protocol precheck allows prepare to create relay endpoint ids"
require_literal_order "$REAL_GATE" 'step "prepare-real-protocol-matrix-endpoints"' 'step "real-access-third-party-matrix-relay-e2e"' 'step "real-v2-relay-pool-e2e"' "real release prepares protocol endpoints before relay matrix"
require_literal_order "$REAL_GATE" 'step "prepare-real-protocol-matrix-endpoints"' 'step "real-protocol-matrix-env-after-prepare"' 'step "real-access-third-party-matrix-relay-e2e"' "real release validates protocol env after endpoint prepare"
require_literal "scripts/check-real-protocol-matrix-env.sh" "--allow-missing-endpoints" "real protocol matrix env supports pre-prepare endpoint-id check"
require_literal "$REAL_PROTOCOL_MATRIX_ENV_VALIDATION" "require_endpoint_uuid_for_matrix" "real protocol matrix env requires endpoint ids after prepare"
require_literal "scripts/lib/prepare-real-protocol-matrix-endpoints/common.sh" "endpoint_payload_matches_database" "real protocol endpoint prepare detects stale endpoint payloads"
require_literal "scripts/prepare-real-protocol-matrix-endpoints.sh" "existing value does not match current private input" "real protocol endpoint prepare recreates stale endpoint ids"
require_literal "$REAL_GATE" "RUNTIME_LOADTEST_ALLOW_UNSAFE_DATABASE=" "real release runtime loadtest bootstrap clears unsafe database override"
require_literal "$REAL_GATE" "BACKLOG_WAIT_SECONDS=300" "real release backlog UAT uses extended replay wait window"
require_literal "$REAL_GATE" "XRAYC_AUTH_HA_FORCE_COMPOSE_START=1" "real release auth HA UAT forces isolated compose mode after env load"
require_literal "$REAL_GATE" "RUNTIME_HTTP_LOADTEST_COMPOSE_NO_BUILD=1" "real release isolated HTTP compose loadtest reuses current image tag"
require_literal "$RUNTIME_HTTP_LOADTEST_COMPOSE" "up_flags=(-d --no-build)" "runtime HTTP compose always forbids rebuilding current image tag"
require_absent_regex "$RUNTIME_HTTP_LOADTEST_COMPOSE" 'up_flags\+\=\(--build\)' "runtime HTTP compose build mode that retags current image"
require_literal "$MAKEFILE" "RUNTIME_HTTP_LOADTEST_COMPOSE_NO_BUILD=1 bash scripts/runtime-http-loadtest-compose.sh" "runtime HTTP compose make target uses no-build mode"
require_literal_order "$REAL_GATE" 'step "runtime-http-loadtest-compose"' 'step "running-runtime-current-final"' "GATE_FINISHED_AT=" "real release script verifies running images after isolated compose tests"
require_literal "$REAL_GATE" 'step "agent-install-cleanup-contract"' "real release script uses agent install cleanup contract name"
require_literal "$REAL_GATE" "scripts/check-agent-install-cleanup.sh" "real release script runs agent install cleanup contract"
require_literal "$MAKEFILE" "check-agent-install-cleanup:" "Makefile exposes agent install cleanup check"
require_literal "$MAKEFILE" "scripts/check-agent-install-cleanup.sh" "Makefile runs agent install cleanup script"
require_absent_regex "$REAL_GATE" "check-remote-deploy-cleanup|remote-deploy-cleanup" "real release script old remote deploy cleanup name"
require_absent_regex "$MAKEFILE" "check-remote-deploy-cleanup|remote-deploy-cleanup" "Makefile old remote deploy cleanup name"
require_absent_regex "$AGENT_INSTALL_CLEANUP" "remote-deploy-cleanup" "agent install cleanup script old output prefix"
require_literal "$REAL_GATE" "DISABLE_SYNTHETIC_AGENT_POSTS=1" "real release disables synthetic agent posts"
require_literal "$REAL_GATE" "REQUIRE_LEDGER_SOURCE_CHECK=true" "real release ledger source check"
require_literal "$REAL_RELAY_MATRIX" "wait_protocol_route_and_ledger" "real release relay matrix verifies route, ledger, and billing together"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "assert_relay_subscription_eligibility" "real release relay matrix checks subscription access-line eligibility"
require_literal "$MAKEFILE" 'check-real-release: check-real-release-env check-deploy-artifact-endpoint' "real release checks an already deployed runtime"
require_literal "$MAKEFILE" "bash scripts/check-running-images-current.sh" "real release gate verifies running images after switching to latest build"
require_literal "$MAKEFILE" 'python3 scripts/check-release-status.py' "read-only release status is separate from deployment and real UAT"
require_literal "$MAKEFILE" '$(COMPOSE) build api caddy' "real release artifact packaging builds api and caddy images"
require_literal_order "$REAL_GATE" 'step "release-env"' "bash scripts/check-running-images-current.sh" 'step "real-smoke"' "real release script refuses stale running runtime before real checks"
require_literal "$REAL_GATE" 'step "real-e2e-auth-accounts"' "real release script preflights no-mock login accounts"
require_literal_order "$REAL_GATE" 'step "running-runtime-current"' 'step "real-e2e-auth-accounts"' 'step "real-playwright-no-mock"' "real release script checks login accounts before Playwright no-mock"
require_absent_regex "$OPS_RECOVERY_UAT" "SELECT user_id FROM user_access_line_assignments WHERE user_id = :'user_id'::uuid" "ops recovery UAT broad user assignment-zero assertion"
require_literal "$OPS_RECOVERY_UAT" "line_group_id = :'group_id'::uuid" "ops recovery UAT checks deleted-plan assignment by temporary group"
require_literal "$OPS_RECOVERY_LARGE_UAT" "action IN ('\${run_marker}', '\${run_marker}.restore')" "ops large recovery audit check uses indexed exact action match"
require_absent_regex "$OPS_RECOVERY_LARGE_UAT" "action LIKE '\\$\\{run_marker\\}%'" "ops large recovery audit check must not prefix-scan action"
require_literal "$REAL_RELEASE_ENV_RULES" "require_production_jwt_runtime_env" "real release env gate validates production JWT runtime env"
require_literal "$REAL_RELEASE_ENV_EXAMPLE" 'JWT_EXPIRES_IN="30m"' "real release env example includes production access token TTL"
require_literal "$REAL_RELEASE_ENV_EXAMPLE" 'JWT_REFRESH_EXPIRES_IN="7d"' "real release env example includes production refresh token TTL"
require_literal "$REAL_RELEASE_ENV_EXAMPLE" "REAL_ACCESS_INBOUND_MATRIX_ENABLE_THIRD_PARTY_SOCKS" "real release env example documents explicit inbound matrix third-party socks opt-in"
require_literal "$REAL_RELEASE_ENV_EXAMPLE" "REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES" "real release env example documents inbound matrix remote retry tuning"
require_literal "$REAL_RELEASE_ENV_EXAMPLE" "ordinary high-port matrix rejects server_4" "real release env example documents Cloudflare target exclusion"
require_literal "$REAL_RELEASE_ENV_TEMPLATE" 'JWT_EXPIRES_IN="30m"' "real release env template includes production access token TTL"
require_literal "$REAL_RELEASE_ENV_TEMPLATE" 'JWT_REFRESH_EXPIRES_IN="7d"' "real release env template includes production refresh token TTL"
require_literal "$REAL_RELEASE_ENV_TEMPLATE" "REAL_ACCESS_INBOUND_MATRIX_ENABLE_THIRD_PARTY_SOCKS" "real release env template documents explicit inbound matrix third-party socks opt-in"
require_literal "$REAL_RELEASE_ENV_TEMPLATE" "REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES" "real release env template documents inbound matrix remote retry tuning"
require_literal "$REAL_RELEASE_ENV_BOOTSTRAP" "set_value JWT_EXPIRES_IN" "real release bootstrap writes JWT access TTL"
require_literal "$REAL_RELEASE_ENV_BOOTSTRAP" "set_value JWT_REFRESH_EXPIRES_IN" "real release bootstrap writes JWT refresh TTL"
require_literal "$REAL_RELEASE_ENV_BOOTSTRAP" "XRAYC_REAL_RELEASE_COMPOSE_DATABASE_URL" "real release bootstrap writes compose-internal database URL"

require_literal "$REAL_REMOTE" "XRAYC_REAL_E2E_INVENTORY" "real remote E2E private inventory"
require_literal "$REAL_REMOTE" "expected_listen_ports" "real remote E2E access listen port assertion"
require_literal "$RESTORE_RUNTIME_DEPLOY" "XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1" "restore runtime deploy keeps pre-created node credentials"
require_literal "$RESTORE_RUNTIME_DEPLOY" 'XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE="${XRAYC_RUNTIME_RESTORE_CLEANUP_ON_FAILURE:-1}"' "restore runtime deploy can preserve failed remote install for debugging"
require_literal "$RESTORE_RUNTIME_ASSETS" "%.sslip.io" "restore runtime deploy accepts direct sslip hostnames while avoiding Cloudflare high ports"
require_literal "$REAL_POOL" "xrayc_real_e2e_assert_subscription_access_servers" "real access pool subscription access-line check"
require_literal "$REAL_POOL" "xrayc_real_e2e_wait_billing_increase" "real access pool billing increase"
require_literal "$REAL_MATRIX" "socks,http,vless,trojan,shadowsocks,hy2" "real third-party protocol matrix"
require_literal "$REAL_MATRIX" "THIRD_PARTY_VLESS_RAW_URL" "real third-party VLESS upstream requirement"
require_literal "$REAL_MATRIX" "THIRD_PARTY_HY2_RAW_URL" "real third-party HY2 upstream requirement"
require_literal "$REAL_PROTOCOL_UPSTREAM_LAB_PAYLOAD" '"HY2_SERVER_NAME": hy2_domain' "real protocol lab HY2 uses real TLS domain"
require_literal "$REAL_PROTOCOL_UPSTREAM_LAB_PAYLOAD" "/etc/letsencrypt/live/{values['HY2_SERVER_NAME']}/fullchain.pem" "real protocol lab HY2 uses letsencrypt certificate"
require_absent_regex "$REAL_PROTOCOL_UPSTREAM_LAB_PAYLOAD" "HY2_RAW_URL.*insecure=1" "real protocol lab HY2 must not generate insecure URL"
require_literal "scripts/lib/prepare-real-protocol-matrix-endpoints/payload.sh" "allow_insecure_source = raw_allow_insecure if raw_url else allow_insecure" "real protocol endpoint HY2 raw URL ignores stale insecure env"
require_literal "$REAL_PROTOCOL_MATRIX_ENV_VALIDATION" "require_hy2_tls_ready" "real protocol matrix env requires HY2 TLS readiness"
require_literal "$REAL_PROTOCOL_MATRIX_ENV_VALIDATION" "THIRD_PARTY_HY2_ALLOW_INSECURE must not be enabled" "real protocol matrix env rejects HY2 insecure"
require_literal "scripts/lib/prepare-real-protocol-matrix-endpoints/payload.sh" 'key = first_value("THIRD_PARTY_HTTP_KEY")' "real protocol endpoint payload separates stale HTTP key from raw URL auth"
require_literal "$REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST" "THIRD_PARTY_VLESS_RAW_URL" "real protocol endpoint payload test covers VLESS raw URL precedence"
require_literal "$REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST" 'assert_json_value "$vless_payload" "outbound_config.uuid"' "real protocol endpoint payload test asserts VLESS UUID from raw URL"
require_literal "$REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST" 'assert_json_absent "$http_payload" "outbound_config.username"' "real protocol endpoint payload test rejects stale HTTP split auth"
require_literal "$REAL_PROTOCOL_MATRIX_ENDPOINT_PAYLOAD_TEST" 'assert_json_absent "$hy2_payload" "outbound_config.allow_insecure"' "real protocol endpoint payload test rejects stale HY2 insecure split env"
require_literal "$REAL_THIRD_PARTY_REDACTION_TEST" "listen_host" "real third-party redaction test covers user subscription API ingress host"
require_literal "$REAL_THIRD_PARTY_REDACTION_TEST" "sni: relay.example.test" "real third-party redaction test rejects sensitive SNI collision"
require_literal "$MAKEFILE" "bash scripts/test-real-third-party-redaction.sh" "make check runs real third-party redaction regression"
require_literal "$REAL_RELAY_MATRIX" "socks,http,vless,trojan,shadowsocks,hy2" "real relay third-party protocol matrix"
require_literal "$REAL_RELAY_MATRIX" 'USER_ACCOUNT="${USER_LOGIN_ACCOUNT:-${XRAYC_REAL_E2E_USER_ACCOUNT:-${E2E_USER_ACCOUNT:-}}}"' "real relay user login env precedence"
require_literal "$REAL_RELAY_MATRIX" 'ADMIN_ACCOUNT="${ADMIN_LOGIN_ACCOUNT:-${XRAYC_REAL_E2E_ADMIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}}"' "real relay admin login env precedence"
require_literal "$REAL_RELAY_MATRIX" '"exit_endpoint_id": os.environ["EXIT_ENDPOINT_ID_VALUE"]' "real relay entry uses line endpoint binding"
require_literal "$REAL_RELAY_MATRIX" "select_initial_relay_endpoint_id" "real relay selects a subscription-eligible initial endpoint"
require_literal "$REAL_RELAY_MATRIX" "assert_protocol_endpoint_reachable_from_relay" "real relay probes upstream endpoints before traffic loop"
require_absent_regex "$REAL_RELAY_MATRIX" '"line_group_id":[[:space:]]*os\.environ\["LINE_GROUP_ID_VALUE"\]' "real relay entry old group binding"
require_literal "$REAL_RELAY_MATRIX" "XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS=1" "real relay agent install keeps pre-created node credentials"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "subscription must contain exactly one matching relay proxy" "real relay subscription matches temporary proxy"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "user subscription api must contain exactly one matching access line" "real relay user subscription matches temporary line"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "unique_names = {" "real relay user subscription de-duplicates multi-group access line matches"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "assert_relay_subscription_eligibility" "real relay checks backend eligibility before subscription download"
require_absent_regex "$REAL_RELAY_MATRIX_REDACTION" "len\\(proxies\\)[[:space:]]*!=[[:space:]]*1" "real relay must not require whole subscription to contain one proxy"
require_absent_regex "$REAL_RELAY_MATRIX_REDACTION" "len\\(access_lines\\)[[:space:]]*!=[[:space:]]*1" "real relay must not require whole user subscription to contain one access line"
require_literal "$REAL_RELAY_MATRIX_REDACTION" "subscription leaked upstream endpoint material" "real relay upstream leak guard"
require_literal "$REAL_RELAY_MATRIX" "assert_protocol_eviction_cycle" "real relay disable and quota eviction"
require_literal "scripts/lib/real-access-third-party-matrix-relay/assertions.sh" "desired_config_hash = NULL" "real relay dirty marks force desired config recomputation"
require_absent_regex "scripts/lib/real-access-third-party-matrix-relay/common.sh" "admin123456|demo123456" "real relay release credential gate must not hard-code demo password rejection"
require_literal "scripts/lib/real-access-third-party-matrix-relay/assertions.sh" 'deadline="$(($(date +%s) + timeout))"' "real relay protocol wait uses wall clock timeout"
require_literal "scripts/lib/real-access-third-party-matrix-relay/client.sh" "CLIENT_PUBLIC_IP_CURL_TIMEOUT_SECONDS:-8" "real relay public IP curl timeout is bounded"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "xrayc_matrix_active_hint" "real inbound matrix detects whether preferred access node has a recent heartbeat"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "xrayc_matrix_direct_access_nodes" "real inbound matrix SQL keeps Cloudflare targets out of DB fallback"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "JOIN xrayc_matrix_direct_access_nodes dn ON dn.id = n.id" "real inbound matrix SQL fallback only selects direct access nodes"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "orange cloud" "real inbound matrix SQL recognizes orange cloud role marker"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'" "real inbound matrix avoids nodes with failed limiter config"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "OR NOT (SELECT matched FROM xrayc_matrix_active_hint)" "real inbound matrix falls back when preferred access node is stale"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "desired_config_hash = NULL" "real inbound matrix dirty marks force desired config recomputation"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "e.last_probe_status = 'healthy'" "real inbound matrix skips unhealthy preferred exit endpoints"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "r.status = 'healthy'" "real inbound matrix requires healthy preferred exit resources"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "LEFT JOIN access_exit_probe_states preferred_ps" "real inbound matrix checks preferred exit against the selected access node"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "LEFT JOIN access_exit_probe_states template_ps" "real inbound matrix checks template exit against the selected access node"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_SQL" "COALESCE(preferred_ps.effective_status, 'healthy') <> 'offline'" "real inbound matrix skips preferred exits offline for the selected access node"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_CONTEXT" "REAL_ACCESS_INBOUND_MATRIX_ENABLE_THIRD_PARTY_SOCKS" "real inbound matrix does not let global third-party socks env hijack tests"
require_literal "$REAL_INBOUND_MATRIX_DB_PREPARE_CONTEXT" 'DB_PREPARE_PREFERRED_EXIT_ENDPOINT_ID="${REAL_ACCESS_INBOUND_MATRIX_EXIT_ENDPOINT_ID:-}"' "real inbound matrix only uses explicitly requested preferred exit endpoint"
require_literal "$REAL_INBOUND_MATRIX_INVENTORY_SSH" "reject_cloudflare_inventory_target" "real inbound matrix rejects Cloudflare targets for ordinary high-port TCP matrix"
require_literal "$REAL_INBOUND_MATRIX_CLIENT_CONFIGS" "cdn_tunnel_proxy" "real inbound matrix client config skips CDN tunnel entries"
require_literal "$REAL_INBOUND_MATRIX_CLIENT_CONFIGS" "cloudflare_marked_proxy" "real inbound matrix client config skips Cloudflare-marked entries"
require_literal "$REAL_INBOUND_MATRIX_CLIENT_CONFIGS" "matrix_direct_proxy" "real inbound matrix client config prefers temporary matrix entries"
require_literal "$REAL_INBOUND_MATRIX_CLIENT_CONFIGS" "server_4" "real inbound matrix client config rejects server_4 subscription entries"
require_literal "$MAKEFILE" "scripts/test-real-access-inbound-matrix-static.sh" "make check runs real inbound matrix Cloudflare exclusion regression"
require_literal "$REAL_SUBSCRIPTION_CLIENT_TRAFFIC" "stage_remote_subscription_client_configs" "real subscription client traffic stages remote configs through controlled status"
require_literal "$REAL_SUBSCRIPTION_CLIENT_TRAFFIC" "remote subscription traffic setup failed; raw output redacted" "real subscription client traffic redacts remote setup failures"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" "select_active_inventory_access_target" "real inbound matrix selects an active inventory target before setting TLS listen host"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" 'ACCESS_TARGET="$selected_access_target"' "real inbound matrix opens firewall on the selected active target"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" "inventory_tls_domain_for_access_target" "real inbound matrix uses selected active target TLS domain for temporary trojan inbound"
require_literal_order "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" 'reject_cloudflare_inventory_target "$access_node_hint" "access"' 'status "preparing_missing_inbound_protocols"' 'bash "$DB_PREPARE_SCRIPT" prepare' "real inbound matrix rejects Cloudflare access target before DB prepare"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PREPARE" "Do not use the stale default target to choose a TLS listen host" "real inbound matrix does not force stale default target listen host"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "temporary inbound access target does not match prepared access node" "real inbound matrix rejects explicit access target mismatch"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "wait_prepared_access_ports_listening" "real inbound matrix verifies prepared remote ports before client traffic"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "prepared_udp_access_ports" "real inbound matrix separates UDP/HY2 prepared ports"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "iptables -I INPUT -p udp" "real inbound matrix opens prepared UDP ports"
require_literal "$REAL_INBOUND_MATRIX_SUBSCRIPTION_PORTS" "wait_prepared_access_udp_ports_listening" "real inbound matrix waits for prepared UDP ports"
require_literal "scripts/lib/real-access-inbound-matrix/remote-client.sh" "retry_remote_exec_stdin" "real inbound matrix retries SSH stdin uploads"
require_literal "scripts/lib/real-access-inbound-matrix/remote-client.sh" "retry_remote_bash_file" "real inbound matrix retries SSH remote scripts"
require_literal "scripts/lib/real-access-inbound-matrix/remote-client.sh" "REAL_ACCESS_INBOUND_MATRIX_REMOTE_RETRIES" "real inbound matrix remote retries are configurable for real timeout debugging"
require_literal "scripts/lib/real-access-inbound-matrix/remote-client.sh" "REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_ATTEMPTS" "real inbound matrix traffic attempts are configurable for real timeout debugging"
require_literal "$PLAN_DOC" "普通入站矩阵拒绝 Cloudflare/橙云目标" "plan documents ordinary inbound matrix Cloudflare exclusion"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "RUNTIME_LOADTEST_FORBID_DATABASE_URL" "runtime loadtest database bootstrap forbids control database reuse"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "runtime loadtest database name must contain loadtest" "runtime loadtest database bootstrap enforces loadtest database name"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "runtime loadtest database ready" "runtime loadtest database bootstrap fast path"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "runtime loadtest database ready: host=%s port=%s db=%s user=%s" "runtime loadtest database bootstrap redacted output"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "CREATE ROLE" "runtime loadtest database bootstrap can create isolated role"
require_literal "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "CREATE DATABASE" "runtime loadtest database bootstrap can create isolated database"
require_literal_order "$RUNTIME_LOADTEST_DB_BOOTSTRAP" "CREATE ROLE" "CREATE DATABASE" "runtime loadtest database bootstrap migrating" "runtime loadtest database bootstrap creates role and database before migration"
require_literal "$RUNTIME_HTTP_LOADTEST" 'output="$(curl "${args[@]}" "${base_url}${path}" 2>/dev/null || true)"' "runtime HTTP loadtest captures curl result once"
require_literal "$RUNTIME_HTTP_LOADTEST" "printf '000 0" "runtime HTTP loadtest has a single fallback failed result"
require_absent_regex "$RUNTIME_HTTP_LOADTEST" "curl .*\\|\\| printf '000 0" "runtime HTTP loadtest must not double-count curl failures"
require_literal "$RUNTIME_HTTP_LOADTEST" "RUNTIME_HTTP_LOADTEST_AGENT_TRAFFIC_MAX_AVG_MS" "runtime HTTP loadtest uses a dedicated agent traffic latency budget"
require_literal "$DB_AGENT_SESSIONS" "ON CONFLICT (access_node_id, access_line_id, xray_user_key, client_ip_hash)" "agent sessions runtime path upserts online sessions idempotently"
require_literal "$DB_AGENT_SESSIONS" "LEFT JOIN users u ON u.xray_user_key = incoming.xray_user_key" "agent sessions runtime path resolves users in bulk"
require_literal "$REAL_V2_RELAY_POOL" "run_user_udp_rate_limit_e2e" "real V2 relay pool UDP shared user rate limit E2E"
require_literal "$REAL_V2_RELAY_POOL" "TCP_TARGET_PORT" "real V2 relay pool has controlled TCP target port"
require_literal "$REAL_V2_RELAY_POOL_EXIT_HOSTS" "setup_tcp_rate_target" "real V2 relay pool starts controlled TCP rate target"
require_literal "$REAL_V2_RELAY_POOL_EXIT_HOSTS" "tcp_rate_target_ready" "real V2 relay pool verifies controlled TCP target readiness"
require_literal "$REAL_V2_RELAY_POOL_RATE_LIMIT" 'controlled_tcp_target_url="http://${EXIT_B_PUBLIC_HOST}:${TCP_TARGET_PORT}/bytes?size=${bytes}"' "real V2 relay pool default rate probe uses controlled TCP target"
require_literal "$REAL_V2_RELAY_POOL" "/local-exit-lines" "real V2 relay pool creates self-hosted local exit lines through access-node API"
require_literal "$REAL_V2_RELAY_POOL" '"stream_config": {"udp_packet_encoding": "xudp"}' "real V2 relay pool local VLESS exits enable XUDP UDP carriage"
require_literal "$REAL_V2_RELAY_POOL_TARGETS" "EXIT_B_UDP_TARGET_HOST" "real V2 relay pool uses an IP literal target for UDP dokodemo-door"
require_literal "$REAL_V2_RELAY_POOL_CLIENT" 'UDP_TARGET_HOST="${EXIT_B_UDP_TARGET_HOST:-$EXIT_B_PUBLIC_HOST}"' "real V2 relay pool client UDP target avoids public-domain DNS dependency"
require_literal "$REAL_V2_RELAY_POOL" '"protocol": "vless", "transport": "tcp"' "real V2 relay pool uses VLESS/TCP access for TCP-reachable test servers"
require_absent_regex "$REAL_V2_RELAY_POOL" '"protocol": "shadowsocks", "network_mode": "udp"' "real V2 relay pool must not require native UDP ingress between test servers"
require_literal "$REAL_V2_RELAY_POOL" "local_exit_lines_created count=2 ownership=self_hosted" "real V2 relay pool self-hosted local exit status"
require_literal "$REAL_V2_RELAY_POOL" "IFS=\$'\\t' read -r exit_endpoint_a exit_endpoint_b" "real V2 relay pool parses both local exit endpoint ids under strict IFS"
require_literal "$REAL_V2_RELAY_POOL" "er.ownership = 'self_hosted'" "real V2 relay pool asserts local exit ownership"
require_literal "$REAL_V2_RELAY_POOL" "er.access_node_id = :'access_node_id'::uuid" "real V2 relay pool asserts local exit access node ownership"
require_absent_regex "$REAL_V2_RELAY_POOL" "create_pool_vless_exit" "real V2 relay pool must not use third-party pool endpoint helper for local exits"
require_literal "$REAL_V2_RELAY_POOL" '/api/admin/access-entries/${access_entry_id}/exit-bindings' "real V2 relay pool creates entry exit bindings through new API"
require_literal "$REAL_V2_RELAY_POOL" "line_group_bound_binding_node count=1" "real V2 relay pool binds self-hosted entry binding nodes"
require_literal "$REAL_V2_RELAY_POOL" "second_exit_egress_ok" "real V2 relay pool verifies second self-hosted exit traffic"
require_literal "$REAL_V2_RELAY_POOL" 'activate_line_group_binding_node "$access_line_id_b" "$pool_id_b" "second-exit-only"' "real V2 relay pool switches line group to second entry binding"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_assigned_exit_endpoint "second-exit-only"' "real V2 relay pool syncs assignment before second self-hosted exit traffic"
require_literal "$REAL_V2_RELAY_POOL" 'activate_line_group_binding_node "$access_line_id_a" "$pool_id_a" "first-exit-restored"' "real V2 relay pool switches line group back to first entry binding"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_assigned_exit_endpoint "first-exit-restored"' "real V2 relay pool refreshes assignments after concrete line restore"
require_literal "$REAL_V2_RELAY_POOL_REBIND" "DELETE FROM user_access_line_assignments" "real V2 relay pool rebind clears stale access assignments"
require_literal "$REAL_V2_RELAY_POOL_REBIND" "DELETE FROM user_exit_assignments" "real V2 relay pool rebind clears stale exit assignments"
require_literal "$REAL_V2_RELAY_POOL_FUNCTIONS" "assignment_resync_ok reason=" "real V2 relay pool assignment refresh status"
require_literal "$REAL_V2_RELAY_POOL_FUNCTIONS" "refresh_subscription_client_runtime" "real V2 relay pool can rebuild client from refreshed subscription"
require_literal "$REAL_V2_RELAY_POOL_SUBSCRIPTION" "relay proxy match count is" "real V2 relay pool subscription parser requires unique proxy match"
require_literal "$REAL_V2_RELAY_POOL_RATE_LIMIT" 'refresh_subscription_client_runtime "rate-limit-applied"' "real V2 relay pool rebuilds client after user rate limit changes runtime port"
require_literal "$REAL_V2_RELAY_POOL_FUNCTIONS" "disable_access_line_for_eviction" "real V2 relay pool can remove an access line from runtime"
require_function_absent_literal "$REAL_V2_RELAY_POOL_FUNCTIONS" "disable_access_line_for_eviction" "updated_at = now()" "real V2 relay pool access-line disable must match access_lines schema"
require_function_absent_literal "$REAL_V2_RELAY_POOL_FUNCTIONS" "restore_access_line_after_eviction" "updated_at = now()" "real V2 relay pool access-line restore must match access_lines schema"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_subscription_client_runtime "access-line-restored"' "real V2 relay pool rebuilds client after access line restore"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_subscription_client_runtime "disabled-user"' "real V2 relay pool rebuilds client after disabled user restore"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_subscription_client_runtime "quota-exhausted"' "real V2 relay pool rebuilds client after quota restore"
require_literal "$REAL_V2_RELAY_POOL" 'refresh_subscription_client_runtime "line-group-restored"' "real V2 relay pool rebuilds client after line group restore"
require_literal "$REAL_V2_RELAY_POOL_RATE_LIMIT" "user_udp_rate_limit_shared_class_ok" "real V2 relay pool UDP shared class assertion"
require_literal "$REAL_V2_RELAY_POOL_CLIENT_CONFIG_TEST" "outbound[\"protocol\"] == \"shadowsocks\"" "real V2 relay pool Shadowsocks client config test"
require_regex "$REAL_LEDGER_LIB" "traffic_source[[:space:]]*=[[:space:]]*'access_line'" "ledger source access_line verification"

require_literal "$TEST_DOC" "Playwright smoke" "test document Playwright smoke coverage"
require_literal "$TEST_DOC" "runtime-no-mock" "test document runtime no-mock coverage"
require_literal "$TEST_DOC" "make check-real-release" "test document real release E2E coverage"
require_literal "$TEST_DOC" "订阅只暴露中转入口" "test document subscription access-line rule"
require_literal "$TEST_DOC" "运行态不是 mock" "test document runtime no-mock rule"
require_literal "$TEST_DOC" "VLESS/TCP 用户入口订阅和 VLESS/XUDP 出口管理出口 endpoint" "test document current real V2 relay pool path"
require_literal "$TEST_DOC" "TCP+UDP 共享用户限速" "test document shared TCP/UDP user rate limit"
require_literal "$PLAN_DOC" "VLESS/TCP 用户入口订阅和 VLESS/XUDP 出口 endpoint" "plan current real V2 relay pool path"
require_literal "$PLAN_DOC" "文档版本：v4.34" "plan document current version"
require_literal "$RATE_LIMIT_PLAN_DOC" "TCP+UDP shared bandwidth limiting" "rate-limit plan shared TCP/UDP goal"
require_literal "$RATE_LIMIT_SPEC_DOC" "TCP+UDP shared smooth limiting" "rate-limit spec shared TCP/UDP design"
require_absent_regex "$RATE_LIMIT_SPEC_DOC" "UDP traffic is not a first-version precision target" "old TCP-only rate-limit wording"
require_literal "$REMOVED_CLIENT_ENDPOINT_TEST" '"/api/client/devices/register"' "removed client API route guard"
require_literal "$REMOVED_CLIENT_ENDPOINT_TEST" "StatusCode::NOT_FOUND" "removed client API route status"


# 发布验证不再隐式启动或替换控制面；实际 UAT 仍可在明确的测试资产上写入。
real_check_recipe="$(awk '/^check-real-release:/ {copy=1;next} copy && /^[^[:space:]]/ {copy=0} copy {print}' "$MAKEFILE")"
if printf '%s\n' "$real_check_recipe" | grep -Eq 'compose.*(up|down|restart)|--remove-orphans'; then
  fail 'real release check must not mutate the deployed control plane'
fi

if [[ "$failures" -gt 0 ]]; then
  printf 'v2-mainline-coverage: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi
printf 'v2-mainline-coverage: passed\n'
