#!/usr/bin/env bash
# 用途：对真实控制面执行基础 smoke 检查，覆盖健康、订阅和 agent API。
# 范围：验证 HTTP 端点、订阅下载、部署资产和可选运行时上报链路。
# 输入：需要 BASE_URL，并可提供订阅、登录、agent、access line 等私有变量。
# 输出：只打印端点名和粗粒度结果，不保存或展示任何秘密值。
# 依赖：使用 curl、临时目录和 Bash JSON/文本检查逻辑完成探测。
# 安全：curl 失败时隐藏 stderr，避免泄露带 token 的完整 URL。
# 约束：默认要求 HTTPS，只有本地或显式私有 HTTP E2E 才允许非 HTTPS。
# 行为：按可用环境选择执行订阅、agent traffic 和部署 artifact 检查。
# 失败：必需环境缺失、HTTP 非 2xx、内容不合规或严格检查失败会退出。
# 维护：新增 smoke 端点时需保持输出脱敏并更新帮助文本。
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  BASE_URL="https://example.com" bash scripts/real-smoke.sh

Optional environment variables:
  SUB_TOKEN          Subscription token from private environment.
  SUBSCRIPTION_URL  Full subscription URL from private environment.
  SMOKE_LOGIN_ACCOUNT   Optional account used to discover current subscription URL.
  SMOKE_LOGIN_PASSWORD  Optional password used to discover current subscription URL.
  AGENT_TOKEN        Access-agent token from private environment.
  ACCESS_NODE_ID     Access node UUID for agent endpoint checks.
  ACCESS_LINE_ID     Access line UUID for runtime metric/session/traffic checks.
  EXIT_ENDPOINT_ID   Exit endpoint UUID for access->exit probe checks.
  XRAY_USER_KEY      User stats key for traffic/session checks.
  STRICT_AGENT_TRAFFIC  Set to 1 to require agent token, access node, access line, and 2xx traffic report.
  DISABLE_SYNTHETIC_AGENT_POSTS  Set to 1 for release gates that must not submit scripted telemetry.
  REQUIRE_SUBSCRIPTION_DOWNLOAD  Set to 1 to fail when subscription URL is missing or returns non-2xx.
  SKIP_SUBSCRIPTION_DOWNLOAD     Set to 1 for deploy-artifact-only smoke checks.
  DEPLOY_ARTIFACT_TOKEN  Optional token for deploy artifact download checks.
  DEPLOY_ARTIFACT_FULL   Set to 1 to download image tarballs and verify gzip/sha256.
  CURL_TIMEOUT      Curl timeout seconds, default: 15.
  ALLOW_INSECURE_HTTP_E2E  Set to 1 only for private remote E2E environments without HTTPS.

This script never stores or prints secret values. It only prints endpoint names
and coarse check results.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

BASE_URL="${BASE_URL:-}"
CURL_TIMEOUT="${CURL_TIMEOUT:-15}"

if [[ -z "$BASE_URL" ]]; then
  echo "BASE_URL is required. Use an HTTPS origin from a private environment." >&2
  exit 2
fi

