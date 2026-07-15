#!/usr/bin/env bash
# 用途：验证真实 V2 主链路 inventory 解析允许用环境 BASE_URL 作为控制面回退。
# 范围：只解析临时假 inventory，不访问网络、不读取私有清单。
# 失败：缺少回退逻辑时，内联解析器会报 inventory target missing BASE_URL。
# 同时覆盖当前私有角色：server_4 是 Cloudflare 目标，relay pool
# 第二出口宿主应复用 server_3 的不同端口，同时仍导出 server_4 元数据。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

inventory="$tmp_dir/inventory.json"
targets_env="$tmp_dir/targets.env"
upstream_targets_env="$tmp_dir/upstream-targets.env"
bad_server1_inventory="$tmp_dir/bad-server1-inventory.json"
bad_orange_cloud_inventory="$tmp_dir/bad-orange-cloud-inventory.json"

python3 - "$inventory" <<'PY'
import json
import sys

inventory = {
    "targets": [
        {
            "alias": f"server_{index}",
            "ssh_host": f"192.0.2.{index}",
            "ssh_user": "root",
            "ssh_port": 22,
            "ssh_password_file": f"/tmp/server-{index}.pass",
        }
        for index in range(1, 5)
    ]
}
inventory["targets"][3]["public_domain"] = "edge4.example.test 这是cloudflare域名"
inventory["targets"][3]["cdn_provider"] = "cloudflare"
inventory["targets"][2]["public_domain"] = "direct3.example.test"
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(inventory, fh)
PY

INVENTORY="$inventory" TMP_DIR="$tmp_dir" BASE_URL="https://control.example.test" bash -c '
  source scripts/lib/real-v2-relay-pool/targets.sh
  write_relay_pool_targets_env
'
grep -Fq "BASE_URL=https://control.example.test" "$targets_env"
grep -Fq "A1=server_1" "$targets_env"
grep -Fq "PH1=192.0.2.1" "$targets_env"
grep -Fq "EXIT_B_REMOTE_INDEX=3" "$targets_env"
grep -Fq "EXIT_B_ALIAS=server_3" "$targets_env"
grep -Fq "A4=server_4" "$targets_env"
grep -Fq "PH4=edge4.example.test" "$targets_env"
grep -Fq "EXIT_B_PUBLIC_HOST=192.0.2.3" "$targets_env"
grep -Fq "EXIT_B_UDP_TARGET_HOST=192.0.2.3" "$targets_env"
grep -Fq "H4=192.0.2.4" "$targets_env"
grep -Fq "U4=root" "$targets_env"
grep -Fq "P4=22" "$targets_env"
grep -Fq "PF4=/tmp/server-4.pass" "$targets_env"

INVENTORY="$inventory" TARGETS_ENV="$upstream_targets_env" bash -c '
  source scripts/lib/real-protocol-upstream-lab/common.sh
  load_inventory_targets
'
grep -Fq "BASIC_EXIT_REMOTE_INDEX=3" "$upstream_targets_env"
grep -Fq "TLS_EXIT_REMOTE_INDEX=3" "$upstream_targets_env"
grep -Fq "TLS_EXIT_ALIAS=server_3" "$upstream_targets_env"

python3 - "$inventory" "$bad_orange_cloud_inventory" <<'PY'
import json
import sys

source, target_path = sys.argv[1:3]
with open(source, encoding="utf-8") as fh:
    inventory = json.load(fh)
inventory["targets"][2]["remark"] = "orange cloud edge"
with open(target_path, "w", encoding="utf-8") as fh:
    json.dump(inventory, fh)
PY
if INVENTORY="$bad_orange_cloud_inventory" TARGETS_ENV="$tmp_dir/bad-orange-upstream-targets.env" bash -c '
  source scripts/lib/real-protocol-upstream-lab/common.sh
  load_inventory_targets
' 2>/dev/null; then
  echo "real-v2-inventory-fallback: TLS lab must not select orange cloud marked target" >&2
  exit 1
fi

python3 - "$inventory" "$bad_server1_inventory" <<'PY'
import json
import sys

source, target_path = sys.argv[1:3]
with open(source, encoding="utf-8") as fh:
    inventory = json.load(fh)
inventory["targets"][0]["cdn_provider"] = "cloudflare"
with open(target_path, "w", encoding="utf-8") as fh:
    json.dump(inventory, fh)
PY
if INVENTORY="$bad_server1_inventory" TARGETS_ENV="$tmp_dir/bad-upstream-targets.env" bash -c '
  source scripts/lib/real-protocol-upstream-lab/common.sh
  load_inventory_targets
' 2>/dev/null; then
  echo "real-v2-inventory-fallback: server_1 must not accept Cloudflare role" >&2
  exit 1
fi
echo "real-v2-inventory-fallback: passed"
