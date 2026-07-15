#!/usr/bin/env bash
# 用途：本地静态验证真实入站矩阵不会复用 Cloudflare/CDN 入口。
# 范围：只使用临时订阅与假 inventory，不访问网络、不读取私有 env。
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

subscription="$tmp_dir/subscription.yaml"
configs="$tmp_dir/configs"
inventory="$tmp_dir/inventory.json"

cat >"$subscription" <<'YAML'
mixed-port: 7890
proxies:
  - name: cf-vless-ws
    type: vless
    server: edge.example.test
    port: 443
    uuid: 00000000-0000-0000-0000-000000000001
    network: ws
    tls: true
    ws-opts:
      path: /edge
      headers:
        Host: edge.example.test
  - name: direct-vless
    type: vless
    server: direct.example.test
    port: 34200
    uuid: 00000000-0000-0000-0000-000000000002
    network: tcp
    tls: false
YAML

python3 scripts/real-access-inbound-matrix-client-configs.py \
  build "$subscription" "$configs" 33180 vless >"$tmp_dir/build.out"
grep -Fq "vless:33180" "$tmp_dir/build.out"
python3 - "$configs/vless.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    config = json.load(fh)
outbound = config["outbounds"][0]
assert outbound["settings"]["vnext"][0]["address"] == "direct.example.test"
assert outbound["settings"]["vnext"][0]["port"] == 34200
PY

cat >"$subscription" <<'YAML'
mixed-port: 7890
proxies:
  - name: server_4-vless-stale-high-port
    type: vless
    server: edge.example.test
    port: 34200
    uuid: 00000000-0000-0000-0000-000000000004
    network: tcp
    tls: false
  - name: real-matrix-vless-direct
    type: vless
    server: direct.example.test
    port: 34200
    uuid: 00000000-0000-0000-0000-000000000002
    network: tcp
    tls: false
YAML

rm -rf "$configs"
python3 scripts/real-access-inbound-matrix-client-configs.py \
  build "$subscription" "$configs" 33180 --preferred-port vless=34200 vless >"$tmp_dir/build-same-port.out"
python3 - "$configs/vless.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    config = json.load(fh)
outbound = config["outbounds"][0]
assert outbound["settings"]["vnext"][0]["address"] == "direct.example.test"
PY

cat >"$subscription" <<'YAML'
mixed-port: 7890
proxies:
  - name: cf-vless-grpc
    type: vless
    server: edge.example.test
    port: 443
    uuid: 00000000-0000-0000-0000-000000000001
    network: grpc
    tls: true
    grpc-opts:
      grpc-service-name: xrayc
YAML

if python3 scripts/real-access-inbound-matrix-client-configs.py \
  build "$subscription" "$configs" 33180 vless >/dev/null 2>&1; then
  echo "real-access-inbound-matrix-static: ordinary matrix accepted CDN-only entry" >&2
  exit 1
fi

for index in 1 2 3 4; do
  : >"$tmp_dir/server-${index}.pass"
done
python3 - "$inventory" "$tmp_dir" <<'PY'
import json
import sys

inventory_path, tmp_dir = sys.argv[1:3]
targets = []
for index in range(1, 5):
    target = {
        "alias": f"server_{index}",
        "ssh_host": f"192.0.2.{index}",
        "ssh_user": "root",
        "ssh_port": 22,
        "ssh_password_file": f"{tmp_dir}/server-{index}.pass",
    }
    targets.append(target)
targets[3]["public_domain"] = "edge.example.test"
targets[3]["cdn_provider"] = "cloudflare"
with open(inventory_path, "w", encoding="utf-8") as fh:
    json.dump({"targets": targets}, fh)
PY

common_env=(
  XRAYC_REAL_ACCESS_INBOUND_MATRIX_LOAD_ENV_FILE=0
  BASE_URL=https://control.example.test
  SUB_TOKEN=demo-token
  XRAYC_REAL_E2E_INVENTORY="$inventory"
  REAL_ACCESS_INBOUND_MATRIX_PROTOCOLS=vless
  REAL_ACCESS_INBOUND_MATRIX_PUBLIC_IP_URL=https://api.ipify.org
  REAL_ACCESS_INBOUND_MATRIX_TRAFFIC_URL=https://api.ipify.org
)

env "${common_env[@]}" \
  REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET=server_1 \
  REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET=server_2 \
  bash scripts/real-access-inbound-matrix-e2e.sh --validate-only >/dev/null

if env "${common_env[@]}" \
  REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET=server_1 \
  REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET=server_4 \
  bash scripts/real-access-inbound-matrix-e2e.sh --validate-only >/dev/null 2>&1; then
  echo "real-access-inbound-matrix-static: ordinary matrix accepted server_4 access target" >&2
  exit 1
fi

if env "${common_env[@]}" \
  REAL_ACCESS_INBOUND_MATRIX_CLIENT_TARGET=edge.example.test \
  REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET=server_2 \
  bash scripts/real-access-inbound-matrix-e2e.sh --validate-only >/dev/null 2>&1; then
  echo "real-access-inbound-matrix-static: ordinary matrix accepted Cloudflare hostname target" >&2
  exit 1
fi

echo "real-access-inbound-matrix-static: passed"
