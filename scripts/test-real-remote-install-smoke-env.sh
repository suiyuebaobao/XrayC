#!/usr/bin/env bash
# 用途：验证 Agent 安装说明和节点侧安装脚本使用 agent 先安装的凭据模型。
# 范围：静态检查脚本变量传递，不读取私有环境、不访问网络。
# 失败：安装脚本继续要求平台先创建节点会破坏 agent 先安装流程。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

install_script="scripts/deploy-access-agent.sh"
common_script="scripts/lib/deploy-access-agent/common.sh"
script="scripts/real-remote-access-deploy-e2e.sh"
agent_first_script="scripts/real-agent-first-nodes-e2e.sh"
inbound_prepare_script="scripts/lib/real-access-inbound-matrix/db-prepare.sh"
inbound_prepare_sql="scripts/lib/real-access-inbound-matrix/db-prepare.psql"
targets_helper="scripts/lib/prepare-real-protocol-matrix-assets/targets.sh"
relay_matrix_script="scripts/real-access-third-party-matrix-relay-e2e.sh"
relay_matrix_client_script="scripts/lib/real-access-third-party-matrix-relay/client.sh"
relay_pool_script="scripts/real-v2-relay-pool-e2e.sh"
relay_pool_subscription_script="scripts/lib/real-v2-relay-pool/subscription.sh"
relay_pool_client_runtime_script="scripts/lib/real-v2-relay-pool/client.sh"
relay_pool_client_script="scripts/lib/real-v2-relay-pool/client-config.py"
! grep -Fq 'require_env XRAYC_NODE_ID' "$install_script"
! grep -Fq 'require_env XRAYC_NODE_TOKEN' "$install_script"
grep -Fq 'ensure_node_credentials' "$install_script"
grep -Fq 'XRAYC_AGENT_AUTH_CODE' "$common_script"
grep -Fq 'xrayc-agent-v1:' "$common_script"
grep -Fq '节点鉴权码' "$install_script"
grep -Fq '$1 == "default"' "$install_script"
! grep -Fq '/ default /' "$install_script"
grep -Fq 'XRAYC_REMOTE_E2E_USE_INVENTORY_NODE_CREDENTIALS' "$script"
grep -Fq 'XRAYC_REMOTE_E2E_EXPECTED_LISTEN_PORTS_OVERRIDE' "$script"
grep -Fq 'NODE_ID=""' "$script"
grep -Fq 'NODE_TOKEN=""' "$script"
grep -Fq 'ACCESS_LINE_ID=""' "$script"
grep -Fq 'EXIT_ENDPOINT_ID=""' "$script"
grep -Fq 'SUB_TOKEN=""' "$script"
grep -Fq 'SUBSCRIPTION_URL=""' "$script"
grep -Fq 'empty access node has no expected listen ports; accepting running agent without state version' \
  scripts/lib/deploy-access-agent/deploy.sh
grep -Fq 'empty access node has no expected listen ports; skipping remote state hash check' \
  "$script"
grep -Fq 'XRAYC_REMOTE_E2E_SERVICE_WAIT_SECONDS:-60' "$script"
grep -Fq 'until test \"\$(\$dcmd inspect -f' "$script"
grep -Fq 'sleep 2; done' "$script"
grep -Fq 'XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS' "$script"
grep -Fq 'public_domain' "$agent_first_script"
grep -Fq 'tls_cert_domains' "$agent_first_script"
grep -Fq 'delete_existing_alias_nodes' "$agent_first_script"
grep -Fq 'EXPLICIT_ACCESS_TARGET' scripts/real-access-inbound-matrix-e2e.sh
grep -Fq 'EXPLICIT_CLIENT_TARGET' scripts/real-access-inbound-matrix-e2e.sh
grep -Fq "ctx.access_node_hint IS NULL" "$inbound_prepare_sql"
grep -Fq "lower(n.name) LIKE '%' || ctx.access_node_hint" "$inbound_prepare_sql"
if grep -Fq '"tls_cert_domains": [os.environ["TRANSIT_PUBLIC_HOST"]]' "$relay_matrix_script"; then
  echo "real-remote-install-smoke-env: relay matrix must not request TLS certs for IP hosts" >&2
  exit 1
