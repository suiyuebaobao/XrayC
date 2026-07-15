#!/usr/bin/env bash
# 这个脚本执行 API 合约冒烟检查的主流程。
# 通用请求、断言和响应结构校验逻辑拆分到 helper 中。
# 此入口只负责读取环境变量、编排检查步骤并汇总失败数。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" bash scripts/api-contract-smoke-draft.sh

Optional environment variables:
  USER_ACCESS_TOKEN   Optional user JWT for read-only user endpoint checks.
  ADMIN_ACCESS_TOKEN  Optional admin JWT for read-only admin endpoint checks.
  CONTRACT_USER_ACCOUNT / CONTRACT_USER_PASSWORD
                      Optional credentials used to derive USER_ACCESS_TOKEN.
  CONTRACT_ADMIN_ACCOUNT / CONTRACT_ADMIN_PASSWORD
                      Optional credentials used to derive ADMIN_ACCESS_TOKEN.
  SUBSCRIPTION_TOKEN  Optional subscription token for /sub/{token} download.
  SAFE_PROBE_TRIGGER  Set to 1 to queue a non-network admin probe task.
  PROBE_EXIT_ENDPOINT_ID Required with SAFE_PROBE_TRIGGER=1.
  CURL_TIMEOUT        Curl timeout seconds, default: 15.
  STRICT_CONTRACT     Set to 1 to fail when checked routes are missing.

This draft smoke is read-only unless SAFE_PROBE_TRIGGER=1 is set. The probe
check only queues a backend task for access-agent pickup; it does not dial an
external exit from this script. It never prints token values or response bodies.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

BASE_URL="${BASE_URL:-}"
CURL_TIMEOUT="${CURL_TIMEOUT:-15}"
STRICT_CONTRACT="${STRICT_CONTRACT:-0}"

if [[ -z "$BASE_URL" ]]; then
  echo "BASE_URL is required." >&2
  exit 2
fi

