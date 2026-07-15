# shellcheck shell=bash
# Real mihomo/clash subscription traffic helpers.
# The parent script owns tmp_dir, profile and generated config paths.

find_free_local_port() {
  python3 - <<'PY'
import socket

sock = socket.socket()
sock.bind(("127.0.0.1", 0))
try:
    print(sock.getsockname()[1])
finally:
    sock.close()
PY
}

wait_local_tcp_port() {
  local port="$1"
  for _ in $(seq 1 30); do
    if timeout 1 bash -lc "</dev/tcp/127.0.0.1/${port}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

curl_through_local_http_proxy() {
  local port="$1"
  local label="$2"
  local output_file="$tmp_dir/${label}.traffic.out"
  local traffic_url="${XRAYC_REAL_SUBSCRIPTION_CLIENT_TRAFFIC_URL:-http://cp.cloudflare.com/generate_204}"

  curl --fail --silent --location --max-time "${CURL_TIMEOUT:-30}" \
    --proxy "http://127.0.0.1:${port}" "$traffic_url" >"$output_file" 2>/dev/null
}

prepare_mihomo_runtime_profile() {
  local input="$1"
  local output="$2"
  local listen_port="${3:-7890}"
  python3 - "$input" "$output" "$listen_port" <<'PY'
import os
import sys
import yaml

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    profile = yaml.safe_load(fh)
if not isinstance(profile, dict):
    raise SystemExit("subscription profile must be a YAML mapping")
proxies = profile.get("proxies")
if not isinstance(proxies, list) or not proxies:
    raise SystemExit("subscription profile must contain at least one proxy")
preferred = [
    item.strip().lower()
    for item in os.environ.get(
        "XRAYC_REAL_SUBSCRIPTION_CLIENT_TRAFFIC_PROXY_TYPES",
        "trojan,vless,ss,hysteria2,hy2",
    ).split(",")
    if item.strip()
]
selected = None
for wanted in preferred:
    for proxy in proxies:
        if not isinstance(proxy, dict):
            continue
        proxy_type = str(proxy.get("type", "")).strip().lower()
        if proxy_type == wanted or (wanted == "ss" and proxy_type == "shadowsocks"):
            selected = proxy
            break
    if selected is not None:
        break
if selected is None:
    selected = next((proxy for proxy in proxies if isinstance(proxy, dict)), None)
if not isinstance(selected, dict):
    raise SystemExit("subscription profile must contain a proxy mapping")
proxy_name = str(selected.get("name", "")).strip()
if not proxy_name:
    raise SystemExit("subscription proxy must have a name")
group_name = "xrayc-real-client-traffic"
profile["proxies"] = [selected]
profile["proxy-groups"] = [{"name": group_name, "type": "select", "proxies": [proxy_name]}]
profile["rules"] = [f"MATCH,{group_name}"]
try:
    listen_port = int(sys.argv[3])
except ValueError as exc:
    raise SystemExit("listen port must be an integer") from exc
if listen_port <= 0 or listen_port > 65535:
    raise SystemExit("listen port out of range")
profile["mixed-port"] = listen_port
profile["allow-lan"] = False
profile["bind-address"] = "127.0.0.1"
profile["log-level"] = "info"
with open(sys.argv[2], "w", encoding="utf-8") as fh:
    yaml.safe_dump(profile, fh, allow_unicode=True, sort_keys=False)
PY
}

remote_subscription_client_traffic_enabled() {
  [[ -n "${XRAYC_REAL_E2E_INVENTORY:-}" && -f "${XRAYC_REAL_E2E_INVENTORY:-}" ]]
}

remote_upload_subscription_client_file() {
  local source_file="$1"
  local target_name="$2"
  remote_exec "cat > /opt/xrayc-subscription-client-compat/${target_name}" < "$source_file"
}

remote_select_free_subscription_client_ports() {
  remote_exec "python3 - <<'PY'
import socket

sock = socket.socket()
sock.bind(('127.0.0.1', 0))
try:
    print(str(sock.getsockname()[1]))
finally:
    sock.close()
PY"
}

stage_remote_subscription_client_configs() {
  remote_exec "rm -rf /opt/xrayc-subscription-client-compat && mkdir -p /opt/xrayc-subscription-client-compat" >/dev/null 2>&1 \
    && remote_upload_subscription_client_file "$mihomo_runtime_profile" "mihomo-run.yaml" >/dev/null 2>&1
}

write_remote_subscription_client_runner() {
  local label="$1"
  local image="$2"
  local args="$3"
  local listen_port="$4"
  local traffic_url="${XRAYC_REAL_SUBSCRIPTION_CLIENT_TRAFFIC_URL:-http://cp.cloudflare.com/generate_204}"
  local curl_timeout="${CURL_TIMEOUT:-30}"

  LABEL_VALUE="$label" IMAGE_VALUE="$image" ARGS_VALUE="$args" \
  LISTEN_PORT_VALUE="$listen_port" TRAFFIC_URL_VALUE="$traffic_url" CURL_TIMEOUT_VALUE="$curl_timeout" \
    python3 - "$tmp_dir/remote-${label}-traffic.sh" <<'PY'
import os
import shlex
import sys

values = {name: os.environ[name] for name in (
    "LABEL_VALUE",
    "IMAGE_VALUE",
    "ARGS_VALUE",
    "LISTEN_PORT_VALUE",
    "TRAFFIC_URL_VALUE",
    "CURL_TIMEOUT_VALUE",
)}
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    for key, value in values.items():
        fh.write(f"export {key}={shlex.quote(value)}\n")
    fh.write(r'''
	set -euo pipefail
	set +x
	base_dir="/opt/xrayc-subscription-client-compat"
	status_file="/tmp/xrayc-subscription-client-${LABEL_VALUE}.status"
	rm -f "$status_file"
	write_status() {
	  printf '%s\n' "$1" > "$status_file"
	}
	write_status "select-port"
	host_port="$LISTEN_PORT_VALUE"
	container="xrayc-subscription-${LABEL_VALUE}-${host_port}"
cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker rm -f "$container" >/dev/null 2>&1 || true
write_status "docker-run"
docker run -d --name "$container" \
  --network host \
  -v "${base_dir}:/work:ro" \
  "$IMAGE_VALUE" $ARGS_VALUE >/dev/null
write_status "wait-port"
for _ in $(seq 1 30); do
  if timeout 1 bash -lc "</dev/tcp/127.0.0.1/${host_port}" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
timeout 1 bash -lc "</dev/tcp/127.0.0.1/${host_port}" >/dev/null 2>&1
write_status "curl-proxy"
	if ! curl --fail --silent --location --max-time "$CURL_TIMEOUT_VALUE" \
	  --proxy "http://127.0.0.1:${host_port}" "$TRAFFIC_URL_VALUE" >/tmp/xrayc-subscription-client-traffic.out 2>/dev/null; then
	  curl_status="$?"
	  write_status "curl-proxy-exit-${curl_status}"
	  exit 1
	fi
write_status "ok"
''')
PY
}

remote_subscription_client_stage() {
  local label="$1"
  remote_exec "cat /tmp/xrayc-subscription-client-${label}.status 2>/dev/null || true" 2>/dev/null \
    | tr -d '\r\n' \
    | sed 's/[^A-Za-z0-9_.:-]/_/g'
}

run_remote_subscription_client_traffic_checks() {
  local previous_tmp_dir="${TMP_DIR:-}"
  local previous_inventory="${INVENTORY:-}"
  local previous_client_target="${CLIENT_TARGET:-}"
  local remote_target="${XRAYC_REAL_SUBSCRIPTION_CLIENT_TRAFFIC_TARGET:-${REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET:-server_1}}"
  local mihomo_port remote_ports
  local remote_status=0

  remote_subscription_client_cleanup() {
    remote_exec "docker rm -f xrayc-subscription-mihomo-${mihomo_port:-0} >/dev/null 2>&1 || true; rm -rf /opt/xrayc-subscription-client-compat; rm -f /tmp/xrayc-subscription-client-mihomo.status /tmp/xrayc-subscription-client-traffic.out" >/dev/null 2>&1 || true
    TMP_DIR="$previous_tmp_dir"
    INVENTORY="$previous_inventory"
    CLIENT_TARGET="$previous_client_target"
  }

  TMP_DIR="$tmp_dir"
  INVENTORY="$XRAYC_REAL_E2E_INVENTORY"
  CLIENT_TARGET="$remote_target"
  load_inventory_client
  build_ssh_command
  if ! remote_ports="$(remote_select_free_subscription_client_ports | tr -d '\r')"; then
    echo "remote subscription traffic port selection failed; details redacted" >&2
    remote_status=1
  fi
  IFS=' ' read -r mihomo_port <<< "${remote_ports:-}"
  if [[ "$remote_status" -eq 0 && ! "$mihomo_port" =~ ^[1-9][0-9]*$ ]]; then
    echo "remote subscription traffic port selection failed; details redacted" >&2
    remote_status=1
  fi
  if [[ "$remote_status" -eq 0 ]]; then
    prepare_mihomo_runtime_profile "$profile" "$mihomo_runtime_profile" "$mihomo_port"
    if ! stage_remote_subscription_client_configs; then
      echo "remote subscription traffic setup failed; raw output redacted" >&2
      remote_status=1
    else
      write_remote_subscription_client_runner "mihomo" "${XRAYC_MIHOMO_IMAGE:-metacubex/mihomo:latest}" "-f /work/mihomo-run.yaml" "$mihomo_port"
      if remote_bash_file "$tmp_dir/remote-mihomo-traffic.sh" >/dev/null 2>&1; then
        echo "mihomo remote subscription traffic ok"
      else
        echo "mihomo remote subscription traffic failed at $(remote_subscription_client_stage mihomo); raw output redacted" >&2
        remote_status=1
      fi
    fi
  fi

  remote_subscription_client_cleanup
  return "$remote_status"
}

run_mihomo_or_clash_traffic() {
  local profile="$1"
  local image="${XRAYC_MIHOMO_IMAGE:-metacubex/mihomo:latest}"
  local port container status

  command -v docker >/dev/null 2>&1 || {
    echo "Docker is required for real mihomo/clash subscription traffic" >&2
    exit 1
  }
  port="$(find_free_local_port)"
  prepare_mihomo_runtime_profile "$profile" "$mihomo_runtime_profile" "$port"
  container="xrayc-subscription-mihomo-${$}-${port}"
  docker rm -f "$container" >/dev/null 2>&1 || true
  docker run -d --name "$container" \
    --network host \
    -v "$tmp_dir:/work:ro" \
    "$image" -f /work/mihomo-run.yaml >/dev/null
  status=0
  if ! wait_local_tcp_port "$port" || ! curl_through_local_http_proxy "$port" "mihomo"; then
    status=1
  fi
  docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ "$status" -ne 0 ]]; then
    echo "mihomo Docker subscription traffic failed; raw output redacted" >&2
    exit 1
  fi
  echo "mihomo Docker subscription traffic ok"
}

run_subscription_client_traffic_checks() {
  if ! bool_enabled "${RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC:-1}"; then
    echo "subscription client traffic check skipped"
    return 0
  fi
  prepare_mihomo_runtime_profile "$profile" "$mihomo_runtime_profile"
  if remote_subscription_client_traffic_enabled; then
    run_remote_subscription_client_traffic_checks
    return 0
  fi
  run_mihomo_or_clash_traffic "$profile"
}