fi
grep -Fq 'CLIENT_REALITY_PUBLIC_KEY' "$relay_matrix_client_script"
grep -Fq 'realitySettings' "$relay_matrix_client_script"
grep -Fq 'CLIENT_SERVER_NAME' "$relay_matrix_client_script"
legacy_line_group_lines="$(rg -n "/api/admin/line-groups[^\"'[:space:]]*/lines" \
  scripts/real-*.sh scripts/lib/real-* -g '*.sh' -g '*.py' || true)"
if [[ -n "$legacy_line_group_lines" ]]; then
  echo "real-remote-install-smoke-env: real-v2 relay pool must bind line groups through binding nodes" >&2
  printf '%s\n' "$legacy_line_group_lines" >&2
  exit 1
fi
grep -Fq '/api/admin/access-entries' "$relay_pool_script"
grep -Fq '/exit-bindings' "$relay_pool_script"
grep -Fq '/binding-nodes' scripts/lib/real-v2-relay-pool/rebind.sh
grep -Fq 'CLIENT_REALITY_PUBLIC_KEY' "$relay_pool_client_script"
grep -Fq 'realitySettings' "$relay_pool_client_script"
grep -Fq 'CLIENT_REALITY_SHORT_ID' "$relay_pool_client_runtime_script"
grep -Fq 'CLIENT_SERVER_NAME' "$relay_pool_client_runtime_script"
grep -Fq 'CLIENT_REALITY_SHORT_ID' "$relay_matrix_client_script"
grep -Fq 'CLIENT_SERVER_NAME' "$relay_matrix_client_script"

# shellcheck source=scripts/lib/deploy-access-agent/common.sh
. "$common_script"
# BUG-E：含运行时 $XRAYC_XRAY_CONFIG 的 xray-test 命令经 env_file 注入在 compose v1/v2 语义不一致
# （v1 不插值保留单 $、v2 会插值把未定义的 $XRAYC_XRAY_CONFIG 插空 → -config "" → agent 永不收敛），
# 无单一转义两侧都对。根除依赖：部署脚本默认不再无条件把该命令写进 env_file，改由 agent config.rs
# 自带等价默认、由 agent 自己 sh -c 展开（不经 compose 插值）。此处校验该默认注入已被移除。
if grep -Eq '^  write_env_line[[:space:]]+XRAYC_XRAY_TEST_COMMAND([[:space:]]|$)' "$install_script"; then
  echo "real-remote-install-smoke-env: 部署脚本默认仍无条件注入 XRAYC_XRAY_TEST_COMMAND（BUG-E：compose v2 会把 \$XRAYC_XRAY_CONFIG 插空）" >&2
  exit 1
fi

# shellcheck source=scripts/lib/prepare-real-protocol-matrix-assets/targets.sh
. "$targets_helper"
cat >"$tmp_dir/source.json" <<'JSON'
{
  "targets": [
    {
      "alias": "server_1",
      "ssh_host": "192.0.2.10",
      "ssh_user": "root",
      "ssh_port": "22",
      "public_domain": "edge.example.test 中文备注"
    }
  ]
}
JSON
write_inventory_json "$tmp_dir/source.json" "$tmp_dir/inventory.json"
python3 - "$tmp_dir/inventory.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
assert target["public_domain"] == "edge.example.test"
assert target["tls_cert_domains"] == ["edge.example.test"]
PY

parser_private_dir="$tmp_dir/private"
mkdir -p "$parser_private_dir"
cat >"$tmp_dir/plain-accounts.txt" <<'EOF_PLAIN'
192.0.2.20
root
test-inline-pass 中文备注
edge.example.test 中文备注
EOF_PLAIN
prepare_targets_from_account_file \
  "$tmp_dir/plain-accounts.txt" \
  "$tmp_dir/plain-targets.json" \
  "$tmp_dir/plain-targets.env" \
  "$tmp_dir" \
  "$parser_private_dir" \
  "parserplain"