if [[ "$BASE_URL" != https://* && "$BASE_URL" != http://localhost* && "$BASE_URL" != http://127.0.0.1* && "${ALLOW_INSECURE_HTTP_E2E:-0}" != "1" ]]; then
  echo "BASE_URL must use HTTPS, except local development origins." >&2
  exit 2
fi

if [[ "${STRICT_AGENT_TRAFFIC:-0}" == "1" && "${DISABLE_SYNTHETIC_AGENT_POSTS:-0}" != "1" ]]; then
  if [[ -z "${AGENT_TOKEN:-}" || -z "${ACCESS_NODE_ID:-}" || -z "${ACCESS_LINE_ID:-}" ]]; then
    echo "STRICT_AGENT_TRAFFIC=1 requires AGENT_TOKEN, ACCESS_NODE_ID, and ACCESS_LINE_ID." >&2
    exit 2
  fi
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

curl_safe() {
  local label="$1"
  local url="$2"
  local output="$3"
  local status_file="$4"
  local err_file="$tmp_dir/curl.err"

  echo "Checking ${label}"
  if ! curl \
    --silent \
    --show-error \
    --location \
    --max-time "$CURL_TIMEOUT" \
    --output "$output" \
    --write-out "%{http_code}" \
    "$url" > "$status_file" 2> "$err_file"; then
    echo "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens." >&2
    exit 1
  fi
}

write_curl_bearer_config() {
  local token="$1"
  local config_file="$2"
  case "$token" in
    *$'\n'*|*$'\r'*)
      echo "authorization token must not contain newlines" >&2
      exit 2
      ;;
  esac
  local escaped="${token//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf 'header = "Authorization: Bearer %s"\n' "$escaped" >"$config_file"
  chmod 600 "$config_file"
}

curl_agent_post() {
  local label="$1"
  local path="$2"
  local body="$3"
  local output="$4"
  local status_file="$5"
  local err_file="$tmp_dir/curl-agent.err"
  local request_file="$tmp_dir/curl-agent-request.json"
  local auth_config="$tmp_dir/curl-agent-auth.conf"

  echo "Checking ${label}"
  printf '%s' "$body" > "$request_file"
  write_curl_bearer_config "$AGENT_TOKEN" "$auth_config"
  if ! curl \
    --silent \
    --show-error \
    --location \
    --max-time "$CURL_TIMEOUT" \
    --config "$auth_config" \
    --header "Content-Type: application/json" \
    --output "$output" \
    --write-out "%{http_code}" \
    --data-binary "@$request_file" \
    "${BASE_URL%/}${path}" > "$status_file" 2> "$err_file"; then
    echo "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens." >&2
    exit 1
  fi
}

curl_bearer_get() {
  local label="$1"
  local path="$2"
  local token="$3"
  local output="$4"
  local status_file="$5"
  local err_file="$tmp_dir/curl-bearer.err"
  local auth_config="$tmp_dir/curl-bearer-${BASHPID}-${RANDOM}.conf"

  echo "Checking ${label}"
  write_curl_bearer_config "$token" "$auth_config"
  if ! curl \
    --silent \
    --show-error \
    --location \
    --max-time "$CURL_TIMEOUT" \
    --config "$auth_config" \
    --output "$output" \
    --write-out "%{http_code}" \
    "${BASE_URL%/}${path}" > "$status_file" 2> "$err_file"; then
    echo "${label}: curl failed; stderr redacted to avoid leaking URLs or tokens." >&2
    exit 1
  fi
}

curl_json_post() {
  local label="$1"
  local path="$2"
  local body="$3"
  local output="$4"
  local status_file="$5"
  local err_file="$tmp_dir/curl-json-post.err"
  local request_file="$tmp_dir/curl-json-post-request.json"

  echo "Checking ${label}"
  printf '%s' "$body" > "$request_file"
  if ! curl \
    --silent \
    --show-error \
    --location \
    --max-time "$CURL_TIMEOUT" \
    --header "Content-Type: application/json" \
    --output "$output" \
    --write-out "%{http_code}" \
    --data-binary "@$request_file" \
    "${BASE_URL%/}${path}" > "$status_file" 2> "$err_file"; then
    echo "${label}: curl failed; stderr redacted to avoid leaking credentials." >&2
    exit 1
  fi
}

json_eval() {
  local expression="$1"
  local file="$2"
  JSON_FILE="$file" node -e "const fs=require('fs'); const r=JSON.parse(fs.readFileSync(process.env.JSON_FILE,'utf8')); ${expression}"
}

login_discovery_payload() {
  SMOKE_LOGIN_ACCOUNT="$SMOKE_LOGIN_ACCOUNT" \
  SMOKE_LOGIN_PASSWORD="$SMOKE_LOGIN_PASSWORD" \
  python3 - <<'PY'
import json
import os

print(json.dumps({
    "account": os.environ["SMOKE_LOGIN_ACCOUNT"],
    "password": os.environ["SMOKE_LOGIN_PASSWORD"],
}, separators=(",", ":")))
PY
}

assert_status_2xx_or_4xx() {
  local label="$1"
  local status="$2"
  case "$status" in
    2*|4*)
      echo "${label}: HTTP ${status}"
      ;;
    *)
      echo "${label}: unexpected HTTP ${status}" >&2
      exit 1
      ;;
  esac
}

assert_status_2xx() {
  local label="$1"
  local status="$2"
  case "$status" in
    2*)
      echo "${label}: HTTP ${status}"
      ;;
    *)
      echo "${label}: unexpected HTTP ${status}" >&2
      exit 1
      ;;
  esac
}

assert_status_exact() {
  local label="$1"
  local status="$2"
  local expected="$3"
  if [[ "$status" != "$expected" ]]; then
    echo "${label}: unexpected HTTP ${status}" >&2
    exit 1
  fi
  echo "${label}: HTTP ${status}"
}

