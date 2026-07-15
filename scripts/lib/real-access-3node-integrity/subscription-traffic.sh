#!/usr/bin/env bash
# 此 helper 提供真实三节点完整性 E2E 的订阅内容、客户端流量和驱逐等待检查。
# 它由主脚本 source，不直接执行，并避免打印订阅地址、代理地址或公网出口 IP。

assert_subscription_shape() {
  local subscription_file="$1"

  xrayc_real_e2e_assert_subscription_access_servers "$subscription_file"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_file"
  xrayc_real_e2e_assert_subscription_missing_pattern \
    "$subscription_file" \
    '(outbound_proxy_url|exit_endpoint|agent_token|private[ _-]?key|secret_key)[[:space:]]*[:=]' \
    "subscription leaked control-plane or upstream fields"

  EXPECTED_ACCESS_SERVERS="$EXPECTED_ACCESS_SERVERS" MIN_RELAY_COUNT="$MIN_RELAY_COUNT" \
    python3 - "$subscription_file" <<'PY'
import os
import sys

try:
    import yaml
except Exception as exc:
    raise SystemExit(f"PyYAML is required for structured subscription checks: {exc}")

expected = {item.strip() for item in os.environ["EXPECTED_ACCESS_SERVERS"].split(",") if item.strip()}
min_relay_count = int(os.environ.get("MIN_RELAY_COUNT", "1"))

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)
if not isinstance(payload, dict):
    raise SystemExit("subscription YAML root is not a mapping")
proxies = payload.get("proxies")
if not isinstance(proxies, list) or not proxies:
    raise SystemExit("subscription YAML has no proxies")

actual = set()
for proxy in proxies:
    if not isinstance(proxy, dict):
        raise SystemExit("subscription proxy entry is not a mapping")
    server = str(proxy.get("server", "")).strip()
    port = proxy.get("port")
    if not server or port is None:
        raise SystemExit("subscription proxy entry is missing server or port")
    endpoint = f"{server}:{int(port)}"
    if endpoint not in expected:
        raise SystemExit("subscription contains a non-relay proxy endpoint")
    actual.add(endpoint)

if len(actual) < min_relay_count:
    raise SystemExit("subscription contains fewer relay endpoints than required")
print("subscription relay-only check passed")
PY
}

subscription_contains_target_access() {
  local subscription_file="$1"
  local target_server="$2"
  EXPECTED_TARGET_ACCESS_SERVER="$target_server" python3 - "$subscription_file" <<'PY'
import os
import sys

try:
    import yaml
except Exception as exc:
    raise SystemExit(f"PyYAML is required for structured subscription checks: {exc}")

target = os.environ["EXPECTED_TARGET_ACCESS_SERVER"]
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)
proxies = payload.get("proxies") if isinstance(payload, dict) else None
if not isinstance(proxies, list):
    raise SystemExit(1)
for proxy in proxies:
    if not isinstance(proxy, dict):
        continue
    server = str(proxy.get("server", "")).strip()
    port = proxy.get("port")
    if port is not None and f"{server}:{int(port)}" == target:
        raise SystemExit(0)
raise SystemExit(1)
PY
}

ip_in_expected_set() {
  local actual="$1"
  ACTUAL_EGRESS_IP="$actual" EXPECTED_EXIT_IPS="$EXPECTED_EXIT_IPS" python3 - <<'PY'
import os
import sys

actual = os.environ["ACTUAL_EGRESS_IP"].strip()
expected = {item.strip() for item in os.environ["EXPECTED_EXIT_IPS"].split(",") if item.strip()}
raise SystemExit(0 if actual in expected else 1)
PY
}

fetch_public_ip_via_client() {
  local output_file="$1"
  xrayc_real_e2e_fetch_url_via_proxy_to_file \
    "$CLIENT_PROXY_URL" \
    "$PUBLIC_IP_URL" \
    "$output_file" \
    "public IP probe failed"
  tr -d '[:space:]' <"$output_file"
}

send_real_client_traffic() {
  local attempt=1
  local actual_ip_file="$tmp_dir/public-ip.txt"
  local actual_ip=""
  echo "Checking real client egress"
  while [[ "$attempt" -le "$TRAFFIC_ATTEMPTS" ]]; do
    actual_ip="$(fetch_public_ip_via_client "$actual_ip_file")"
    if ! ip_in_expected_set "$actual_ip"; then
      fail "unexpected egress ip: actual value differs from expected value"
    fi
    if [[ "$REAL_TRAFFIC_URL" != "$PUBLIC_IP_URL" ]]; then
      xrayc_real_e2e_fetch_url_via_proxy_to_file \
        "$CLIENT_PROXY_URL" \
        "$REAL_TRAFFIC_URL" \
        "$tmp_dir/real-traffic-${attempt}.body" \
        "real traffic probe failed"
    fi
    sleep "$TRAFFIC_INTERVAL_SECONDS"
    attempt=$((attempt + 1))
  done
}