python3 - "$tmp_dir/plain-targets.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
with open(target["ssh_password_file"], encoding="utf-8") as fh:
    password = fh.read()
assert password == "test-inline-pass"
assert target["public_domain"] == "edge.example.test"
PY

cat >"$tmp_dir/plain-quoted-accounts.txt" <<'EOF_PLAIN_QUOTED'
192.0.2.22
root
"quoted pass with spaces" 中文备注
edge-quoted.example.test
EOF_PLAIN_QUOTED
prepare_targets_from_account_file \
  "$tmp_dir/plain-quoted-accounts.txt" \
  "$tmp_dir/plain-quoted-targets.json" \
  "$tmp_dir/plain-quoted-targets.env" \
  "$tmp_dir" \
  "$parser_private_dir" \
  "parserquoted"
python3 - "$tmp_dir/plain-quoted-targets.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
with open(target["ssh_password_file"], encoding="utf-8") as fh:
    password = fh.read()
assert password == "quoted pass with spaces"
PY

cat >"$tmp_dir/plain-special-accounts.txt" <<'EOF_PLAIN_SPECIAL'
192.0.2.23
root
abcDEF123+/=@#:_.$!- 中文备注
edge-special.example.test
EOF_PLAIN_SPECIAL
prepare_targets_from_account_file \
  "$tmp_dir/plain-special-accounts.txt" \
  "$tmp_dir/plain-special-targets.json" \
  "$tmp_dir/plain-special-targets.env" \
  "$tmp_dir" \
  "$parser_private_dir" \
  "parserspecial"
python3 - "$tmp_dir/plain-special-targets.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
with open(target["ssh_password_file"], encoding="utf-8") as fh:
    password = fh.read()
assert password == "abcDEF123+/=@#:_.$!-"
PY

cat >"$tmp_dir/dotenv-accounts.env" <<'EOF_DOTENV'
SERVER_1_SSH_HOST=192.0.2.21
SERVER_1_SSH_USER=root
SERVER_1_SSH_PASSWORD=test-dotenv-pass 中文备注
SERVER_1_PUBLIC_DOMAIN=edge-dotenv.example.test 中文备注
EOF_DOTENV
prepare_targets_from_account_file \
  "$tmp_dir/dotenv-accounts.env" \
  "$tmp_dir/dotenv-targets.json" \
  "$tmp_dir/dotenv-targets.env" \
  "$tmp_dir" \
  "$parser_private_dir" \
  "parserdotenv"
python3 - "$tmp_dir/dotenv-targets.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
with open(target["ssh_password_file"], encoding="utf-8") as fh:
    password = fh.read()
assert password == "test-dotenv-pass"
assert target["public_domain"] == "edge-dotenv.example.test"
PY

cat >"$tmp_dir/json-accounts.json" <<'EOF_JSON_ACCOUNTS'
{
  "targets": [
    {
      "alias": "server_1",
      "ssh_host": "192.0.2.24",
      "ssh_user": "root",
      "ssh_password": "test-json-pass 中文备注",
      "public_domain": "edge-json.example.test 中文备注"
    }
  ]
}
EOF_JSON_ACCOUNTS
prepare_targets_from_account_file \
  "$tmp_dir/json-accounts.json" \
  "$tmp_dir/json-targets.json" \
  "$tmp_dir/json-targets.env" \
  "$tmp_dir" \
  "$parser_private_dir" \
  "parserjson"
python3 - "$tmp_dir/json-targets.json" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
target = data["targets"][0]
with open(target["ssh_password_file"], encoding="utf-8") as fh:
    password = fh.read()
assert password == "test-json-pass"
assert target["public_domain"] == "edge-json.example.test"
PY

# shellcheck source=scripts/lib/real-v2-relay-pool/subscription.sh
. "$relay_pool_subscription_script"
cat >"$tmp_dir/relay-pool-reality.yaml" <<'EOF_RELAY_POOL_REALITY'
proxies:
  - name: real-v2-pool-target
    type: vless
    server: transit.example.test
    port: 443
    uuid: 11111111-1111-1111-1111-111111111111
    network: tcp
    tls: true
    servername: www.example.test
    client-fingerprint: chrome
    flow: xtls-rprx-vision
    reality-opts:
      public-key: reality-public-key-test
      short-id: abcd1234