assert_no_sensitive_subscription_leak() {
  local file="$1"

  if grep -Eiq '(vless|trojan|ss|ssr|hysteria2|hy2|socks|socks4|socks4a|socks5|socks5h)://[^[:space:]]+' "$file"; then
    echo "Subscription contains a full proxy URL; expected Clash/mihomo YAML only." >&2
    exit 1
  fi

  if grep -Eiq 'https?://[^[:space:]'\''"]+@[^[:space:]'\''"]+' "$file"; then
    echo "Subscription contains a credentialed HTTP proxy URL; expected Clash/mihomo YAML fields only." >&2
    exit 1
  fi

  if grep -Eiq '(outbound_proxy_url|agent_token|private[ _-]?key|secret_key|exit_endpoint)[[:space:]]*[:=][[:space:]]*[^[:space:]#]+' "$file"; then
    echo "Subscription contains a control-plane or upstream sensitive field." >&2
    exit 1
  fi
}

health_body="$tmp_dir/health.body"
health_status="$tmp_dir/health.status"
curl_safe "health" "${BASE_URL%/}/health" "$health_body" "$health_status"
assert_status_2xx "health" "$(cat "$health_status")"

auth_body="$tmp_dir/auth-security.body"
auth_status="$tmp_dir/auth-security.status"
curl_safe "auth security" "${BASE_URL%/}/api/auth/security" "$auth_body" "$auth_status"
assert_status_2xx "auth security" "$(cat "$auth_status")"

plans_body="$tmp_dir/plans.body"
plans_status="$tmp_dir/plans.status"
curl_safe "plans" "${BASE_URL%/}/api/plans" "$plans_body" "$plans_status"
assert_status_2xx "plans" "$(cat "$plans_status")"

if [[ -n "${DEPLOY_ARTIFACT_TOKEN:-}" ]]; then
  artifact_unauth_body="$tmp_dir/artifact-unauth.body"
  artifact_unauth_status="$tmp_dir/artifact-unauth.status"
  curl_safe \
    "deploy artifact unauthorized" \
    "${BASE_URL%/}/api/deploy/artifacts/access-agent-manifest.json" \
    "$artifact_unauth_body" \
    "$artifact_unauth_status"
  assert_status_exact "deploy artifact unauthorized" "$(cat "$artifact_unauth_status")" "401"

  artifact_manifest_body="$tmp_dir/artifact-manifest.body"
  artifact_manifest_status="$tmp_dir/artifact-manifest.status"
  curl_bearer_get \
    "deploy artifact manifest" \
    "/api/deploy/artifacts/access-agent-manifest.json" \
    "$DEPLOY_ARTIFACT_TOKEN" \
    "$artifact_manifest_body" \
    "$artifact_manifest_status"
  assert_status_2xx "deploy artifact manifest" "$(cat "$artifact_manifest_status")"

  artifact_sha_body="$tmp_dir/artifact-sha.body"
  artifact_sha_status="$tmp_dir/artifact-sha.status"
  curl_bearer_get \
    "deploy artifact checksum" \
    "/api/deploy/artifacts/access-agent.sha256" \
    "$DEPLOY_ARTIFACT_TOKEN" \
    "$artifact_sha_body" \
    "$artifact_sha_status"
  assert_status_2xx "deploy artifact checksum" "$(cat "$artifact_sha_status")"
  if ! grep -Eq '^[0-9a-f]{64}[[:space:]]+' "$artifact_sha_body"; then
    echo "deploy artifact checksum: invalid sha256 format" >&2
    exit 1
  fi

  if [[ "${DEPLOY_ARTIFACT_FULL:-0}" == "1" ]]; then
    cp "$artifact_sha_body" "$tmp_dir/access-agent.sha256"
    cp "$artifact_manifest_body" "$tmp_dir/access-agent-manifest.json"
    for artifact in access-agent-image.tar.gz xray-image.tar.gz; do
      artifact_body="$tmp_dir/${artifact}"
      artifact_status="$tmp_dir/${artifact}.status"
      curl_bearer_get \
        "deploy artifact ${artifact}" \
        "/api/deploy/artifacts/${artifact}" \
        "$DEPLOY_ARTIFACT_TOKEN" \
        "$artifact_body" \
        "$artifact_status"
      assert_status_2xx "deploy artifact ${artifact}" "$(cat "$artifact_status")"
      gzip -t "$artifact_body"
    done
    (cd "$tmp_dir" && sha256sum -c access-agent.sha256 >/dev/null)
    echo "deploy artifact full verification: passed"
  fi
