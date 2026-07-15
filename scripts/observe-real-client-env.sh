#!/usr/bin/env bash
# 用途：观察真实客户端环境并生成脱敏后的连通性与出口诊断结果。
# 范围：面向私有真实环境运行，不负责创建资源或修改远端配置。
# 输入：读取 BASE_URL、订阅信息、客户端代理和公开 IP 探测地址。
# 输出：输出粗粒度状态、HTTP 结果和出口判断，避免泄露敏感 URL。
# 依赖：使用 curl、real-e2e-lib.sh 和本地临时目录完成探测。
# 安全：不得打印订阅 token、代理凭据、真实主机或完整响应体。
# 约束：仅用于人工排查真实客户端链路，不作为发布写入动作。
# 行为：按可用输入检查控制面、订阅下载和代理出口可达性。
# 失败：必需环境缺失、URL scheme 非法或探测失败时返回非零。
# 维护：新增客户端观测项时需保持输出脱敏和临时文件清理。
set -euo pipefail
IFS=$'\n\t'

if [[ $- == *x* ]]; then
  set +x
  echo "observe-real-client-env: disabled shell xtrace to avoid printing private values" >&2
fi

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
PUBLIC_IP_PROBE_URL="${PUBLIC_IP_PROBE_URL:-https://api.ipify.org}"
CURL_TIMEOUT="${CURL_TIMEOUT:-30}"
OVERWRITE="${XRAYC_OBSERVE_OVERWRITE:-0}"

# 客户端代理、出口 IP 都是私有运行观测材料，文件默认只允许当前用户读取。
umask 077

usage() {
  cat <<'EOF'
usage: bash scripts/observe-real-client-env.sh

Observe real client proxy egress IPs and write EXPECTED_EXIT_IP* variables into
the private real-release env file. The script never prints proxy URLs or IPs.

Environment:
  XRAYC_REAL_RELEASE_ENV_FILE  target private env file, default .env.real-release
  PUBLIC_IP_PROBE_URL          public IP echo endpoint, default https://api.ipify.org
  XRAYC_OBSERVE_OVERWRITE=1    overwrite existing EXPECTED_EXIT_IP* values
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-target-name* ]] && return 0
  [[ "$value" == *private-inventory-path* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *upstream-host* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *your-secret* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *example* ]] && return 0
  return 1
}

value_ready() {
  local value="${1:-}"
  [[ -n "$value" ]] || return 1
  value_is_placeholder "$value" && return 1
  return 0
}

upsert_env_value() {
  local name="$1"
  local value="${2:-}"
  [[ -n "$value" ]] || return 0
  python3 - "$REAL_RELEASE_ENV_FILE" "$name" "$value" <<'PY'
from pathlib import Path
import os
import re
import shlex
import sys

path = Path(sys.argv[1])
name = sys.argv[2]
value = sys.argv[3]
line = f"{name}={shlex.quote(value)}\n"
pattern = re.compile(rf"^{re.escape(name)}=")

lines = []
found = False
if path.exists():
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
deduped = []
for existing in lines:
    if pattern.match(existing):
        if not found:
            deduped.append(line)
            found = True
        continue
    deduped.append(existing)
lines = deduped
if not found:
    if lines and not lines[-1].endswith("\n"):
        lines[-1] += "\n"
    lines.append(line)
path.write_text("".join(lines), encoding="utf-8")
os.chmod(path, 0o600)
PY
}

observe_proxy_ip() {
  local proxy_url="$1"
  local output_file="$2"
  if ! curl --fail --silent --show-error --location \
    --max-time "$CURL_TIMEOUT" \
    --proxy "$proxy_url" \
    "$PUBLIC_IP_PROBE_URL" > "$output_file" 2>/dev/null; then
    echo "observe-real-client-env: public IP probe failed; details redacted" >&2
    return 1
  fi
  python3 - "$output_file" <<'PY'
import ipaddress
import sys

value = open(sys.argv[1], "r", encoding="utf-8").read().strip()
try:
    ipaddress.ip_address(value)
except ValueError:
    raise SystemExit("public IP probe returned an invalid IP value")
print(value)
PY
}

observe_pair() {
  local proxy_var="$1"
  local expected_var="$2"
  local proxy_value="${!proxy_var:-}"
  local expected_value="${!expected_var:-}"
  local observed_file="$tmp_dir/${expected_var}.txt"
  local observed_ip=""

  if ! value_ready "$proxy_value"; then
    echo "observe-real-client-env: skipped ${expected_var} (${proxy_var} not ready)"
    return 0
  fi

  observed_ip="$(observe_proxy_ip "$proxy_value" "$observed_file")"
  if value_ready "$expected_value" && [[ "$OVERWRITE" != "1" ]]; then
    if [[ "$observed_ip" != "$expected_value" ]]; then
      echo "observe-real-client-env: ${expected_var} differs from observed egress IP; values redacted" >&2
      return 1
    fi
    echo "observe-real-client-env: verified ${expected_var}"
    return 0
  fi

  upsert_env_value "$expected_var" "$observed_ip"
  echo "observe-real-client-env: wrote ${expected_var}"
}

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

observed=0
for suffix in BASE SOCKS HTTP VLESS TROJAN SHADOWSOCKS HY2; do
  case "$suffix" in
    BASE)
      proxy_var="CLIENT_PROXY_URL"
      expected_var="EXPECTED_EXIT_IP"
      ;;
    *)
      proxy_var="CLIENT_PROXY_URL_${suffix}"
      expected_var="EXPECTED_EXIT_IP_${suffix}"
      ;;
  esac
  if value_ready "${!proxy_var:-}"; then
    observed=$((observed + 1))
  fi
  observe_pair "$proxy_var" "$expected_var"
done

if [[ "$observed" -eq 0 ]]; then
  echo "observe-real-client-env: no ready CLIENT_PROXY_URL* values found"
else
  echo "observe-real-client-env: completed without printing proxy URLs or IPs"
fi
