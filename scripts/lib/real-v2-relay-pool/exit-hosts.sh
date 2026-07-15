setup_exit() {
  local role="$1"
  local index="$2"
  local container="$3"
  local listen_port="$4"
  local username="$5"
  local password="$6"
  local public_host ss_port ss_password vless_port vless_uuid
  if [[ "$role" == "exit_a" ]]; then
    public_host="$EXIT_A_PUBLIC_HOST"
    ss_port="$EXIT_A_SS_PORT"
    ss_password="$EXIT_A_SS_PASSWORD"
    vless_port="$EXIT_A_VLESS_PORT"
    vless_uuid="$EXIT_A_VLESS_UUID"
  else
    public_host="$EXIT_B_PUBLIC_HOST"
    ss_port="$EXIT_B_SS_PORT"
    ss_password="$EXIT_B_SS_PASSWORD"
    vless_port="$EXIT_B_VLESS_PORT"
    vless_uuid="$EXIT_B_VLESS_UUID"
  fi
  local env_file="$TMP_DIR/exit-${role}.env"
  BASE_URL="$BASE_URL" DEPLOY_TOKEN="$DEPLOY_TOKEN" CONTAINER_NAME="$container" LISTEN_PORT="$listen_port" \
    SOCKS_USERNAME="$username" SOCKS_PASSWORD="$password" PUBLIC_HOST="$public_host" \
    SHADOWSOCKS_PORT="$ss_port" SHADOWSOCKS_METHOD="$EXIT_SS_METHOD" SHADOWSOCKS_PASSWORD="$ss_password" \
    VLESS_PORT="$vless_port" VLESS_UUID="$vless_uuid" \
    write_shell_env "$env_file" BASE_URL DEPLOY_TOKEN CONTAINER_NAME LISTEN_PORT SOCKS_USERNAME SOCKS_PASSWORD PUBLIC_HOST SHADOWSOCKS_PORT SHADOWSOCKS_METHOD SHADOWSOCKS_PASSWORD VLESS_PORT VLESS_UUID
  remote_bash_env "$index" "$env_file" <<'REMOTE'
set -euo pipefail
install_dir="/opt/${CONTAINER_NAME}"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
mkdir -p "$install_dir"
if ! docker image inspect xrayc/xray:local >/dev/null 2>&1; then
  printf 'header = "Authorization: Bearer %s"\n' "$DEPLOY_TOKEN" > "$tmp_dir/curl.conf"
  curl --fail --silent --show-error --location --connect-timeout 15 --max-time 180 \
    --speed-time 30 --speed-limit 1024 --config "$tmp_dir/curl.conf" \
    --output "$tmp_dir/xray-image.tar.gz" \
    "${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz" >/dev/null
  docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
fi
python3 - "$install_dir/config.json" "$LISTEN_PORT" <<'PY'
import json
import os
import sys

path, port = sys.argv[1], int(sys.argv[2])
username, password = os.environ["SOCKS_USERNAME"], os.environ["SOCKS_PASSWORD"]
config = {
    "log": {"loglevel": "warning"},
    "inbounds": [
        {
            "tag": "socks-in",
            "listen": "0.0.0.0",
            "port": port,
            "protocol": "socks",
            "settings": {
                "auth": "password",
                "accounts": [{"user": username, "pass": password}],
                "udp": True,
                "ip": os.environ["PUBLIC_HOST"],
            },
        },
        {
            "tag": "shadowsocks-in",
            "listen": "0.0.0.0",
            "port": int(os.environ["SHADOWSOCKS_PORT"]),
            "protocol": "shadowsocks",
            "settings": {
                "method": os.environ["SHADOWSOCKS_METHOD"],
                "password": os.environ["SHADOWSOCKS_PASSWORD"],
                "network": "tcp,udp",
            },
        },
        {
            "tag": "vless-in",
            "listen": "0.0.0.0",
            "port": int(os.environ["VLESS_PORT"]),
            "protocol": "vless",
            "settings": {
                "clients": [
                    {
                        "id": os.environ["VLESS_UUID"],
                        "email": "xrayc-real-vless-exit",
                    }
                ],
                "decryption": "none",
            },
            "streamSettings": {"network": "tcp", "security": "none"},
        },
    ],
    "outbounds": [{"tag": "direct", "protocol": "freedom"}],
}
open(path, "w", encoding="utf-8").write(json.dumps(config, separators=(",", ":")))
PY
docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
docker run -d --name "$CONTAINER_NAME" --restart unless-stopped --network host \
  -v "$install_dir/config.json:/etc/xray/config.json:ro" \
  xrayc/xray:local run -config /etc/xray/config.json >/dev/null
iptables -I INPUT -p tcp --dport "$LISTEN_PORT" -j ACCEPT >/dev/null 2>&1 || true
iptables -I INPUT -p udp --dport "$LISTEN_PORT" -j ACCEPT >/dev/null 2>&1 || true
iptables -I INPUT -p tcp --dport "$SHADOWSOCKS_PORT" -j ACCEPT >/dev/null 2>&1 || true
iptables -I INPUT -p udp --dport "$SHADOWSOCKS_PORT" -j ACCEPT >/dev/null 2>&1 || true
iptables -I INPUT -p tcp --dport "$VLESS_PORT" -j ACCEPT >/dev/null 2>&1 || true
iptables -I INPUT -p udp --dport "$VLESS_PORT" -j ACCEPT >/dev/null 2>&1 || true
if command -v ufw >/dev/null 2>&1; then
  ufw allow "${LISTEN_PORT}/tcp" >/dev/null 2>&1 || true
  ufw allow "${LISTEN_PORT}/udp" >/dev/null 2>&1 || true
  ufw allow "${SHADOWSOCKS_PORT}/tcp" >/dev/null 2>&1 || true
  ufw allow "${SHADOWSOCKS_PORT}/udp" >/dev/null 2>&1 || true
  ufw allow "${VLESS_PORT}/tcp" >/dev/null 2>&1 || true
  ufw allow "${VLESS_PORT}/udp" >/dev/null 2>&1 || true
fi
for _ in $(seq 1 20); do
  if curl --fail --silent --show-error --max-time 8 \
    --socks5-hostname "${SOCKS_USERNAME}:${SOCKS_PASSWORD}@127.0.0.1:${LISTEN_PORT}" https://api.ipify.org >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done
exit 1
REMOTE
}

