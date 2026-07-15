#!/usr/bin/env bash
set -euo pipefail
set +x

env_file="$1"
ports_file="$2"
tarball="$3"
# shellcheck disable=SC1090
. "$env_file"

mkdir -p "$REMOTE_TMP/configs" "$REMOTE_TMP/results" "$REMOTE_CLIENT_DIR"
tar -xzf "$tarball" -C "$REMOTE_TMP/configs"

upload_sink_pid=""
cleanup_runner() {
  if [[ -n "${upload_sink_pid:-}" ]]; then
    kill "$upload_sink_pid" >/dev/null 2>&1 || true
  fi
  while iptables -D INPUT -p tcp --dport "$UPLOAD_LISTEN_PORT" -j ACCEPT >/dev/null 2>&1; do
    :
  done
}
trap cleanup_runner EXIT

if ! docker image inspect xrayc/xray:local >/dev/null 2>&1; then
  tmp_dir="$(mktemp -d)"
  printf 'header = "Authorization: Bearer %s"\n' "$DEPLOY_ARTIFACT_TOKEN" >"$tmp_dir/curl.conf"
  curl --fail --silent --show-error --location --config "$tmp_dir/curl.conf" \
    --output "$tmp_dir/xray-image.tar.gz" \
    "${BASE_URL%/}/api/deploy/artifacts/xray-image.tar.gz" >/dev/null
  docker load --input "$tmp_dir/xray-image.tar.gz" >/dev/null
  rm -rf "$tmp_dir"
fi

cat >"$REMOTE_TMP/upload-sink.py" <<'PY'
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import sys

port = int(sys.argv[1])
counters = sys.argv[2]

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if not self.path.startswith("/download"):
            self.send_response(404)
            self.end_headers()
            return
        size = 1048576
        if "bytes=" in self.path:
            try:
                size = max(1, min(int(self.path.rsplit("bytes=", 1)[1].split("&", 1)[0]), 16 * 1024 * 1024))
            except ValueError:
                size = 1048576
        chunk = b"x" * min(size, 64 * 1024)
        self.send_response(200)
        self.send_header("Content-Length", str(size))
        self.end_headers()
        remaining = size
        while remaining > 0:
            data = chunk[: min(len(chunk), remaining)]
            self.wfile.write(data)
            remaining -= len(data)

    def do_POST(self):
        self._handle_upload()

    def do_PUT(self):
        self._handle_upload()

    def _handle_upload(self):
        remaining = int(self.headers.get("Content-Length") or "0")
        total = 0
        while remaining > 0:
            chunk = self.rfile.read(min(remaining, 1024 * 1024))
            if not chunk:
                break
            total += len(chunk)
            remaining -= len(chunk)
        with open(counters, "a", encoding="utf-8") as fh:
            fh.write(f"{self.path}\t{total}\n")
        self.send_response(204)
        self.end_headers()

    def log_message(self, *_args):
        return

ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
PY
iptables -I INPUT -p tcp --dport "$UPLOAD_LISTEN_PORT" -j ACCEPT >/dev/null 2>&1 || true
python3 "$REMOTE_TMP/upload-sink.py" "$UPLOAD_LISTEN_PORT" "$REMOTE_TMP/upload-counters.tsv" &
upload_sink_pid="$!"
echo "$upload_sink_pid" >"$REMOTE_TMP/upload-sink.pid"
upload_sink_ready=0
for _ in $(seq 1 20); do
  if printf ready | curl --fail --silent --show-error --max-time 5 \
    --request POST --data-binary @- "http://127.0.0.1:${UPLOAD_LISTEN_PORT}/ready" >/dev/null 2>&1; then
    upload_sink_ready=1
    break
  fi
  sleep 1
done
[[ "$upload_sink_ready" == "1" ]] || { echo "upload_sink_readiness=failed" >&2; exit 1; }

