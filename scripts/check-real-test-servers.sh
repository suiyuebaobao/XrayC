#!/usr/bin/env bash
# 用途：真实发布门禁前检查测试服务器资产是否还满足当前矩阵要求。
# 范围：读取私有 real-release env、账号文件和 inventory，验证前 N 台服务器 SSH 可达。
# 安全：输出只包含 target 序号和粗状态，不打印主机、账号、密码文件或密钥路径。
# 输入：XRAYC_REAL_RELEASE_ENV_FILE、XRAYC_REAL_E2E_INVENTORY、XRAYC_SERVER_ACCOUNT_FILE、XRAYC_REAL_TEST_SERVER_COUNT。
# 输出：通过时打印每台 target_N=ok 和汇总；失败时打印脱敏原因并返回非零。
# 依赖：python3、ssh；密码认证目标还需要 sshpass。
# 约束：未显式设置 N 时优先按账号文件推断服务器数量；真实矩阵至少需要三台。
# 行为：任一目标缺少 host/auth 文件、host 重复或 SSH 不通都会提前失败。
# 维护：不得在本脚本中输出 inventory 原始字段值。
# 本头部满足中文注释和文件说明要求。
set -euo pipefail
IFS=$'\n\t'
set +x

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

inventory="${XRAYC_REAL_E2E_INVENTORY:-}"
account_file="${XRAYC_SERVER_ACCOUNT_FILE:-}"
required_count="${XRAYC_REAL_TEST_SERVER_COUNT:-}"
timeout_seconds="${XRAYC_REAL_TEST_SERVER_SSH_TIMEOUT_SECONDS:-12}"
connect_timeout_seconds="${XRAYC_REAL_TEST_SERVER_CONNECT_TIMEOUT_SECONDS:-8}"

die() {
  printf 'real-test-server-assets: %s\n' "$*" >&2
  exit 1
}

[[ -n "$inventory" ]] || die "XRAYC_REAL_E2E_INVENTORY is required"
[[ -f "$inventory" ]] || die "inventory file is missing"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
command -v ssh >/dev/null 2>&1 || die "ssh is required"

tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

if [[ -z "$required_count" && -n "$account_file" && -f "$account_file" ]]; then
  required_count="$(
    python3 - "$account_file" <<'PY'
import json
import shlex
import sys

path = sys.argv[1]

def first_nonempty(*values):
    for value in values:
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""

def json_count(payload):
    source = payload.get("servers")
    if isinstance(source, dict):
        total = 0
        idx = 1
        while f"server_{idx}" in source:
            total += 1
            idx += 1
        return total
    targets = payload.get("targets", [])
    return len(targets) if isinstance(targets, list) else 0

def dotenv_count(lines):
    values = {}
    for raw in lines:
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        try:
            parsed = shlex.split(value.strip(), comments=False, posix=True)
            value = parsed[0] if len(parsed) == 1 else value.strip()
        except ValueError:
            value = value.strip().strip("'\"")
        values[key.strip()] = value
    total = 0
    idx = 1
    while first_nonempty(values.get(f"SERVER_{idx}_SSH_HOST"), values.get(f"SERVER_{idx}_HOST"), values.get(f"SERVER_{idx}_IP")):
        total += 1
        idx += 1
    return total

def plain_count(lines):
    items = []
    for raw in lines:
        line = raw.strip()
        if not line or line.startswith("#") or len(line) == 1:
            continue
        items.append(line)

    def is_url(value):
        return "://" in value

    def looks_like_host(value):
        return not is_url(value) and not any(ch.isspace() for ch in value) and ("." in value or ":" in value)

    def looks_like_user(value):
        return bool(value.strip()) and not is_url(value) and not looks_like_host(value)

    def looks_like_server_start(pos):
        return pos + 2 < len(items) and looks_like_host(items[pos]) and looks_like_user(items[pos + 1])

    pos = 0
    total = 0
    while pos < len(items):
        if not looks_like_server_start(pos):
            pos += 1
            continue
        total += 1
        pos += 3
        if pos < len(items) and not looks_like_server_start(pos) and not is_url(items[pos]):
            pos += 1
    return total

text = open(path, encoding="utf-8").read()
try:
    count = json_count(json.loads(text))
except json.JSONDecodeError:
    lines = text.splitlines()
    count = dotenv_count(lines)
    if count == 0:
        count = plain_count(lines)
print(count)
PY
  )"
fi

if [[ -z "$required_count" ]]; then
  required_count="$(
    python3 - "$inventory" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    print(len(json.load(fh).get("targets", [])))
PY
  )"
fi