setup_udp_target() {
  local env_file="$TMP_DIR/udp-target.env"
  UDP_TARGET_PORT="$UDP_TARGET_PORT" write_shell_env "$env_file" UDP_TARGET_PORT
  remote_bash_env "$EXIT_B_REMOTE_INDEX" "$env_file" <<'REMOTE'
set -euo pipefail
install_dir="/opt/xrayc-real-udp-target"
mkdir -p "$install_dir"
if [[ -s "$install_dir/pid" ]]; then
  kill "$(cat "$install_dir/pid")" >/dev/null 2>&1 || true
fi
cat > "$install_dir/server.py" <<'PY'
import os
import socket
import sys

port = int(sys.argv[1])
state_path = sys.argv[2]
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("0.0.0.0", port))
packets = 0
bytes_seen = 0
with open(state_path, "w", encoding="utf-8") as fh:
    fh.write("0\t0\n")
while True:
    payload, _addr = sock.recvfrom(65535)
    packets += 1
    bytes_seen += len(payload)
    tmp_path = f"{state_path}.tmp"
    with open(tmp_path, "w", encoding="utf-8") as fh:
        fh.write(f"{packets}\t{bytes_seen}\n")
    os.replace(tmp_path, state_path)
PY
nohup python3 "$install_dir/server.py" "$UDP_TARGET_PORT" "$install_dir/counters.tsv" >/dev/null 2>&1 &
echo "$!" > "$install_dir/pid"
iptables -I INPUT -p udp --dport "$UDP_TARGET_PORT" -j ACCEPT >/dev/null 2>&1 || true
if command -v ufw >/dev/null 2>&1; then
  ufw allow "${UDP_TARGET_PORT}/udp" >/dev/null 2>&1 || true
fi
for _ in $(seq 1 20); do
  if [[ -s "$install_dir/counters.tsv" ]] && kill -0 "$(cat "$install_dir/pid")" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done
exit 1
REMOTE
}