while IFS=$'\t' read -r idx user_id xray_key port; do
  name="xrayc-multi-user-client-${RUN_ID}-${idx}"
  install_dir="${REMOTE_CLIENT_DIR}/${RUN_ID}-${idx}"
  mkdir -p "$install_dir"
  cp "${REMOTE_TMP}/configs/user-${idx}"/*.json "$install_dir/config.json"
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker run -d --name "$name" --restart unless-stopped --network host \
    -v "${install_dir}/config.json:/etc/xray/config.json:ro" \
    xrayc/xray:local run -config /etc/xray/config.json >/dev/null
done <"$ports_file"

while IFS=$'\t' read -r idx user_id xray_key port; do
  ready=0
  readiness_err="${REMOTE_TMP}/client-${idx}-readiness.err"
  : >"$readiness_err"
  for _ in $(seq 1 90); do
    if curl --fail --silent --show-error --max-time 12 \
      --socks5-hostname "127.0.0.1:${port}" "$PUBLIC_IP_URL" \
      >/dev/null 2>"$readiness_err"; then
      ready=1
      break
    fi
    sleep 1
  done
  if [[ "$ready" != "1" ]]; then
    echo "client_${idx}_readiness=failed" >&2
    if [[ -s "$readiness_err" ]]; then
      sed 's/[[:cntrl:]]//g' "$readiness_err" | tail -n 20 >&2 || true
    fi
    docker ps -a --filter "name=xrayc-multi-user-client-${RUN_ID}-" >&2 || true
    docker logs --tail 80 "xrayc-multi-user-client-${RUN_ID}-${idx}" >&2 || true
    exit 1
  fi
  if [[ -n "$EXPECTED_EXIT_IP" ]]; then
    # 出口身份核验带有界重试：每用户专属限速入口在 agent reload xray 后,
    # 首个 egress 请求可能落在连接尚未稳定的瞬时窗口而返回空/超时(非旁路、非路由错误)。
    # 给最多 30 次(每次间隔 2s)机会等待稳定;只要任一次返回值等于预期出口 IP 即通过,
    # 始终不匹配才判失败——真旁路绝不会偶发命中预期出口 IP,防旁路语义不被削弱。
    egress_ok=0
    actual=""
    for _ in $(seq 1 30); do
      actual="$(curl --fail --silent --show-error --max-time 12 \
        --socks5-hostname "127.0.0.1:${port}" "$PUBLIC_IP_URL" 2>/dev/null || true)"
      if [[ "$actual" == "$EXPECTED_EXIT_IP" ]]; then
        egress_ok=1
        break
      fi
      sleep 2
    done
    [[ "$egress_ok" == "1" ]] || { echo "client_${idx}_egress=unexpected" >&2; exit 1; }
  fi
  if ! printf preflight | curl --fail --silent --show-error --connect-timeout 8 --max-time 30 \
    --request POST --data-binary @- \
    --socks5-hostname "127.0.0.1:${port}" \
    "${UPLOAD_URL%/}/preflight-${idx}" >/dev/null 2>&1; then
    echo "client_${idx}_upload_preflight=failed" >&2
    exit 1
  fi
  echo "client_${idx}_ready=ok"
done <"$ports_file"

run_user() {
  local idx="$1"
  local port="$2"
  local result_file="${REMOTE_TMP}/results/user-${idx}.result"
  local download_ok=0
  local download_fail=0
  local upload_ok=0
  local upload_fail=0
  local started="$SECONDS"
  local deadline=$((SECONDS + DURATION_SECONDS))
  local old_ifs url success attempt speed speed_int measured_bps max_upload_bps
  while [[ "$SECONDS" -lt "$deadline" ]]; do
    success=0
    old_ifs="$IFS"
    IFS=','
    for url in $TRAFFIC_URLS; do
      IFS="$old_ifs"
      url="${url#"${url%%[![:space:]]*}"}"
      url="${url%"${url##*[![:space:]]}"}"
      [[ -n "$url" ]] || continue
      for attempt in 1 2; do
        if curl --fail --silent --show-error --connect-timeout 8 \
          --max-time "$TRAFFIC_MAX_TIME" --limit-rate "$TRAFFIC_RATE" \
          --socks5-hostname "127.0.0.1:${port}" "$url" >/dev/null 2>&1; then
          success=1
          break
        fi
        sleep 2
      done
      [[ "$success" == "1" ]] && break
    done
    IFS="$old_ifs"
    if [[ "$success" == "1" ]]; then
      download_ok=$((download_ok + 1))
    else
      download_fail=$((download_fail + 1))
      sleep 5
    fi
    speed="$(head -c "$UPLOAD_BYTES" /dev/zero | curl --fail --silent --show-error \
      --connect-timeout 8 --max-time "$UPLOAD_MAX_TIME" \
      --request POST --data-binary @- \
      --socks5-hostname "127.0.0.1:${port}" \
      -o /dev/null -w '%{speed_upload}' \
      "${UPLOAD_URL%/}/user-${idx}" 2>/dev/null || true)"
    speed_int="${speed%.*}"
    if [[ "$speed_int" =~ ^[0-9]+$ && "$speed_int" -gt 0 ]]; then
      measured_bps=$((speed_int * 8))
      max_upload_bps=$((RATE_LIMIT_BPS * 3))
      if [[ "$RATE_LIMIT_BPS" =~ ^[1-9][0-9]*$ && "$measured_bps" -le "$max_upload_bps" ]]; then
        upload_ok=$((upload_ok + 1))
      else
        upload_fail=$((upload_fail + 1))
      fi
    else
      upload_fail=$((upload_fail + 1))
    fi
  done
  if [[ "$download_ok" -le 0 ]]; then
    download_fail=$((download_fail + 1))
  fi
  if [[ "$upload_ok" -le 0 ]]; then
    upload_fail=$((upload_fail + 1))
  fi
  printf 'user_%s download_ok=%s download_fail=%s upload_ok=%s upload_fail=%s seconds=%s\n' \
    "$idx" "$download_ok" "$download_fail" "$upload_ok" "$upload_fail" "$((SECONDS - started))" >"$result_file"
}

user_pids=()
while IFS=$'\t' read -r idx user_id xray_key port; do
  run_user "$idx" "$port" &
  user_pids+=("$!")
done <"$ports_file"

user_wait_failed=0
for pid in "${user_pids[@]}"; do
  if ! wait "$pid"; then
    user_wait_failed=1
  fi
done
[[ "$user_wait_failed" == "0" ]] || exit 1

cat "${REMOTE_TMP}"/results/*.result