[[ "$required_count" =~ ^[1-9][0-9]*$ ]] || die "XRAYC_REAL_TEST_SERVER_COUNT must be a positive integer"
if (( required_count < 3 )); then
  die "at least three test servers are required"
fi
[[ "$timeout_seconds" =~ ^[1-9][0-9]*$ ]] || die "XRAYC_REAL_TEST_SERVER_SSH_TIMEOUT_SECONDS must be a positive integer"
[[ "$connect_timeout_seconds" =~ ^[1-9][0-9]*$ ]] || die "XRAYC_REAL_TEST_SERVER_CONNECT_TIMEOUT_SECONDS must be a positive integer"

python3 - "$inventory" "$required_count" >"$tmp_file" <<'PY'
import json
import shlex
import sys

inventory, required_raw = sys.argv[1:3]
required = int(required_raw)
with open(inventory, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
if len(targets) < required:
    raise SystemExit(f"inventory requires {required} targets")
if len(targets) != required:
    raise SystemExit(f"inventory target count must equal {required}")
hosts = [str(item.get("ssh_host", "")).strip() for item in targets[:required]]
if any(not host for host in hosts):
    raise SystemExit("inventory target is missing ssh_host")
if len(set(hosts)) != required:
    raise SystemExit("first inventory targets must be distinct")

cloudflare_markers = ("cloudflare", "orange-cloud", "orange cloud", "橙云")

def alias_for(index, target):
    return str(target.get("alias") or target.get("name") or f"server_{index}").strip().lower()

for index, target in enumerate(targets[:required], 1):
    expected_alias = f"server_{index}"
    if alias_for(index, target) != expected_alias:
        raise SystemExit(f"target_{index} alias must be {expected_alias}")

def is_cloudflare_target(index, target):
    values = [
        target.get("ssh_host", ""),
        target.get("public_host", ""),
        target.get("public_domain", ""),
        target.get("domain", ""),
        target.get("tls_domain", ""),
        target.get("sni", ""),
        target.get("name", ""),
        target.get("role", ""),
        target.get("cdn_provider", ""),
        target.get("cdn", ""),
        target.get("provider", ""),
        target.get("remark", ""),
        target.get("remarks", ""),
        target.get("note", ""),
        target.get("notes", ""),
        target.get("description", ""),
    ]
    joined = " ".join(str(value).strip().lower() for value in values if value is not None)
    return any(marker in joined for marker in cloudflare_markers)

if is_cloudflare_target(1, targets[0]):
    raise SystemExit("server_1 must be direct/non-CF")
if required >= 4 and not is_cloudflare_target(4, targets[3]):
    raise SystemExit("server_4 must be marked Cloudflare/orange-cloud")

for index, target in enumerate(targets[:required], 1):
    values = {
        "index": str(index),
        "host": target.get("ssh_host", ""),
        "user": target.get("ssh_user", "root"),
        "port": str(target.get("ssh_port", 22)),
        "password_file": target.get("ssh_password_file", ""),
        "identity_file": target.get("ssh_identity_file", ""),
    }
    if not str(values["password_file"]).strip() and not str(values["identity_file"]).strip():
        raise SystemExit(f"target_{index} is missing SSH auth")
    print(" ".join(f"{key}={shlex.quote(str(value))}" for key, value in values.items()))
PY

while IFS= read -r line; do
  eval "$line"
  auth_kind=""
  if [[ -n "${password_file:-}" ]]; then
    [[ -f "$password_file" ]] || die "target_${index} password file is missing"
    command -v sshpass >/dev/null 2>&1 || die "sshpass is required for password-auth targets"
    auth_kind="password"
    ssh_cmd=(sshpass -f "$password_file" ssh)
  elif [[ -n "${identity_file:-}" ]]; then
    [[ -f "$identity_file" ]] || die "target_${index} identity file is missing"
    auth_kind="identity"
    ssh_cmd=(ssh -i "$identity_file")
  else
    die "target_${index} SSH auth is missing"
  fi
  if timeout "$timeout_seconds" "${ssh_cmd[@]}" \
    -n \
    -p "$port" \
    -o StrictHostKeyChecking=accept-new \
    -o UserKnownHostsFile=/root/.ssh/known_hosts \
    -o ConnectTimeout="$connect_timeout_seconds" \
    -o BatchMode=$([[ "$auth_kind" == "password" ]] && printf no || printf yes) \
    -o LogLevel=ERROR \
    "${user}@${host}" true >/dev/null 2>&1; then
    printf 'real-test-server-assets: target_%s=ok\n' "$index"
  else
    die "target_${index} ssh failed"
  fi
done <"$tmp_file"

printf 'real-test-server-assets: passed targets=%s\n' "$required_count"
