#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的客户端订阅解析、出网探测和运行时部署函数。
# 负责从订阅中提取 relay 客户端参数、部署本地 xray 客户端容器并验证 socks 出口。
# 失败诊断只输出脱敏后的容器状态和日志，不输出 transit 地址、UUID 或私有 endpoint。
# 本文件依赖 common.sh 的 die/status/write_shell_env 和 remote.sh 的 remote_exec/remote_bash_env。

client_public_ip() {
  local output status old_errexit url curl_timeout
  curl_timeout="${CLIENT_PUBLIC_IP_CURL_TIMEOUT_SECONDS:-8}"
  old_errexit=0
  case "$-" in
    *e*) old_errexit=1; set +e ;;
  esac
  status=1
  for url in "${PUBLIC_IP_URLS[@]}"; do
    [[ -n "$url" ]] || continue
    output="$(remote_exec 1 "curl --fail --silent --show-error --max-time ${curl_timeout} --socks5-hostname 127.0.0.1:${CLIENT_SOCKS_PORT} '${url}'" 2>/dev/null)"
    status=$?
    if [[ "$status" -eq 0 && -n "$(printf '%s' "$output" | tr -d '[:space:]')" ]]; then
      break
    fi
  done
  if [[ "$old_errexit" -eq 1 ]]; then
    set -e
  fi
  [[ "$status" -eq 0 ]] || return "$status"
  printf '%s' "$output" | tr -d '[:space:]'
}

client_direct_public_ip() {
  local output status old_errexit url curl_timeout
  curl_timeout="${CLIENT_PUBLIC_IP_CURL_TIMEOUT_SECONDS:-8}"
  old_errexit=0
  case "$-" in
    *e*) old_errexit=1; set +e ;;
  esac
  status=1
  for url in "${PUBLIC_IP_URLS[@]}"; do
    [[ -n "$url" ]] || continue
    output="$(remote_exec 1 "curl --fail --silent --show-error --max-time ${curl_timeout} '${url}'" 2>/dev/null)"
    status=$?
    if [[ "$status" -eq 0 && -n "$(printf '%s' "$output" | tr -d '[:space:]')" ]]; then
      break
    fi
  done
  if [[ "$old_errexit" -eq 1 ]]; then
    set -e
  fi
  [[ "$status" -eq 0 ]] || return "$status"
  printf '%s' "$output" | tr -d '[:space:]'
}

client_egress_matches_any_expected() {
  local actual=""
  local expected=""
  actual="$(client_public_ip || true)"
  [[ -n "$actual" ]] || return 1
  for expected in "${protocol_expected_ips[@]}"; do
    [[ "$actual" == "$expected" ]] && return 0
  done
  return 1
}

wait_client_blocked() {
  local reason="$1"
  local actual="" direct_actual="" proxy_status=0 direct_status=0 unavailable=0
  for _ in $(seq 1 36); do
    if actual="$(client_public_ip)"; then
      proxy_status=0
    else
      proxy_status=$?
      actual=""
    fi
    if [[ "$proxy_status" -eq 0 && -n "$actual" ]]; then
      sleep 5
      continue
    fi
    if direct_actual="$(client_direct_public_ip)"; then
      direct_status=0
    else
      direct_status=$?
      direct_actual=""
    fi
    if [[ "$direct_status" -eq 0 && -n "$direct_actual" ]]; then
      status "client_blocked_ok reason=${reason}"
      return 0
    fi
    unavailable=1
    sleep 5
  done
  if [[ "$unavailable" -eq 1 ]]; then
    die "client block check was unavailable after ${reason}"
  fi
  die "client still has proxy egress after ${reason}"
}

wait_client_restored() {
  local reason="$1"
  for _ in $(seq 1 36); do
    if client_egress_matches_any_expected; then
      status "client_restore_ok reason=${reason}"
      return 0
    fi
    sleep 5
  done
  dump_remote_relay_debug
  die "client did not restore expected egress after ${reason}"
}