else
  echo "Skipping deploy artifact checks: DEPLOY_ARTIFACT_TOKEN not provided."
fi

subscription_url=""
subscription_discovered="0"
if [[ -n "${SUB_TOKEN:-}" ]]; then
  subscription_url="${BASE_URL%/}/sub/${SUB_TOKEN}"
elif [[ -n "${SUBSCRIPTION_URL:-}" ]]; then
  subscription_url="$SUBSCRIPTION_URL"
fi

if [[ -z "$subscription_url" && -n "${SMOKE_LOGIN_ACCOUNT:-}" && -n "${SMOKE_LOGIN_PASSWORD:-}" ]]; then
  login_body="$tmp_dir/login.body"
  login_status="$tmp_dir/login.status"
  login_payload="$(login_discovery_payload)"
  curl_json_post \
    "demo login for subscription discovery" \
    "/api/auth/login" \
    "$login_payload" \
    "$login_body" \
    "$login_status"
  assert_status_2xx "demo login for subscription discovery" "$(cat "$login_status")"
  login_token="$(json_eval 'const d=r.data||r; const t=d.access_token||d.accessToken||d.token; if(!t) process.exit(1); process.stdout.write(t);' "$login_body")"

  user_sub_body="$tmp_dir/user-subscription.body"
  user_sub_status="$tmp_dir/user-subscription.status"
  curl_bearer_get \
    "user subscription discovery" \
    "/api/user/subscription" \
    "$login_token" \
    "$user_sub_body" \
    "$user_sub_status"
  assert_status_2xx "user subscription discovery" "$(cat "$user_sub_status")"
  subscription_url="$(json_eval 'const d=r.data||r; const url=d.subscription_url||d.subscriptionUrl||d.clash_url||d.clashUrl||d.url; if(url) process.stdout.write(url);' "$user_sub_body")"
  subscription_discovered="1"
fi