client_proxy_is_blocked() {
  local actual_ip_file="$tmp_dir/blocked-public-ip.txt"
  local actual_ip=""
  if ! curl --fail --silent --location --max-time "$CURL_TIMEOUT" \
    --proxy "$CLIENT_PROXY_URL" "$PUBLIC_IP_URL" >"$actual_ip_file" 2>/dev/null; then
    return 0
  fi
  actual_ip="$(tr -d '[:space:]' <"$actual_ip_file")"
  if ! ip_in_expected_set "$actual_ip"; then
    return 0
  fi
  return 1
}

wait_client_proxy_blocked() {
  local reason="$1"
  local elapsed=0
  echo "Waiting for client eviction (${reason})"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    if client_proxy_is_blocked; then
      echo "client eviction observed (${reason})"
      return
    fi
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "client proxy still reaches expected egress after eviction timeout (${reason})"
}

assert_subscription_hides_exit_hosts() {
  local subscription_file="$1"
  local host=""
  [[ -n "$EXIT_POOL_HOSTS_FILE" && -f "$EXIT_POOL_HOSTS_FILE" ]] || return 0
  while IFS= read -r host; do
    [[ -n "$host" ]] || continue
    xrayc_real_e2e_assert_subscription_missing_literal \
      "$subscription_file" \
      "$host" \
      "subscription leaked an exit-pool host"
  done <"$EXIT_POOL_HOSTS_FILE"
}

subscription_http_status() {
  local status_file="$tmp_dir/subscription-probe.status"
  local body_file="$tmp_dir/subscription-probe.body"
  local url
  url="$(subscription_download_url)"
  if ! curl --silent --location --max-time "$CURL_TIMEOUT" \
    --output "$body_file" \
    --write-out "%{http_code}" \
    "$url" >"$status_file" 2>/dev/null; then
    printf '000\n'
    return
  fi
  cat "$status_file"
}

wait_subscription_rejected() {
  local reason="$1"
  local elapsed=0
  local status=""
  echo "Waiting for subscription rejection (${reason})"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    status="$(subscription_http_status)"
    case "$status" in
      2*) ;;
      *)
        echo "subscription rejection observed (${reason})"
        return
        ;;
    esac
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "subscription remained downloadable after timeout (${reason})"
}

wait_subscription_target_absent() {
  local reason="$1"
  local elapsed=0
  local status_file="$tmp_dir/subscription-target.status"
  local body_file="$tmp_dir/subscription-target.yaml"
  local url
  url="$(subscription_download_url)"
  echo "Waiting for target relay removal from subscription (${reason})"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    if curl --silent --location --max-time "$CURL_TIMEOUT" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" >"$status_file" 2>/dev/null; then
      case "$(cat "$status_file")" in
        2*)
          if ! subscription_contains_target_access "$body_file" "$TARGET_ACCESS_SERVER"; then
            echo "target relay absent from subscription (${reason})"
            return
          fi
          ;;
        *)
          echo "subscription became unavailable (${reason})"
          return
          ;;
      esac
    else
      echo "subscription became unavailable (${reason})"
      return
    fi
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "target relay remained in subscription after timeout (${reason})"
}

wait_subscription_target_present() {
  local elapsed=0
  local body_file="$tmp_dir/subscription-restored.yaml"
  local status_file="$tmp_dir/subscription-restored.status"
  local url
  url="$(subscription_download_url)"
  echo "Waiting for subscription restoration"
  while [[ "$elapsed" -le "$EVICTION_POLL_SECONDS" ]]; do
    if curl --silent --location --max-time "$CURL_TIMEOUT" \
      --output "$body_file" \
      --write-out "%{http_code}" \
      "$url" >"$status_file" 2>/dev/null; then
      if [[ "$(cat "$status_file")" == 2* ]] && subscription_contains_target_access "$body_file" "$TARGET_ACCESS_SERVER"; then
        echo "subscription restoration observed"
        return
      fi
    fi
    sleep "$EVICTION_POLL_INTERVAL_SECONDS"
    elapsed=$((elapsed + EVICTION_POLL_INTERVAL_SECONDS))
  done
  fail "target relay did not return to subscription after restoration timeout"
}
