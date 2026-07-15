#!/usr/bin/env bash
# 用途：验证真实第三方中继矩阵订阅脱敏断言不会把入口 server 误判为出口材料。
# 范围：只运行本地 YAML fixture，不读取私有 env、不访问远端、不输出敏感值。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

die() {
  echo "redaction-test: $*" >&2
  exit 1
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" || "$value" == "<"*">" ]]
}

# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/lib/real-access-third-party-matrix-relay/assertions.sh"

write_literal_file() {
  local output="$1"
  {
    printf '%s\n' "relay.example.test"
    printf '%s\n' "dummy-upstream-literal"
  } > "$output"
}

assert_fails() {
  local yaml_file="$1"
  local literal_file="$2"
  if (assert_named_proxy_missing_literals "$yaml_file" "$literal_file" "relay-line" "expected failure" "relay.example.test" "25443") 2>/dev/null; then
    die "sensitive non-server field was not rejected"
  fi
}

literals="$TMP_DIR/literals.txt"
write_literal_file "$literals"

allowed_yaml="$TMP_DIR/allowed.yaml"
cat > "$allowed_yaml" <<'YAML'
proxies:
  - name: relay-line
    type: vless
    server: relay.example.test
    port: 25443
    uuid: access-user-uuid
YAML

assert_named_proxy_missing_literals "$allowed_yaml" "$literals" "relay-line" "unexpected leak" "relay.example.test" "25443"

bad_sni_yaml="$TMP_DIR/bad-sni.yaml"
cat > "$bad_sni_yaml" <<'YAML'
proxies:
  - name: relay-line
    type: vless
    server: relay.example.test
    port: 25443
    sni: relay.example.test
    uuid: access-user-uuid
YAML
assert_fails "$bad_sni_yaml" "$literals"

bad_secret_yaml="$TMP_DIR/bad-secret.yaml"
cat > "$bad_secret_yaml" <<'YAML'
proxies:
  - name: relay-line
    type: vless
    server: relay.example.test
    port: 25443
    password: dummy-upstream-literal
YAML
assert_fails "$bad_secret_yaml" "$literals"

wrong_port_yaml="$TMP_DIR/wrong-port.yaml"
cat > "$wrong_port_yaml" <<'YAML'
proxies:
  - name: relay-line
    type: vless
    server: relay.example.test
    port: 443
    uuid: access-user-uuid
YAML
assert_fails "$wrong_port_yaml" "$literals"

allowed_api_json="$TMP_DIR/allowed-api.json"
cat > "$allowed_api_json" <<'JSON'
{
  "data": {
    "access_lines": [
      {
        "name": "relay-line",
        "line_group_name": "relay-group",
        "listen_host": "relay.example.test",
        "listen_port": 25443
      }
    ]
  }
}
JSON

assert_named_proxy_missing_literals "$allowed_api_json" "$literals" "relay-line" "unexpected api leak" "relay.example.test" "25443"

bad_api_json="$TMP_DIR/bad-api.json"
cat > "$bad_api_json" <<'JSON'
{
  "data": {
    "access_lines": [
      {
        "name": "relay-line",
        "line_group_name": "relay-group",
        "listen_host": "relay.example.test",
        "listen_port": 25443,
        "sni": "relay.example.test"
      }
    ]
  }
}
JSON
assert_fails "$bad_api_json" "$literals"