extract_subscription_client_env() {
  local subscription_yaml="$1"
  local proxy_name="$2"
  local output="$3"
  python3 - "$subscription_yaml" "$proxy_name" "$PH2" "$output" <<'PY'
import json
import shlex
import sys
import yaml

path, proxy_name, transit_host, output = sys.argv[1:5]
with open(path, "r", encoding="utf-8") as fh:
    doc = yaml.safe_load(fh) or {}
proxies = doc.get("proxies") if isinstance(doc, dict) else None
if not isinstance(proxies, list):
    raise SystemExit("subscription proxies are missing")
matches = [
    item for item in proxies
    if isinstance(item, dict) and proxy_name in str(item.get("name", ""))
]
if len(matches) != 1:
    raise SystemExit("relay subscription proxy count mismatch")
proxy = matches[0]
if str(proxy.get("server", "")).strip() != transit_host:
    raise SystemExit("subscription proxy server is not the relay ingress")
uuid = str(proxy.get("uuid", "")).strip()
port = int(proxy.get("port", 0))
if not uuid or port <= 0:
    raise SystemExit("subscription proxy is missing client connection fields")
values = {
    "CLIENT_SERVER": str(proxy.get("server", "")).strip(),
    "CLIENT_PORT": str(port),
    "CLIENT_UUID": uuid,
    "CLIENT_NETWORK": str(proxy.get("network", "tcp") or "tcp").strip() or "tcp",
    "CLIENT_SECURITY": "tls" if bool(proxy.get("tls")) else "none",
    "CLIENT_SERVER_NAME": str(proxy.get("servername", "") or "").strip(),
    "CLIENT_FINGERPRINT": str(proxy.get("client-fingerprint", "") or "").strip(),
    "CLIENT_FLOW": str(proxy.get("flow", "") or "").strip(),
    "CLIENT_PACKET_ENCODING": str(proxy.get("packet-encoding", "") or "").strip(),
}
reality_opts = proxy.get("reality-opts") if isinstance(proxy.get("reality-opts"), dict) else {}
if reality_opts:
    public_key = str(
        reality_opts.get("public-key") or reality_opts.get("publicKey") or ""
    ).strip()
    short_id = str(
        reality_opts.get("short-id") or reality_opts.get("shortId") or ""
    ).strip()
    if not public_key:
        raise SystemExit("subscription reality public key is missing")
    values["CLIENT_SECURITY"] = "reality"
    values["CLIENT_REALITY_PUBLIC_KEY"] = public_key
    values["CLIENT_REALITY_SHORT_ID"] = short_id
else:
    values["CLIENT_REALITY_PUBLIC_KEY"] = ""
    values["CLIENT_REALITY_SHORT_ID"] = ""
xhttp_opts = proxy.get("xhttp-opts") if isinstance(proxy.get("xhttp-opts"), dict) else {}
values["CLIENT_XHTTP_PATH"] = str(xhttp_opts.get("path") or "/xrayc").strip() or "/xrayc"
values["CLIENT_XHTTP_MODE"] = str(xhttp_opts.get("mode") or "stream-one").strip() or "stream-one"
with open(output, "w", encoding="utf-8") as fh:
    for key, value in values.items():
        fh.write(f"{key}={shlex.quote(value)}\n")
PY
}

deploy_client_runtime() {
  local attempt
  for attempt in 1 2 3; do
    if deploy_client_runtime_once; then
      return 0
    fi
    status "client_runtime_deploy_retry=${attempt}"
    sleep 5
  done
  die "client runtime deploy failed"
}

