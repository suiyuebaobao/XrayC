#!/usr/bin/env bash
# 用途：验证真实 V2 中转池客户端脚本能从 Shadowsocks 订阅生成可用配置。
# 范围：只在本地构造临时订阅 YAML 和 Xray 客户端 JSON，不访问远端服务器。
# 输入：读取 real-v2-relay-pool 的订阅解析函数和客户端配置 helper。
# 输出：成功时打印 passed，失败时输出缺失的协议字段或配置断言。
# 安全：测试使用固定假主机和假凭据，不读取真实环境变量或私有密码文件。
# 约束：订阅只允许暴露中转入口，不允许把出口主机写入客户端环境。
# 行为：覆盖 Shadowsocks 协议、UDP dokodemo-door 入站和出站认证字段。
# 失败：用于阻断真实 E2E 回退到只支持 VLESS 客户端的旧路径。
# 维护：新增客户端协议分支时应在这里补充最小配置断言。
# shellcheck shell=bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# shellcheck source=scripts/lib/real-v2-relay-pool/subscription.sh
. "$ROOT_DIR/scripts/lib/real-v2-relay-pool/subscription.sh"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

subscription_yaml="$tmp_dir/subscription.yaml"
client_env="$tmp_dir/client.env"
client_config="$tmp_dir/client-config.json"

cat > "$subscription_yaml" <<'YAML'
proxies:
  - name: real-relay-line-test 1
    type: ss
    server: transit.example.test
    port: 31313
    cipher: 2022-blake3-aes-128-gcm
    password: root-secret:user-secret
    udp: true
YAML

write_subscription_client_env \
  "$subscription_yaml" \
  "real-relay-line-test" \
  "transit.example.test" \
  "exit-a.example.test" \
  "exit-b.example.test" \
  "$client_env"

set -a
# shellcheck disable=SC1090
. "$client_env"
set +a

CLIENT_SOCKS_PORT=31080 \
CLIENT_UDP_PORT=31081 \
UDP_TARGET_HOST=udp-target.example.test \
UDP_TARGET_PORT=31999 \
  python3 "$ROOT_DIR/scripts/lib/real-v2-relay-pool/client-config.py" "$client_config"

python3 - "$client_config" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    config = json.load(fh)

assert config["inbounds"][0]["protocol"] == "socks"
assert config["inbounds"][0]["settings"]["udp"] is True
udp_inbound = config["inbounds"][1]
assert udp_inbound["protocol"] == "dokodemo-door"
assert udp_inbound["port"] == 31081
assert udp_inbound["settings"]["address"] == "udp-target.example.test"
assert udp_inbound["settings"]["port"] == 31999
assert udp_inbound["settings"]["network"] == "udp"
outbound = config["outbounds"][0]
assert outbound["protocol"] == "shadowsocks"
settings = outbound["settings"]
assert settings["address"] == "transit.example.test"
assert settings["port"] == 31313
assert settings["method"] == "2022-blake3-aes-128-gcm"
assert settings["password"] == "root-secret:user-secret"
assert "uot" not in settings
assert "UoTVersion" not in settings
PY

if ! grep -Fq "client_udp_port" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"; then
  echo "UDP generator must take client_udp_port from CLIENT_UDP_PORT" >&2
  exit 1
fi
if ! grep -Fq "CLIENT_UDP_PORT" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"; then
  echo "rate-limit script must pass CLIENT_UDP_PORT into the UDP generator" >&2
  exit 1
fi
if grep -Fq "UDP ASSOCIATE" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"; then
  echo "UDP generator must use dokodemo-door CLIENT_UDP_PORT, not SOCKS UDP ASSOCIATE" >&2
  exit 1
fi
if grep -Fq "socks_address(target_host)" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"; then
  echo "UDP generator must not wrap packets in SOCKS UDP payloads" >&2
  exit 1
fi
grep -Fq "rate_limit_ifb_interface" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"
grep -Fq "classid 2:" "$ROOT_DIR/scripts/lib/real-v2-relay-pool/rate-limit.sh"

printf 'real-v2-shadowsocks-client-config: passed\n'