setup_tcp_rate_target() {
  local env_file="$TMP_DIR/tcp-rate-target.env"
  TCP_TARGET_PORT="$TCP_TARGET_PORT" write_shell_env "$env_file" TCP_TARGET_PORT
  remote_bash_env "$EXIT_B_REMOTE_INDEX" "$env_file" <<'REMOTE'
set -euo pipefail
install_dir="/opt/xrayc-real-tcp-target"
mkdir -p "$install_dir"
if [[ -s "$install_dir/pid" ]]; then
  kill "$(cat "$install_dir/pid")" >/dev/null 2>&1 || true
fi
cat > "$install_dir/server.py" <<'PY'
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse
import sys


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        query = parse_qs(urlparse(self.path).query)
        size = int(query.get("size", ["1048576"])[0])
        size = max(1, min(size, 16 * 1024 * 1024))
        chunk = b"x" * 16384
        self.send_response(200)
        self.send_header("content-type", "application/octet-stream")
        self.send_header("content-length", str(size))
        self.end_headers()
        remaining = size
        while remaining > 0:
            payload = chunk[: min(len(chunk), remaining)]
            self.wfile.write(payload)
            remaining -= len(payload)

    def log_message(self, _format, *_args):
        return


server = ThreadingHTTPServer(("0.0.0.0", int(sys.argv[1])), Handler)
server.serve_forever()
PY
nohup python3 "$install_dir/server.py" "$TCP_TARGET_PORT" >/dev/null 2>&1 &
echo "$!" > "$install_dir/pid"
iptables -I INPUT -p tcp --dport "$TCP_TARGET_PORT" -j ACCEPT >/dev/null 2>&1 || true
if command -v ufw >/dev/null 2>&1; then
  ufw allow "${TCP_TARGET_PORT}/tcp" >/dev/null 2>&1 || true
fi
for _ in $(seq 1 20); do
  if kill -0 "$(cat "$install_dir/pid")" >/dev/null 2>&1 \
    && curl --fail --silent --show-error --max-time 5 "http://127.0.0.1:${TCP_TARGET_PORT}/bytes?size=128" >/dev/null; then
    exit 0
  fi
  sleep 1
done
exit 1
REMOTE
}

wait_udp_target_reachable() {
  wait_udp_target_reachable_from 2 transit
  wait_udp_target_reachable_from 3 exit
}

wait_tcp_rate_target_reachable() {
  wait_tcp_rate_target_reachable_from 2 transit
  wait_tcp_rate_target_reachable_from 3 exit
}

wait_tcp_rate_target_reachable_from() {
  local source_index="$1"
  local label="$2"
  if remote_exec "$source_index" "curl --fail --silent --show-error --max-time 8 'http://${EXIT_B_PUBLIC_HOST}:${TCP_TARGET_PORT}/bytes?size=1024' >/dev/null" >/dev/null; then
    echo "real_v2_relay_pool_e2e: tcp_rate_target_ready source=${label}"
    return
  fi
  die "${label} cannot reach TCP rate target"
}

wait_udp_target_reachable_from() {
  local source_index="$1"
  local label="$2"
  local before_packets before_bytes after_packets after_bytes
  IFS=$'\t' read -r before_packets before_bytes < <(
    remote_exec "$EXIT_B_REMOTE_INDEX" "cat /opt/xrayc-real-udp-target/counters.tsv 2>/dev/null || printf '0\t0\n'"
  )
  [[ "${before_packets:-}" =~ ^[0-9]+$ && "${before_bytes:-}" =~ ^[0-9]+$ ]] \
    || die "UDP target counter is unavailable"
  remote_exec "$source_index" "python3 - <<'PY'
import socket

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(2)
sock.sendto(b'xrayc-udp-readiness', ('${EXIT_B_PUBLIC_HOST}', ${UDP_TARGET_PORT}))
PY" >/dev/null
  for _ in $(seq 1 10); do
    IFS=$'\t' read -r after_packets after_bytes < <(
      remote_exec "$EXIT_B_REMOTE_INDEX" "cat /opt/xrayc-real-udp-target/counters.tsv 2>/dev/null || printf '0\t0\n'"
    )
    if [[ "${after_packets:-}" =~ ^[0-9]+$ && "$after_packets" -gt "$before_packets" ]]; then
      echo "real_v2_relay_pool_e2e: udp_target_ready source=${label}"
      return
    fi
    sleep 1
  done
  die "${label} cannot reach UDP target"
}