if [[ "$BASE_URL" != https://* && "$BASE_URL" != http://localhost* && "$BASE_URL" != http://127.0.0.1* && "${ALLOW_INSECURE_HTTP_E2E:-0}" != "1" ]]; then
  echo "BASE_URL must use HTTPS, except local development origins." >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

failures=0
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/api-contract-smoke-draft/core.sh
source "${script_dir}/lib/api-contract-smoke-draft/core.sh"

derive_access_token_if_needed \
  "user" \
  "${CONTRACT_USER_ACCOUNT:-${E2E_USER_ACCOUNT:-}}" \
  "${CONTRACT_USER_PASSWORD:-${E2E_USER_PASSWORD:-}}" \
  USER_ACCESS_TOKEN
derive_access_token_if_needed \
  "admin" \
  "${CONTRACT_ADMIN_ACCOUNT:-${E2E_ADMIN_ACCOUNT:-}}" \
  "${CONTRACT_ADMIN_PASSWORD:-${E2E_ADMIN_PASSWORD:-}}" \
  ADMIN_ACCESS_TOKEN

body="$tmp_dir/health.body"
status_file="$tmp_dir/health.status"
request_get "health" "/health" "" "$body" "$status_file"
assert_2xx "health" "$(cat "$status_file")"

body="$tmp_dir/auth-security.body"
status_file="$tmp_dir/auth-security.status"
request_get "auth security" "/api/auth/security" "" "$body" "$status_file"
assert_2xx "auth security" "$(cat "$status_file")"
assert_no_sensitive_fields "auth security" "$body"

body="$tmp_dir/plans.body"
status_file="$tmp_dir/plans.status"
request_get "plans" "/api/plans" "" "$body" "$status_file"
assert_2xx "plans" "$(cat "$status_file")"
assert_public_plans_contract "plans" "$body"

body="$tmp_dir/user-subscription-unauth.body"
status_file="$tmp_dir/user-subscription-unauth.status"
request_get "user subscription without token" "/api/user/subscription" "" "$body" "$status_file"
assert_auth_boundary_or_missing "user subscription without token" "$(cat "$status_file")"

body="$tmp_dir/orders-unauth.body"
status_file="$tmp_dir/orders-unauth.status"
request_get "orders without token" "/api/orders" "" "$body" "$status_file"
assert_auth_boundary_or_missing "orders without token" "$(cat "$status_file")"

body="$tmp_dir/admin-redeem-codes-unauth.body"
status_file="$tmp_dir/admin-redeem-codes-unauth.status"
request_get "admin redeem codes without token" "/api/admin/redeem-codes" "" "$body" "$status_file"
assert_auth_boundary_or_missing "admin redeem codes without token" "$(cat "$status_file")"

body="$tmp_dir/admin-plans-unauth.body"
status_file="$tmp_dir/admin-plans-unauth.status"
request_get "admin plans without token" "/api/admin/plans" "" "$body" "$status_file"
assert_auth_boundary_or_missing "admin plans without token" "$(cat "$status_file")"

body="$tmp_dir/admin-exit-resources-unauth.body"
status_file="$tmp_dir/admin-exit-resources-unauth.status"
request_get "admin exit resources without token" "/api/admin/exit-resources" "" "$body" "$status_file"
if assert_optional_contract_response "admin exit resources without token" "$(cat "$status_file")"; then
  assert_admin_collection_contract \
    "admin exit resources without token" \
    "$body" \
    "exit_resources" \
    id name region_code provider_name ownership enabled status created_at
  assert_no_sensitive_fields "admin exit resources without token" "$body"
fi

body="$tmp_dir/admin-exit-endpoints-unauth.body"
status_file="$tmp_dir/admin-exit-endpoints-unauth.status"
request_get "admin exit endpoints without token" "/api/admin/exit-endpoints" "" "$body" "$status_file"
if assert_optional_contract_response "admin exit endpoints without token" "$(cat "$status_file")"; then
    assert_admin_collection_contract \
      "admin exit endpoints without token" \
      "$body" \
      "exit_endpoints" \
      id exit_resource_id exit_resource_name name outbound_type host host_redacted port enabled exit_resource_enabled created_at
fi

body="$tmp_dir/admin-exit-pools-unauth.body"
status_file="$tmp_dir/admin-exit-pools-unauth.status"
request_get "admin exit pools without token" "/api/admin/exit-pools" "" "$body" "$status_file"
if assert_optional_contract_response "admin exit pools without token" "$(cat "$status_file")"; then
  assert_admin_collection_contract \
    "admin exit pools without token" \
    "$body" \
    "exit_pools" \
    id uuid name region_code status healthy_members total_members strategy active_assignments
  assert_no_sensitive_fields "admin exit pools without token" "$body"
fi

body="$tmp_dir/access-operations-summary-unauth.body"
status_file="$tmp_dir/access-operations-summary-unauth.status"
request_get "access operations summary without token" "/api/admin/access-operations/summary" "" "$body" "$status_file"
if assert_optional_contract_response "access operations summary without token" "$(cat "$status_file")"; then
  assert_operations_summary_contract "access operations summary without token" "$body"
  assert_no_sensitive_fields "access operations summary without token" "$body"
fi

body="$tmp_dir/access-operations-ledger-ranking-unauth.body"
status_file="$tmp_dir/access-operations-ledger-ranking-unauth.status"
request_get "access operations ledger ranking without token" "/api/admin/access-operations/ledger-ranking?limit=10" "" "$body" "$status_file"
if assert_optional_contract_response "access operations ledger ranking without token" "$(cat "$status_file")"; then
  assert_ledger_ranking_contract "access operations ledger ranking without token" "$body"
  assert_no_sensitive_fields "access operations ledger ranking without token" "$body"
fi

body="$tmp_dir/access-operations-settings-unauth.body"
status_file="$tmp_dir/access-operations-settings-unauth.status"
request_get "access operations settings without token" "/api/admin/access-operations/settings" "" "$body" "$status_file"
if assert_optional_contract_response "access operations settings without token" "$(cat "$status_file")"; then
  assert_access_operations_settings_contract "access operations settings without token" "$body"
  assert_no_sensitive_fields "access operations settings without token" "$body"
fi

if [[ -n "${SUBSCRIPTION_TOKEN:-}" ]]; then
  body="$tmp_dir/subscription-download.body"
  status_file="$tmp_dir/subscription-download.status"
  request_get "subscription download" "/sub/${SUBSCRIPTION_TOKEN}" "" "$body" "$status_file"
  if status_is_2xx "$(cat "$status_file")"; then
    echo "subscription download: HTTP $(cat "$status_file")"
    assert_subscription_yaml_contract "subscription download" "$body"
  else
    record_failure "subscription download: expected 2xx with provided SUBSCRIPTION_TOKEN, got HTTP $(cat "$status_file")"
  fi
else
  body="$tmp_dir/subscription-download-invalid.body"
  status_file="$tmp_dir/subscription-download-invalid.status"
  request_get "subscription download invalid token" "/sub/xrayc-contract-smoke-invalid-token" "" "$body" "$status_file"
  if status_is_missing "$(cat "$status_file")" || status_is_auth_reject "$(cat "$status_file")"; then
    echo "subscription download invalid token: reject boundary present, HTTP $(cat "$status_file")"
  else
    record_failure "subscription download invalid token: expected 401/403/404/405 without a real token, got HTTP $(cat "$status_file")"
  fi
fi

body="$tmp_dir/subscription-legacy-query.body"
status_file="$tmp_dir/subscription-legacy-query.status"
request_get "subscription legacy query" "/sub/xrayc-contract-smoke-invalid-token?target=base64" "" "$body" "$status_file"
if [[ "$(cat "$status_file")" == "400" ]]; then
  echo "subscription legacy query: rejected as expected"
else
  record_failure "subscription legacy query: expected HTTP 400 for legacy format parameters, got HTTP $(cat "$status_file")"
fi

if [[ -n "${USER_ACCESS_TOKEN:-}" ]]; then
  body="$tmp_dir/user-subscription.body"
  status_file="$tmp_dir/user-subscription.status"
  request_get "user subscription with token" "/api/user/subscription" "$USER_ACCESS_TOKEN" "$body" "$status_file"
  assert_optional_token_read "user subscription with token" "$(cat "$status_file")"
  assert_no_sensitive_fields "user subscription with token" "$body"

  body="$tmp_dir/orders.body"
  status_file="$tmp_dir/orders.status"
  request_get "orders with token" "/api/orders" "$USER_ACCESS_TOKEN" "$body" "$status_file"
  assert_optional_token_read "orders with token" "$(cat "$status_file")"
else
  if [[ "$STRICT_CONTRACT" == "1" ]]; then
    record_failure "authenticated user reads require USER_ACCESS_TOKEN or CONTRACT_USER_* in strict mode"
  else
    echo "Skipping authenticated user reads: USER_ACCESS_TOKEN not provided."
  fi
fi

if [[ -n "${ADMIN_ACCESS_TOKEN:-}" ]]; then
  body="$tmp_dir/admin-plans.body"
  status_file="$tmp_dir/admin-plans.status"
  request_get "admin plans with token" "/api/admin/plans" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  assert_optional_token_read "admin plans with token" "$(cat "$status_file")"
  assert_no_sensitive_fields "admin plans with token" "$body"

  body="$tmp_dir/admin-redeem-codes.body"
  status_file="$tmp_dir/admin-redeem-codes.status"
  request_get "admin redeem codes with token" "/api/admin/redeem-codes" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  assert_optional_token_read "admin redeem codes with token" "$(cat "$status_file")"

  body="$tmp_dir/admin-exit-resources.body"
  status_file="$tmp_dir/admin-exit-resources.status"
  request_get "admin exit resources with token" "/api/admin/exit-resources" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "admin exit resources with token" "$(cat "$status_file")"; then
    assert_admin_collection_contract \
      "admin exit resources with token" \
      "$body" \
      "exit_resources" \
      id name region_code provider_name ownership enabled status created_at
    assert_no_sensitive_fields "admin exit resources with token" "$body"
  fi

  body="$tmp_dir/admin-exit-endpoints.body"
  status_file="$tmp_dir/admin-exit-endpoints.status"
  request_get "admin exit endpoints with token" "/api/admin/exit-endpoints" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "admin exit endpoints with token" "$(cat "$status_file")"; then
    assert_admin_collection_contract \
      "admin exit endpoints with token" \
      "$body" \
      "exit_endpoints" \
      id exit_resource_id exit_resource_name name outbound_type host host_redacted port enabled exit_resource_enabled created_at
  fi

  body="$tmp_dir/admin-exit-pools.body"
  status_file="$tmp_dir/admin-exit-pools.status"
  request_get "admin exit pools with token" "/api/admin/exit-pools" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "admin exit pools with token" "$(cat "$status_file")"; then
    assert_admin_collection_contract \
      "admin exit pools with token" \
      "$body" \
      "exit_pools" \
      id uuid name region_code status healthy_members total_members strategy active_assignments
    assert_no_sensitive_fields "admin exit pools with token" "$body"
  fi

  body="$tmp_dir/access-operations-summary.body"
  status_file="$tmp_dir/access-operations-summary.status"
  request_get "access operations summary with token" "/api/admin/access-operations/summary" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "access operations summary with token" "$(cat "$status_file")"; then
    assert_operations_summary_contract "access operations summary with token" "$body"
    assert_no_sensitive_fields "access operations summary with token" "$body"
  fi

  body="$tmp_dir/access-operations-ledger-ranking.body"
  status_file="$tmp_dir/access-operations-ledger-ranking.status"
  request_get "access operations ledger ranking with token" "/api/admin/access-operations/ledger-ranking?limit=10" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "access operations ledger ranking with token" "$(cat "$status_file")"; then
    assert_ledger_ranking_contract "access operations ledger ranking with token" "$body"
    assert_no_sensitive_fields "access operations ledger ranking with token" "$body"
  fi

  body="$tmp_dir/access-operations-settings.body"
  status_file="$tmp_dir/access-operations-settings.status"
  request_get "access operations settings with token" "/api/admin/access-operations/settings" "$ADMIN_ACCESS_TOKEN" "$body" "$status_file"
  if assert_optional_contract_response "access operations settings with token" "$(cat "$status_file")"; then
    assert_access_operations_settings_contract "access operations settings with token" "$body"
    assert_no_sensitive_fields "access operations settings with token" "$body"
    settings_payload="$(
      python3 - "$body" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
data = payload.get("data", payload)
if not isinstance(data, dict):
    data = {}
probe = data.get("probe_policy") if isinstance(data.get("probe_policy"), dict) else {}
retention = data.get("traffic_log_retention") if isinstance(data.get("traffic_log_retention"), dict) else {}
backup = data.get("database_backup") if isinstance(data.get("database_backup"), dict) else {}
print(json.dumps({
    "probe_policy": probe,
    "traffic_log_retention": {
        "detail_retention_days": int(retention.get("detail_retention_days", 14)),
        "prune_enabled": bool(retention.get("prune_enabled", True)),
        "delete_batch_size": int(retention.get("delete_batch_size", 5000)),
    },
    "database_backup": {
        "enabled": bool(backup.get("enabled", True)),
        "interval_days": int(backup.get("interval_days", 1)),
        "retention_days": int(backup.get("retention_days", 30)),
    },
}))
PY
    )"
    body="$tmp_dir/access-operations-settings-put.body"
    status_file="$tmp_dir/access-operations-settings-put.status"
    request_put_json \
      "access operations settings retention put with token" \
      "/api/admin/access-operations/settings" \
      "$ADMIN_ACCESS_TOKEN" \
      "$settings_payload" \
      "$body" \
      "$status_file"
    if status_is_2xx "$(cat "$status_file")"; then
      assert_access_operations_settings_contract "access operations settings retention put with token" "$body"
      assert_no_sensitive_fields "access operations settings retention put with token" "$body"
    else
      record_failure "access operations settings retention put with token: expected 2xx response, got HTTP $(cat "$status_file")"
    fi
  fi

  if [[ "${SAFE_PROBE_TRIGGER:-0}" == "1" ]]; then
    if [[ -z "${PROBE_EXIT_ENDPOINT_ID:-}" ]]; then
      record_failure "safe probe trigger: PROBE_EXIT_ENDPOINT_ID is required when SAFE_PROBE_TRIGGER=1"
    else
      body="$tmp_dir/access-operations-probe.body"
      status_file="$tmp_dir/access-operations-probe.status"
      request_post_json \
        "access operations probe trigger" \
        "/api/admin/access-operations/probe" \
        "$ADMIN_ACCESS_TOKEN" \
        "{\"exit_endpoint_id\":\"${PROBE_EXIT_ENDPOINT_ID}\"}" \
        "$body" \
        "$status_file"
      if status_is_2xx "$(cat "$status_file")"; then
        echo "access operations probe trigger: queued, HTTP $(cat "$status_file")"
        assert_no_sensitive_fields "access operations probe trigger" "$body"
      else
        record_failure "access operations probe trigger: expected 2xx queued response, got HTTP $(cat "$status_file")"
      fi
    fi
  else
    echo "Skipping probe trigger: SAFE_PROBE_TRIGGER is not 1."
  fi
else
  if [[ "$STRICT_CONTRACT" == "1" ]]; then
    record_failure "authenticated admin reads require ADMIN_ACCESS_TOKEN or CONTRACT_ADMIN_* in strict mode"
  else
    echo "Skipping authenticated admin reads: ADMIN_ACCESS_TOKEN not provided."
  fi
fi

if [[ "$failures" -gt 0 ]]; then
  echo "api-contract-smoke: failed with ${failures} finding(s)" >&2
  exit 1
fi

echo "api-contract-smoke: completed"
