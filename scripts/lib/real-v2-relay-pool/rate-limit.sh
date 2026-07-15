# 真实 V2 中转用户级限速验收辅助函数。
# 由主脚本在真实客户端和账本已打通后调用。
# 验证口径：用户限速覆盖套餐限速，且覆盖值高于套餐值时仍以用户值生效。
# shellcheck shell=bash

rate_limit_snapshot_saved="${rate_limit_snapshot_saved:-0}"
original_plan_rate_limit_bps="${original_plan_rate_limit_bps:-}"
original_user_rate_limit_bps="${original_user_rate_limit_bps:-}"
original_user_rate_limit_bps_is_null="${original_user_rate_limit_bps_is_null:-0}"

rate_limit_shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

rate_limit_transit_check() {
  sshpass -f "$PF2" ssh -p "$P2" \
    -o ConnectTimeout=10 -o ServerAliveInterval=10 \
    -o ServerAliveCountMax=2 \
    -o StrictHostKeyChecking=accept-new -o BatchMode=no \
    "$U2@$H2" "$@"
}

rate_limit_client_probe() {
  sshpass -f "$PF1" ssh -p "$P1" \
    -o ConnectTimeout=10 -o ServerAliveInterval=10 \
    -o ServerAliveCountMax=2 \
    -o StrictHostKeyChecking=accept-new -o BatchMode=no \
    "$U1@$H1" "$@"
}

rate_limit_require_positive_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

rate_limit_tc_rate() {
  local value="$1"
  if (( value >= 1000000 && value % 1000000 == 0 )); then
    printf '%smbit' "$((value / 1000000))"
  elif (( value >= 1000 && value % 1000 == 0 )); then
    printf '%skbit' "$((value / 1000))"
  else
    printf '%sbit' "$value"
  fi
}

snapshot_rate_limit_state() {
  if [[ "$rate_limit_snapshot_saved" == "1" ]]; then
    return
  fi
  original_plan_rate_limit_bps="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v plan_id="$user_plan_id" <<'SQL'
SELECT rate_limit_bps::text FROM plans WHERE id = :'plan_id'::uuid LIMIT 1;
SQL
)"
  original_user_rate_limit_bps="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v user_id="$user_id" <<'SQL'
SELECT COALESCE(rate_limit_bps::text, '__NULL__')
FROM users
WHERE id = :'user_id'::uuid
LIMIT 1;
SQL
)"
  [[ -n "$original_plan_rate_limit_bps" && -n "$original_user_rate_limit_bps" ]] \
    || die "rate limit snapshot failed"
  if [[ "$original_user_rate_limit_bps" == "__NULL__" ]]; then
    original_user_rate_limit_bps=""
    original_user_rate_limit_bps_is_null=1
  else
    original_user_rate_limit_bps_is_null=0
  fi
  rate_limit_snapshot_saved=1
}

apply_user_rate_limit_override() {
  local plan_bps="$1"
  local user_bps="$2"
  psql_db -Xq -v ON_ERROR_STOP=1 \
    -v plan_id="$user_plan_id" \
    -v user_id="$user_id" \
    -v access_node_id="$access_node_id" \
    -v plan_bps="$plan_bps" \
    -v user_bps="$user_bps" <<'SQL'
BEGIN;
UPDATE plans SET rate_limit_bps = :'plan_bps'::bigint WHERE id = :'plan_id'::uuid;
UPDATE users SET rate_limit_bps = :'user_bps'::bigint WHERE id = :'user_id'::uuid;
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_user_rate_limit_override'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
}

wait_relay_rate_limit_plan() {
  local user_bps="$1"
  local tc_rate
  local marked_tag
  tc_rate="$(rate_limit_tc_rate "$user_bps")"
  marked_tag="exit-${assigned_exit_endpoint_id//-/}-user-${user_id//-/}"
  local quoted_rate quoted_tag
  quoted_rate="$(rate_limit_shell_quote "rate ${tc_rate}")"
  quoted_tag="$(rate_limit_shell_quote "$marked_tag")"
  for _ in $(seq 1 60); do
    if rate_limit_transit_check "test -s /opt/xrayc-real-relay/state/limiter-plan.sh && grep -Fq ${quoted_rate} /opt/xrayc-real-relay/state/limiter-plan.sh && grep -Fq ${quoted_tag} /opt/xrayc-real-relay/xray/config.json && grep -Fq '\"mark\"' /opt/xrayc-real-relay/xray/config.json" >/dev/null 2>&1; then
      echo "real_v2_relay_pool_e2e: rate_limit_plan_ok"
      return
    fi
    sleep 3
  done
  die "relay limiter plan did not include user override"
}