EOF_RELAY_POOL_REALITY
write_subscription_client_env \
  "$tmp_dir/relay-pool-reality.yaml" \
  "real-v2-pool-target" \
  "transit.example.test" \
  "exit-a.example.test" \
  "exit-b.example.test" \
  "$tmp_dir/relay-pool-client.env"
set -a
# shellcheck disable=SC1090
. "$tmp_dir/relay-pool-client.env"
set +a
CLIENT_SOCKS_PORT=32100 \
CLIENT_UDP_PORT=32101 \
UDP_TARGET_HOST=udp.example.test \
UDP_TARGET_PORT=32102 \
python3 "$relay_pool_client_script" "$tmp_dir/relay-pool-client.json"
python3 - "$tmp_dir/relay-pool-client.json" <<'PY'
import json
import sys

config = json.load(open(sys.argv[1], encoding="utf-8"))
outbound = config["outbounds"][0]
stream = outbound["streamSettings"]
user = outbound["settings"]["vnext"][0]["users"][0]
assert stream["security"] == "reality"
assert stream["realitySettings"]["publicKey"] == "reality-public-key-test"
assert stream["realitySettings"]["serverName"] == "www.example.test"
assert stream["realitySettings"]["shortId"] == "abcd1234"
assert user["flow"] == "xtls-rprx-vision"
PY

# shellcheck source=scripts/lib/real-access-third-party-matrix-relay/client.sh
. "$relay_matrix_client_script"
cat >"$tmp_dir/matrix-reality.yaml" <<'EOF_MATRIX_REALITY'
proxies:
  - name: matrix-reality-target
    type: vless
    server: transit-matrix.example.test
    port: 443
    uuid: 22222222-2222-2222-2222-222222222222
    network: tcp
    tls: true
    servername: reality.example.test
    client-fingerprint: chrome
    flow: xtls-rprx-vision
    reality-opts:
      public-key: matrix-reality-public-key-test
      short-id: dcba4321
  - name: matrix-xhttp-target
    type: vless
    server: transit-matrix.example.test
    port: 8443
    uuid: 33333333-3333-3333-3333-333333333333
    network: xhttp
    tls: true
    servername: xhttp.example.test
    packet-encoding: xudp
    xhttp-opts:
      path: /xrayc-test
      mode: stream-one
EOF_MATRIX_REALITY
PH2="transit-matrix.example.test" extract_subscription_client_env \
  "$tmp_dir/matrix-reality.yaml" \
  "matrix-reality-target" \
  "$tmp_dir/matrix-reality.env"
PH2="transit-matrix.example.test" extract_subscription_client_env \
  "$tmp_dir/matrix-reality.yaml" \
  "matrix-xhttp-target" \
  "$tmp_dir/matrix-xhttp.env"
python3 - "$tmp_dir/matrix-reality.env" "$tmp_dir/matrix-xhttp.env" <<'PY'
import shlex
import sys

def read_env(path):
    out = {}
    for raw in open(path, encoding="utf-8"):
        if not raw.strip():
            continue
        key, value = raw.rstrip("\n").split("=", 1)
        out[key] = shlex.split(value)[0] if value else ""
    return out

reality = read_env(sys.argv[1])
xhttp = read_env(sys.argv[2])
assert reality["CLIENT_SECURITY"] == "reality"
assert reality["CLIENT_REALITY_PUBLIC_KEY"] == "matrix-reality-public-key-test"
assert reality["CLIENT_REALITY_SHORT_ID"] == "dcba4321"
assert reality["CLIENT_SERVER_NAME"] == "reality.example.test"
assert xhttp["CLIENT_SECURITY"] == "tls"
assert xhttp["CLIENT_NETWORK"] == "xhttp"
assert xhttp["CLIENT_PACKET_ENCODING"] == "xudp"
assert xhttp["CLIENT_XHTTP_PATH"] == "/xrayc-test"
PY
echo "real-remote-install-smoke-env: passed"