if [[ "$subscription_url" == /* ]]; then
  subscription_url="${BASE_URL%/}${subscription_url}"
fi

if [[ "${SKIP_SUBSCRIPTION_DOWNLOAD:-0}" == "1" ]]; then
  echo "Skipping subscription download: SKIP_SUBSCRIPTION_DOWNLOAD=1."
elif [[ -n "$subscription_url" ]]; then
  sub_body="$tmp_dir/subscription.body"
  sub_status="$tmp_dir/subscription.status"
  curl_safe "subscription download" "$subscription_url" "$sub_body" "$sub_status"
  status="$(cat "$sub_status")"
  if [[ "$status" =~ ^(404|422)$ && "${REQUIRE_SUBSCRIPTION_DOWNLOAD:-0}" != "1" ]]; then
    echo "subscription download: current subscription has no available access line; skipped"
  elif [[ "$status" =~ ^(404|422)$ && "$subscription_discovered" == "1" ]]; then
    if [[ "${REQUIRE_SUBSCRIPTION_DOWNLOAD:-0}" == "1" ]]; then
      echo "subscription download: discovered subscription returned ${status} in required mode" >&2
      exit 1
    fi
    echo "subscription download: discovered demo subscription is inactive or expired; skipped"
  else
  assert_status_2xx "subscription download" "$status"
  assert_no_sensitive_subscription_leak "$sub_body"
  echo "subscription leak check: passed"
  fi
else
  if [[ "${REQUIRE_SUBSCRIPTION_DOWNLOAD:-0}" == "1" ]]; then
    echo "SUB_TOKEN or SUBSCRIPTION_URL is required when REQUIRE_SUBSCRIPTION_DOWNLOAD=1" >&2
    exit 2
  fi
  echo "Skipping subscription download: SUB_TOKEN or SUBSCRIPTION_URL not provided."
fi

if [[ "${DISABLE_SYNTHETIC_AGENT_POSTS:-0}" == "1" ]]; then
  echo "Skipping synthetic agent endpoint checks: DISABLE_SYNTHETIC_AGENT_POSTS=1."
elif [[ -n "${AGENT_TOKEN:-}" && -n "${ACCESS_NODE_ID:-}" ]]; then
  now_unix="$(date +%s)"

  heartbeat_body="$tmp_dir/agent-heartbeat.body"
  heartbeat_status="$tmp_dir/agent-heartbeat.status"
  curl_agent_post \
    "agent heartbeat" \
    "/api/agent/access/heartbeat" \
    "{\"access_node_id\":\"${ACCESS_NODE_ID}\"}" \
    "$heartbeat_body" \
    "$heartbeat_status"
  assert_status_2xx "agent heartbeat" "$(cat "$heartbeat_status")"

  config_body="$tmp_dir/agent-config-result.body"
  config_status="$tmp_dir/agent-config-result.status"
  curl_agent_post \
    "agent config result" \
    "/api/agent/access/config-result" \
    "{\"access_node_id\":\"${ACCESS_NODE_ID}\",\"success\":true,\"message\":\"real-smoke-observation-only\"}" \
    "$config_body" \
    "$config_status"
  assert_status_2xx "agent config result" "$(cat "$config_status")"

  if [[ -n "${ACCESS_LINE_ID:-}" ]]; then
    metrics_body="$tmp_dir/agent-metrics.body"
    metrics_status="$tmp_dir/agent-metrics.status"
    curl_agent_post \
      "agent metrics" \
      "/api/agent/access/metrics" \
      "{\"access_node_id\":\"${ACCESS_NODE_ID}\",\"metrics\":[{\"access_line_id\":\"${ACCESS_LINE_ID}\",\"online_users\":1,\"active_connections\":1,\"unique_client_ips\":1,\"uplink_rate_bps\":1,\"downlink_rate_bps\":1,\"collected_at_unix\":${now_unix}}]}" \
      "$metrics_body" \
      "$metrics_status"
    assert_status_2xx "agent metrics" "$(cat "$metrics_status")"

    sessions_body="$tmp_dir/agent-sessions.body"
    sessions_status="$tmp_dir/agent-sessions.status"
    curl_agent_post \
      "agent sessions" \
      "/api/agent/access/sessions" \
      "{\"access_node_id\":\"${ACCESS_NODE_ID}\",\"sessions\":[{\"access_line_id\":\"${ACCESS_LINE_ID}\",\"xray_user_key\":\"${XRAY_USER_KEY:-real-smoke@xrayc.local}\",\"client_ip_hash\":\"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\",\"started_at_unix\":${now_unix},\"last_seen_at_unix\":${now_unix}}]}" \
      "$sessions_body" \
      "$sessions_status"
    assert_status_2xx "agent sessions" "$(cat "$sessions_status")"

    traffic_body="$tmp_dir/agent-traffic.body"
    traffic_status="$tmp_dir/agent-traffic.status"
    curl_agent_post \
      "agent traffic" \
      "/api/agent/access/traffic" \
      "{\"access_node_id\":\"${ACCESS_NODE_ID}\",\"snapshots\":[{\"access_line_id\":\"${ACCESS_LINE_ID}\",\"xray_user_key\":\"${XRAY_USER_KEY:-real-smoke@xrayc.local}\",\"uplink_bytes\":1,\"downlink_bytes\":1,\"captured_at_unix\":${now_unix}}]}" \
      "$traffic_body" \
      "$traffic_status"
    if [[ "${STRICT_AGENT_TRAFFIC:-0}" == "1" ]]; then
      assert_status_2xx "agent traffic" "$(cat "$traffic_status")"
    else
      assert_status_2xx_or_4xx "agent traffic" "$(cat "$traffic_status")"
    fi

    if [[ -n "${EXIT_ENDPOINT_ID:-}" ]]; then
      probes_body="$tmp_dir/agent-probes.body"
      probes_status="$tmp_dir/agent-probes.status"
      curl_agent_post \
        "agent probes" \
        "/api/agent/access/probes" \
        "{\"access_node_id\":\"${ACCESS_NODE_ID}\",\"line_probes\":[{\"access_line_id\":\"${ACCESS_LINE_ID}\",\"status\":\"healthy\",\"latency_ms\":1,\"probed_at_unix\":${now_unix}}],\"exit_probes\":[{\"exit_endpoint_id\":\"${EXIT_ENDPOINT_ID}\",\"status\":\"healthy\",\"latency_ms\":1,\"probed_at_unix\":${now_unix}}]}" \
        "$probes_body" \
        "$probes_status"
      assert_status_2xx "agent probes" "$(cat "$probes_status")"
    else
      echo "Skipping agent probes: EXIT_ENDPOINT_ID not provided."
    fi
  else
    echo "Skipping agent runtime checks: ACCESS_LINE_ID not provided."
  fi
else
  echo "Skipping agent endpoint checks: AGENT_TOKEN and ACCESS_NODE_ID not provided."
fi

echo "real smoke checks completed"