rate_limit_marked_tag() {
  printf 'exit-%s-user-%s' "${assigned_exit_endpoint_id//-/}" "${user_id//-/}"
}

rate_limit_user_mark() {
  local marked_tag quoted_tag
  marked_tag="$(rate_limit_marked_tag)"
  quoted_tag="$(rate_limit_shell_quote "$marked_tag")"
  rate_limit_transit_check "python3 -c 'import json, sys
tag = sys.argv[1]
with open(\"/opt/xrayc-real-relay/xray/config.json\", \"r\", encoding=\"utf-8\") as fh:
    config = json.load(fh)
for outbound in config.get(\"outbounds\", []):
    if outbound.get(\"tag\") == tag:
        mark = outbound.get(\"streamSettings\", {}).get(\"sockopt\", {}).get(\"mark\", \"\")
        print(mark)
        break
' ${quoted_tag}" | tr -d '[:space:]'
}

rate_limit_user_class_id() {
  local mark="$1"
  local quoted_mark
  quoted_mark="$(rate_limit_shell_quote "$mark")"
  rate_limit_transit_check "awk -v mark=${quoted_mark} '
    /tc filter replace dev/ && /parent 1:/ && / fw flowid 1:/ {
      if (index(\$0, \"handle \" mark \" fw\") > 0) {
        for (i = 1; i <= NF; i++) {
          if (\$i == \"flowid\") {
            split(\$(i + 1), parts, \":\")
            print parts[2]
            exit
          }
        }
      }
    }
  ' /opt/xrayc-real-relay/state/limiter-plan.sh" | tr -d '[:space:]'
}

rate_limit_assert_class_rate() {
  local class_id="$1"
  local user_bps="$2"
  local tc_rate quoted_class
  tc_rate="$(rate_limit_tc_rate "$user_bps")"
  quoted_class="$(rate_limit_shell_quote "classid 1:${class_id} htb rate ${tc_rate} ceil ${tc_rate}")"
  rate_limit_transit_check "grep -Fq ${quoted_class} /opt/xrayc-real-relay/state/limiter-plan.sh"
}

rate_limit_assert_ifb_class_rate() {
  local class_id="$1"
  local user_bps="$2"
  local tc_rate quoted_class
  tc_rate="$(rate_limit_tc_rate "$user_bps")"
  quoted_class="$(rate_limit_shell_quote "classid 2:${class_id} htb rate ${tc_rate} ceil ${tc_rate}")"
  rate_limit_transit_check "grep -Fq ${quoted_class} /opt/xrayc-real-relay/state/limiter-plan.sh"
}

rate_limit_primary_interface() {
  rate_limit_transit_check "awk '/tc qdisc replace dev/ && /root handle 1:/ { print \$5; exit }' /opt/xrayc-real-relay/state/limiter-plan.sh" \
    | tr -d '[:space:]'
}

rate_limit_ifb_interface() {
  rate_limit_transit_check "awk '/tc qdisc replace dev/ && /root handle 2:/ { print \$5; exit }' /opt/xrayc-real-relay/state/limiter-plan.sh" \
    | tr -d '[:space:]'
}

rate_limit_class_bytes() {
  local interface="$1"
  local major="$2"
  local class_id="$3"
  rate_limit_transit_check "tc -s class show dev ${interface} classid ${major}:${class_id} | awk '/Sent/ { print \$2; exit }'" \
    | tr -d '[:space:]'
}

rate_limit_class_packets() {
  local interface="$1"
  local major="$2"
  local class_id="$3"
  rate_limit_transit_check "tc -s class show dev ${interface} classid ${major}:${class_id} | awk '/Sent/ { print \$4; exit }'" \
    | tr -d '[:space:]'
}

rate_limit_udp_target_counters() {
  remote_exec "$EXIT_B_REMOTE_INDEX" "cat /opt/xrayc-real-udp-target/counters.tsv 2>/dev/null || printf '0\t0\n'"
}

start_client_udp_generator() {
  local bytes="$1"
  local duration="$2"
  local send_bps="$3"
  rate_limit_client_probe "cat > /tmp/xrayc-real-udp-generator.py <<'PY'
import socket
import sys
import time


target_bytes = int(sys.argv[1])
duration = float(sys.argv[2])
client_udp_port = int(sys.argv[3])
send_bps = int(sys.argv[4])
payload = b'x' * 1200
bytes_per_second = max(send_bps / 8.0, len(payload))


udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
deadline = time.monotonic() + duration
next_send = time.monotonic()
sent = 0
while sent < target_bytes and time.monotonic() < deadline:
    now = time.monotonic()
    if now < next_send:
        time.sleep(min(next_send - now, 0.01))
        continue
    udp.sendto(payload, ('127.0.0.1', client_udp_port))
    sent += len(payload)
    next_send += len(payload) / bytes_per_second
print(sent)
PY
python3 /tmp/xrayc-real-udp-generator.py ${bytes} ${duration} ${CLIENT_UDP_PORT} ${send_bps} >/tmp/xrayc-real-udp-generator.log 2>&1 & echo \$!"
}

wait_client_udp_generator() {
  local pid="$1"
  local duration="$2"
  local max_wait=$((duration + 15))
  rate_limit_client_probe "for _ in \$(seq 1 ${max_wait}); do if ! kill -0 ${pid} >/dev/null 2>&1; then cat /tmp/xrayc-real-udp-generator.log 2>/dev/null || true; exit 0; fi; sleep 1; done; kill ${pid} >/dev/null 2>&1 || true; cat /tmp/xrayc-real-udp-generator.log 2>/dev/null || true; exit 1"
}

assert_user_udp_rate_limit_class_delta() {
  local delta_bytes="$1"
  local measured_bps="$2"
  local user_bps="$3"
  local delta_packets="$4"
  local target_delta_bytes="$5"
  local target_delta_packets="$6"
  local sent_bytes="$7"
  local min_bytes="${XRAYC_REAL_E2E_UDP_RATE_LIMIT_MIN_BYTES:-12000}"
  local max_bps="${XRAYC_REAL_E2E_UDP_RATE_LIMIT_MAX_BPS:-$((user_bps + user_bps / 2 + 250000))}"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_RATE_LIMIT_MIN_BYTES "$min_bytes"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_RATE_LIMIT_MAX_BPS "$max_bps"
  if ! python3 - "$delta_bytes" "$measured_bps" "$user_bps" "$delta_packets" "$target_delta_bytes" "$target_delta_packets" "$sent_bytes" "$min_bytes" "$max_bps" <<'PY'
import sys
delta, measured, user, packets, target_delta, target_packets, sent, minimum, maximum = map(int, sys.argv[1:])
if sent <= 0:
    raise SystemExit("UDP generator did not send bytes")
if delta < minimum:
    raise SystemExit("UDP class byte delta too small")
if packets <= 0:
    raise SystemExit("UDP class packet delta too small")
if target_delta < minimum:
    raise SystemExit("UDP target byte delta too small")
if target_packets <= 0:
    raise SystemExit("UDP target packet delta too small")
if measured > maximum:
    raise SystemExit("UDP measured class rate above expected ceiling")
if user <= 0:
    raise SystemExit("user limit must be positive")
PY
  then
    if [[ "${XRAYC_REAL_E2E_UDP_RATE_LIMIT_PAUSE_ON_FAILURE:-0}" == "1" ]]; then
      echo "real_v2_relay_pool_e2e: udp_rate_limit_probe_paused" >&2
      sleep "${XRAYC_REAL_E2E_UDP_RATE_LIMIT_PAUSE_SECONDS:-300}"
    fi
    die "UDP shared user limiter class assertion failed delta_bytes=${delta_bytes} packets=${delta_packets} target_delta_bytes=${target_delta_bytes} target_packets=${target_delta_packets} measured_bps=${measured_bps} user_bps=${user_bps}"
  fi
}

measure_user_rate_limit_bps() {
  local default_bps="${XRAYC_REAL_E2E_USER_RATE_LIMIT_BPS:-4000000}"
  local default_bytes=$((default_bps * 20 / 8))
  if (( default_bytes < 262144 )); then
    default_bytes=262144
  elif (( default_bytes > 4194304 )); then
    default_bytes=4194304
  fi
  local bytes="${XRAYC_REAL_E2E_RATE_LIMIT_BYTES:-$default_bytes}"
  local max_time="${XRAYC_REAL_E2E_RATE_LIMIT_MAX_TIME_SECONDS:-60}"
  local artifact_url="${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz"
  local attempts="${XRAYC_REAL_E2E_RATE_LIMIT_ATTEMPTS:-2}"
  local url_candidates=()
  if [[ -n "${XRAYC_REAL_E2E_RATE_LIMIT_URLS:-}" ]]; then
    local saved_ifs="$IFS"
    IFS=' ,'
    read -r -a url_candidates <<<"${XRAYC_REAL_E2E_RATE_LIMIT_URLS}"
    IFS="$saved_ifs"
  else
    local controlled_tcp_target_url="http://${EXIT_B_PUBLIC_HOST}:${TCP_TARGET_PORT}/bytes?size=${bytes}"
    url_candidates=(
      "$controlled_tcp_target_url"
      "$artifact_url"
      "https://speed.cloudflare.com/__down?bytes=${bytes}"
      "https://cachefly.cachefly.net/10mb.test"
      "https://proof.ovh.net/files/10Mb.dat"
    )
  fi
  rate_limit_require_positive_int XRAYC_REAL_E2E_RATE_LIMIT_BYTES "$bytes"
  rate_limit_require_positive_int XRAYC_REAL_E2E_RATE_LIMIT_MAX_TIME_SECONDS "$max_time"
  rate_limit_require_positive_int XRAYC_REAL_E2E_RATE_LIMIT_ATTEMPTS "$attempts"
  local url attempt quoted_url speed_bytes
  for url in "${url_candidates[@]}"; do
    [[ -n "$url" ]] || continue
    quoted_url="$(rate_limit_shell_quote "$url")"
    local header_arg=""
    if [[ "$url" == "${BASE_URL%/}/api/deploy/artifacts/"* ]]; then
      header_arg="--header $(rate_limit_shell_quote "Authorization: Bearer ${DEPLOY_TOKEN}")"
    fi
    for attempt in $(seq 1 "$attempts"); do
      set +e
      speed_bytes="$(rate_limit_client_probe "curl --fail --silent --show-error --location --connect-timeout 8 --max-time ${max_time} --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} ${header_arg} -o /dev/null -w '%{speed_download}' --url ${quoted_url} 2>/dev/null" 2>/dev/null | tr -d '[:space:]')"
      probe_status=$?
      set -e
      if [[ "$speed_bytes" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        local measured_bps
        if measured_bps="$(python3 - "$speed_bytes" <<'PY'
import sys
speed = float(sys.argv[1])
if speed <= 0:
    raise SystemExit(1)
print(int(speed * 8))
PY
)"; then
          printf '%s\n' "$measured_bps"
          return
        fi
        if [[ -n "${XRAYC_REAL_E2E_RATE_LIMIT_DEBUG:-}" ]]; then
          echo "real_v2_relay_pool_e2e: rate_limit_probe_numeric_invalid status=${probe_status} raw=${speed_bytes} url=${url}" >&2
        fi
      elif [[ -n "${XRAYC_REAL_E2E_RATE_LIMIT_DEBUG:-}" ]]; then
        echo "real_v2_relay_pool_e2e: rate_limit_probe_attempt_failed status=${probe_status} output_len=${#speed_bytes} url=${url}" >&2
      fi
      sleep 2
    done
  done
  if rate_limit_client_probe "curl --fail --silent --show-error --connect-timeout 8 --max-time 15 --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} https://api.ipify.org >/dev/null 2>&1" >/dev/null 2>&1; then
    echo "real_v2_relay_pool_e2e: rate_limit_small_probe_ok" >&2
  else
    echo "real_v2_relay_pool_e2e: rate_limit_small_probe_failed" >&2
  fi
  if [[ "${XRAYC_REAL_E2E_RATE_LIMIT_PAUSE_ON_FAILURE:-0}" == "1" ]]; then
    echo "real_v2_relay_pool_e2e: rate_limit_probe_paused" >&2
    sleep "${XRAYC_REAL_E2E_RATE_LIMIT_PAUSE_SECONDS:-300}"
  fi
  die "rate limit download probe failed"
}

assert_user_rate_limit_measurement() {
  local measured_bps="$1"
  local plan_bps="$2"
  local user_bps="$3"
  local min_bps="${XRAYC_REAL_E2E_RATE_LIMIT_MIN_BPS:-$((user_bps / 2))}"
  local max_bps="${XRAYC_REAL_E2E_RATE_LIMIT_MAX_BPS:-$((user_bps + user_bps / 2 + 250000))}"
  rate_limit_require_positive_int XRAYC_REAL_E2E_RATE_LIMIT_MIN_BPS "$min_bps"
  rate_limit_require_positive_int XRAYC_REAL_E2E_RATE_LIMIT_MAX_BPS "$max_bps"
  if ! python3 - "$measured_bps" "$plan_bps" "$user_bps" "$min_bps" "$max_bps" <<'PY'
import sys
measured, plan, user, minimum, maximum = map(int, sys.argv[1:])
if measured <= plan:
    raise SystemExit("measured rate did not exceed the lower plan default")
if measured < minimum:
    raise SystemExit("measured rate is below expected user override range")
if measured > maximum:
    raise SystemExit("measured rate is above expected user override ceiling")
if user <= plan:
    raise SystemExit("test setup invalid: user rate must be above plan rate")
PY
  then
    die "rate limit measurement outside expected range measured_bps=${measured_bps} plan_bps=${plan_bps} user_bps=${user_bps}"
  fi
}

run_user_rate_limit_e2e() {
  local plan_bps="${XRAYC_REAL_E2E_PLAN_RATE_LIMIT_BPS:-1000000}"
  local user_bps="${XRAYC_REAL_E2E_USER_RATE_LIMIT_BPS:-4000000}"
  rate_limit_require_positive_int XRAYC_REAL_E2E_PLAN_RATE_LIMIT_BPS "$plan_bps"
  rate_limit_require_positive_int XRAYC_REAL_E2E_USER_RATE_LIMIT_BPS "$user_bps"
  (( user_bps > plan_bps )) || die "user rate limit must be greater than plan rate for override test"
  snapshot_rate_limit_state
  apply_user_rate_limit_override "$plan_bps" "$user_bps"
  wait_relay_rate_limit_plan "$user_bps"
  refresh_subscription_client_runtime "rate-limit-applied"
  wait_client_restored "rate-limit-applied"
  local measured_bps
  measured_bps="$(measure_user_rate_limit_bps)"
  echo "real_v2_relay_pool_e2e: rate_limit_measurement measured_bps=${measured_bps}"
  assert_user_rate_limit_measurement "$measured_bps" "$plan_bps" "$user_bps"
  echo "real_v2_relay_pool_e2e: user_rate_limit_override_ok measured_bps=${measured_bps}"
}

run_user_udp_rate_limit_e2e() {
  local user_bps="${XRAYC_REAL_E2E_USER_RATE_LIMIT_BPS:-4000000}"
  local duration="${XRAYC_REAL_E2E_UDP_RATE_LIMIT_DURATION_SECONDS:-18}"
  local sample_seconds="${XRAYC_REAL_E2E_UDP_RATE_LIMIT_SAMPLE_SECONDS:-8}"
  rate_limit_require_positive_int XRAYC_REAL_E2E_USER_RATE_LIMIT_BPS "$user_bps"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_RATE_LIMIT_DURATION_SECONDS "$duration"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_RATE_LIMIT_SAMPLE_SECONDS "$sample_seconds"
  local default_bytes=$((user_bps * duration / 2))
  if (( default_bytes < 1048576 )); then
    default_bytes=1048576
  fi
  local bytes="${XRAYC_REAL_E2E_UDP_RATE_LIMIT_BYTES:-$default_bytes}"
  local send_bps="${XRAYC_REAL_E2E_UDP_SEND_BPS:-$((user_bps * 4))}"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_RATE_LIMIT_BYTES "$bytes"
  rate_limit_require_positive_int XRAYC_REAL_E2E_UDP_SEND_BPS "$send_bps"
  rate_limit_require_positive_int XRAYC_REAL_E2E_CLIENT_SOCKS_PORT "$CLIENT_SOCKS_PORT"
  rate_limit_require_positive_int XRAYC_REAL_E2E_CLIENT_UDP_PORT "$CLIENT_UDP_PORT"
  local mark class_id interface
  mark="$(rate_limit_user_mark)"
  [[ "$mark" =~ ^[1-9][0-9]*$ ]] || die "relay limiter user mark was not found"
  class_id="$(rate_limit_user_class_id "$mark")"
  [[ "$class_id" =~ ^[0-9]+$ ]] || die "relay limiter user class was not found"
  rate_limit_assert_class_rate "$class_id" "$user_bps" || die "relay limiter user egress class rate does not match user override"
  rate_limit_assert_ifb_class_rate "$class_id" "$user_bps" || die "relay limiter user ingress class rate does not match user override"
  interface="$(rate_limit_ifb_interface)"
  [[ "$interface" =~ ^[A-Za-z0-9_.:-]+$ ]] || die "relay limiter IFB interface was not found"
  local before after packets_before packets_after delta packet_delta measured_bps pid sent_bytes
  local target_delta_bytes target_delta_packets
  local target_packets_before target_bytes_before target_packets_after target_bytes_after
  IFS=$'\t' read -r target_packets_before target_bytes_before < <(rate_limit_udp_target_counters)
  [[ "${target_packets_before:-}" =~ ^[0-9]+$ && "${target_bytes_before:-}" =~ ^[0-9]+$ ]] \
    || die "UDP target counter is unavailable"
  before="$(rate_limit_class_bytes "$interface" 2 "$class_id")"
  packets_before="$(rate_limit_class_packets "$interface" 2 "$class_id")"
  [[ "$before" =~ ^[0-9]+$ ]] || die "relay limiter class byte counter is unavailable"
  [[ "$packets_before" =~ ^[0-9]+$ ]] || die "relay limiter class packet counter is unavailable"
  pid="$(start_client_udp_generator "$bytes" "$duration" "$send_bps" | tr -d '[:space:]')"
  [[ "$pid" =~ ^[0-9]+$ ]] || die "UDP generator did not start"
  sleep "$sample_seconds"
  after="$(rate_limit_class_bytes "$interface" 2 "$class_id")"
  packets_after="$(rate_limit_class_packets "$interface" 2 "$class_id")"
  sent_bytes="$(wait_client_udp_generator "$pid" "$duration" | tail -n 1 | tr -d '[:space:]')" || die "UDP generator did not finish"
  IFS=$'\t' read -r target_packets_after target_bytes_after < <(rate_limit_udp_target_counters)
  [[ "$after" =~ ^[0-9]+$ && "$before" =~ ^[0-9]+$ ]] || die "relay limiter class byte counter is invalid"
  [[ "$packets_after" =~ ^[0-9]+$ && "$packets_before" =~ ^[0-9]+$ ]] || die "relay limiter class packet counter is invalid"
  [[ "$sent_bytes" =~ ^[0-9]+$ ]] || die "UDP generator sent byte count is invalid"
  [[ "${target_packets_after:-}" =~ ^[0-9]+$ && "${target_bytes_after:-}" =~ ^[0-9]+$ ]] \
    || die "UDP target counter is invalid"
  delta=$((after - before))
  packet_delta=$((packets_after - packets_before))
  target_delta_bytes=$((target_bytes_after - target_bytes_before))
  target_delta_packets=$((target_packets_after - target_packets_before))
  measured_bps="$(python3 - "$delta" "$sample_seconds" <<'PY'
import sys
delta = int(sys.argv[1])
seconds = int(sys.argv[2])
print(int(delta * 8 / seconds))
PY
)"
  assert_user_udp_rate_limit_class_delta "$delta" "$measured_bps" "$user_bps" "$packet_delta" "$target_delta_bytes" "$target_delta_packets" "$sent_bytes"
  echo "real_v2_relay_pool_e2e: user_udp_rate_limit_shared_class_ok delta_bytes=${delta} packets=${packet_delta} target_delta_bytes=${target_delta_bytes} target_packets=${target_delta_packets} measured_bps=${measured_bps}"
}