wait_exit_socks_udp_reachable() {
  local socks_host="$1"
  local socks_port="$2"
  local socks_username="$3"
  local socks_password="$4"
  local env_file="$TMP_DIR/socks-udp-probe.env"
  local before_packets before_bytes after_packets after_bytes
  IFS=$'\t' read -r before_packets before_bytes < <(
    remote_exec "$EXIT_B_REMOTE_INDEX" "cat /opt/xrayc-real-udp-target/counters.tsv 2>/dev/null || printf '0\t0\n'"
  )
  [[ "${before_packets:-}" =~ ^[0-9]+$ && "${before_bytes:-}" =~ ^[0-9]+$ ]] \
    || die "UDP target counter is unavailable"
  SOCKS_HOST="$socks_host" SOCKS_PORT="$socks_port" SOCKS_USERNAME="$socks_username" \
    SOCKS_PASSWORD="$socks_password" UDP_TARGET_HOST="$EXIT_B_PUBLIC_HOST" UDP_TARGET_PORT="$UDP_TARGET_PORT" \
    write_shell_env "$env_file" SOCKS_HOST SOCKS_PORT SOCKS_USERNAME SOCKS_PASSWORD UDP_TARGET_HOST UDP_TARGET_PORT
  remote_bash_env 2 "$env_file" <<'REMOTE' >/dev/null
set -euo pipefail
python3 - <<'PY'
import ipaddress
import os
import socket
import struct
import time


def read_exact(sock, size):
    data = b''
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError('short socks reply')
        data += chunk
    return data


def socks_address(value):
    try:
        parsed = ipaddress.ip_address(value)
    except ValueError:
        encoded = value.encode()
        return b'\x03' + bytes([len(encoded)]) + encoded
    if parsed.version == 4:
        return b'\x01' + parsed.packed
    return b'\x04' + parsed.packed


socks_host = os.environ['SOCKS_HOST']
socks_port = int(os.environ['SOCKS_PORT'])
username = os.environ['SOCKS_USERNAME'].encode()
password = os.environ['SOCKS_PASSWORD'].encode()
target_host = os.environ['UDP_TARGET_HOST']
target_port = int(os.environ['UDP_TARGET_PORT'])

tcp = socket.create_connection((socks_host, socks_port), timeout=8)
tcp.settimeout(8)
tcp.sendall(b'\x05\x01\x02')
method = read_exact(tcp, 2)
if method != b'\x05\x02':
    raise RuntimeError('socks auth method rejected')
tcp.sendall(b'\x01' + bytes([len(username)]) + username + bytes([len(password)]) + password)
if read_exact(tcp, 2) != b'\x01\x00':
    raise RuntimeError('socks auth failed')
tcp.sendall(b'\x05\x03\x00\x01\x00\x00\x00\x00\x00\x00')
header = read_exact(tcp, 4)
if header[:2] != b'\x05\x00':
    raise RuntimeError('socks udp associate failed')
atyp = header[3]
if atyp == 1:
    relay_host = socket.inet_ntop(socket.AF_INET, read_exact(tcp, 4))
elif atyp == 3:
    relay_host = read_exact(tcp, read_exact(tcp, 1)[0]).decode()
elif atyp == 4:
    relay_host = socket.inet_ntop(socket.AF_INET6, read_exact(tcp, 16))
else:
    raise RuntimeError('unsupported socks relay address')
relay_port = struct.unpack('!H', read_exact(tcp, 2))[0]
if relay_host in {'0.0.0.0', '::'}:
    relay_host = socks_host

udp = socket.socket(socket.AF_INET6 if ':' in relay_host else socket.AF_INET, socket.SOCK_DGRAM)
payload = b'xrayc-socks-udp-readiness'
packet = b'\x00\x00\x00' + socks_address(target_host) + struct.pack('!H', target_port) + payload
for _ in range(5):
    udp.sendto(packet, (relay_host, relay_port))
    time.sleep(0.2)
PY
REMOTE
  for _ in $(seq 1 10); do
    IFS=$'\t' read -r after_packets after_bytes < <(
      remote_exec "$EXIT_B_REMOTE_INDEX" "cat /opt/xrayc-real-udp-target/counters.tsv 2>/dev/null || printf '0\t0\n'"
    )
    if [[ "${after_packets:-}" =~ ^[0-9]+$ && "$after_packets" -gt "$before_packets" ]]; then
      echo "real_v2_relay_pool_e2e: exit_aux_socks_udp_ready"
      return
    fi
    sleep 1
  done
  die "transit cannot reach UDP target through auxiliary exit SOCKS readiness sidecar"
}