deploy_client_runtime_once() {
  local env_file="$TMP_DIR/client.env"
  BASE_URL="$BASE_URL" DEPLOY_TOKEN="$DEPLOY_TOKEN" CLIENT_SERVER="$CLIENT_SERVER" CLIENT_PORT="$CLIENT_PORT" \
    CLIENT_UUID="$CLIENT_UUID" CLIENT_NETWORK="$CLIENT_NETWORK" CLIENT_SECURITY="$CLIENT_SECURITY" \
    CLIENT_SERVER_NAME="${CLIENT_SERVER_NAME:-}" CLIENT_FINGERPRINT="${CLIENT_FINGERPRINT:-}" \
    CLIENT_REALITY_PUBLIC_KEY="${CLIENT_REALITY_PUBLIC_KEY:-}" CLIENT_REALITY_SHORT_ID="${CLIENT_REALITY_SHORT_ID:-}" \
    CLIENT_FLOW="$CLIENT_FLOW" CLIENT_XHTTP_PATH="${CLIENT_XHTTP_PATH:-/xrayc}" CLIENT_XHTTP_MODE="${CLIENT_XHTTP_MODE:-stream-one}" \
    CLIENT_PACKET_ENCODING="${CLIENT_PACKET_ENCODING:-}" CLIENT_SOCKS_PORT="$CLIENT_SOCKS_PORT" \
    write_shell_env "$env_file" BASE_URL DEPLOY_TOKEN CLIENT_SERVER CLIENT_PORT CLIENT_UUID CLIENT_NETWORK CLIENT_SECURITY CLIENT_SERVER_NAME CLIENT_FINGERPRINT CLIENT_REALITY_PUBLIC_KEY CLIENT_REALITY_SHORT_ID CLIENT_FLOW CLIENT_XHTTP_PATH CLIENT_XHTTP_MODE CLIENT_PACKET_ENCODING CLIENT_SOCKS_PORT
  remote_bash_env 1 "$env_file" <<'REMOTE'
install_dir="/opt/xrayc-real-matrix-relay-client"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
mkdir -p "$install_dir"
printf 'header = "Authorization: Bearer %s"\n' "$DEPLOY_TOKEN" > "$tmp_dir/curl.conf"
curl --fail --silent --show-error --location --config "$tmp_dir/curl.conf" \
  --output "$tmp_dir/xray-image.tar.gz" \
  "${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz" >/dev/null
docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
python3 - "$install_dir/config.json" <<'PY'
import json
import os
import sys

path = sys.argv[1]
user = {"id": os.environ["CLIENT_UUID"], "encryption": "none"}
flow = os.environ.get("CLIENT_FLOW", "").strip()
if flow:
    user["flow"] = flow
packet_encoding = os.environ.get("CLIENT_PACKET_ENCODING", "").strip()
if packet_encoding:
    user["packetEncoding"] = packet_encoding
stream_settings = {"network": os.environ["CLIENT_NETWORK"]}
if stream_settings["network"] == "xhttp":
    stream_settings["xhttpSettings"] = {
        "path": os.environ.get("CLIENT_XHTTP_PATH", "/xrayc") or "/xrayc",
        "mode": os.environ.get("CLIENT_XHTTP_MODE", "stream-one") or "stream-one",
    }
security = os.environ.get("CLIENT_SECURITY", "none").strip().lower()
if security and security != "none":
    stream_settings["security"] = security
    server_name = os.environ.get("CLIENT_SERVER_NAME", "").strip()
    fingerprint = os.environ.get("CLIENT_FINGERPRINT", "").strip() or "chrome"
    if security == "reality":
        public_key = os.environ.get("CLIENT_REALITY_PUBLIC_KEY", "").strip()
        if not public_key:
            raise SystemExit("CLIENT_REALITY_PUBLIC_KEY is required")
        reality = {
            "fingerprint": fingerprint,
            "publicKey": public_key,
        }
        if server_name:
            reality["serverName"] = server_name
        short_id = os.environ.get("CLIENT_REALITY_SHORT_ID", "").strip()
        if short_id:
            reality["shortId"] = short_id
        stream_settings["realitySettings"] = reality
    elif security == "tls" and server_name:
        stream_settings["tlsSettings"] = {"serverName": server_name}
config = {
    "log": {"loglevel": "warning"},
    "inbounds": [{
        "tag": "socks-in",
        "listen": "127.0.0.1",
        "port": int(os.environ["CLIENT_SOCKS_PORT"]),
        "protocol": "socks",
        "settings": {"udp": True},
    }],
    "outbounds": [{
        "tag": "proxy",
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": os.environ["CLIENT_SERVER"],
                "port": int(os.environ["CLIENT_PORT"]),
                "users": [user],
            }]
        },
        "streamSettings": stream_settings,
    }],
}
open(path, "w", encoding="utf-8").write(json.dumps(config, separators=(",", ":")))
PY
docker rm -f xrayc-real-matrix-relay-client >/dev/null 2>&1 || true
docker run -d --name xrayc-real-matrix-relay-client --restart unless-stopped --network host \
  -v "$install_dir/config.json:/etc/xray/config.json:ro" \
  xrayc/xray:local run -config /etc/xray/config.json >/dev/null
for _ in $(seq 1 30); do
  if timeout 5 bash -lc "</dev/tcp/127.0.0.1/${CLIENT_SOCKS_PORT}" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 2
done
echo "client proxy readiness failed; sensitive values redacted" >&2
docker ps --filter name=xrayc-real-matrix-relay-client --format 'client_container={{.Status}}' >&2 || true
redact_sensitive() {
  python3 -c 'import os, sys
data = sys.stdin.read()
for key, replacement in (
    ("CLIENT_SERVER", "<transit>"),
    ("CLIENT_UUID", "<credential>"),
    ("CLIENT_SERVER_NAME", "<server-name>"),
    ("CLIENT_REALITY_PUBLIC_KEY", "<reality-key>"),
    ("CLIENT_REALITY_SHORT_ID", "<reality-short-id>"),
):
    value = os.environ.get(key, "")
    if value:
        data = data.replace(value, replacement)
sys.stdout.write(data)'
}
docker logs --tail 20 xrayc-real-matrix-relay-client 2>&1 \
  | redact_sensitive \
  | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g' >&2 || true
exit 1
REMOTE
}
