#!/usr/bin/env bash
# 用途：提供真实协议上游实验脚本的通用 shell 函数。
# 包含本地命令校验、私有 inventory 解析、SSH 命令构造和远端上传。
# 该文件仅由 scripts/real-protocol-upstream-lab.sh source，不直接执行。

die() {
  echo "real protocol upstream lab: $*" >&2
  exit 1
}

status() {
  printf 'real protocol upstream lab: %s\n' "$1"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "local ${1} command is missing"
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

load_inventory_targets() {
  python3 - "$INVENTORY" "$TARGETS_ENV" <<'PY'
import json
import shlex
import sys

inventory, output = sys.argv[1:3]
with open(inventory, "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])

if len(targets) < 3:
    raise SystemExit("inventory must contain at least server_1..server_3")

def is_cloudflare_target(target):
    alias = str(target.get("alias", "")).strip().lower()
    fields = [
        target.get("cdn_provider", ""),
        target.get("role", ""),
        target.get("roles", ""),
        target.get("tags", ""),
        target.get("remark", ""),
        target.get("note", ""),
        target.get("notes", ""),
        target.get("public_domain", ""),
    ]
    joined = " ".join(str(value) for value in fields).lower()
    return (
        alias == "server_4"
        or "cloudflare" in joined
        or "orange-cloud" in joined
        or "orange cloud" in joined
        or "橙云" in joined
    )

hosts = [str(item.get("ssh_host", "")).strip() for item in targets]
if len(set(hosts)) != len(hosts):
    raise SystemExit("server_N targets must be distinct hosts")
if is_cloudflare_target(targets[0]):
    raise SystemExit("server_1 must be direct/non-CF")

def has_public_domain(target):
    return bool(str(target.get("public_domain", "")).strip())

tls_exit_source = 0
for candidate_index in (4, 2, 3, 1):
    if candidate_index <= len(targets):
        candidate = targets[candidate_index - 1]
        if not is_cloudflare_target(candidate) and has_public_domain(candidate):
            tls_exit_source = candidate_index
            break
if tls_exit_source == 0:
    raise SystemExit("a direct/non-CF public domain target is required for TLS protocol lab")

with open(output, "w", encoding="utf-8") as fh:
    role_sources = {
        "BASIC_EXIT": 3,
        "TLS_EXIT": tls_exit_source,
    }
    for role, source_index in role_sources.items():
        target = targets[source_index - 1]
        values = {
            f"{role}_REMOTE_INDEX": str(source_index),
            f"{role}_ALIAS": target.get("alias", f"server_{source_index}"),
            f"{role}_HOST": target.get("ssh_host", ""),
            f"{role}_DOMAIN": target.get("public_domain", ""),
            f"{role}_USER": target.get("ssh_user", "root"),
            f"{role}_PORT": str(target.get("ssh_port", "22")),
            f"{role}_PASSWORD_FILE": target.get("ssh_password_file", ""),
            f"{role}_IDENTITY_FILE": target.get("ssh_identity_file", ""),
        }
        if values[f"{role}_ALIAS"] != f"server_{source_index}":
            raise SystemExit("inventory aliases must be sequential server_N labels")
        if not values[f"{role}_HOST"] or not values[f"{role}_USER"] or not values[f"{role}_PORT"]:
            raise SystemExit("exit target SSH fields are incomplete")
        if not values[f"{role}_PASSWORD_FILE"] and not values[f"{role}_IDENTITY_FILE"]:
            raise SystemExit("exit target SSH auth file is missing")
        for key in (f"{role}_PASSWORD_FILE", f"{role}_IDENTITY_FILE"):
            value = values[key]
            if value and "\n" in str(value):
                raise SystemExit("invalid SSH auth file")
        for key, value in values.items():
            fh.write(f"{key}={shlex.quote(str(value))}\n")
PY

  # shellcheck disable=SC1090
  . "$TARGETS_ENV"
}

validate_ssh_auth_files() {
  local role label password_file identity_file
  for role in BASIC_EXIT TLS_EXIT; do
    label="$(printf '%s' "$role" | tr '[:upper:]' '[:lower:]' | tr '_' '-')"
    eval "password_file=\${${role}_PASSWORD_FILE:-}" "identity_file=\${${role}_IDENTITY_FILE:-}"
    if [[ -n "$password_file" && ! -f "$password_file" ]]; then
      die "${label} SSH password file is missing"
    fi
    if [[ -n "$identity_file" && ! -f "$identity_file" ]]; then
      die "${label} SSH identity file is missing"
    fi
    if [[ -n "$password_file" ]]; then
      require_command sshpass
    fi
  done
}

build_ssh_command() {
  local role="$1"
  local -n out_ref="$2"
  local host user port password_file identity_file
  eval "host=\$${role}_HOST" \
    "user=\$${role}_USER" \
    "port=\$${role}_PORT" \
    "password_file=\${${role}_PASSWORD_FILE:-}" \
    "identity_file=\${${role}_IDENTITY_FILE:-}"

  out_ref=(
    ssh
    -p "$port"
    -o ConnectTimeout=20
    -o ServerAliveInterval=15
    -o StrictHostKeyChecking=accept-new
  )
  if [[ -n "$identity_file" ]]; then
    out_ref+=(-o BatchMode=yes -i "$identity_file")
  fi
  if [[ -n "$password_file" ]]; then
    out_ref+=(-o BatchMode=no)
    out_ref=(sshpass -f "$password_file" "${out_ref[@]}")
  else
    out_ref+=(-o BatchMode=yes)
  fi
  out_ref+=("${user}@${host}")
}

remote_run() {
  local role="$1"
  shift
  local ssh_cmd=()
  local stdout_file="${TMP_DIR}/remote-${role}.out"
  local stderr_file="${TMP_DIR}/remote-${role}.err"
  build_ssh_command "$role" ssh_cmd
  if "${ssh_cmd[@]}" "$@" >"$stdout_file" 2>"$stderr_file"; then
    cat "$stdout_file"
    return 0
  fi
  return 1
}

remote_bash() {
  local role="$1"
  local ssh_cmd=()
  local stdout_file="${TMP_DIR}/remote-${role}.out"
  local stderr_file="${TMP_DIR}/remote-${role}.err"
  build_ssh_command "$role" ssh_cmd
  if "${ssh_cmd[@]}" bash -s >"$stdout_file" 2>"$stderr_file"; then
    cat "$stdout_file"
    return 0
  fi
  return 1
}

write_remote_assignment() {
  local name="$1"
  local value="$2"
  printf '%s=%s\n' "$name" "$(shell_quote "$value")"
}

remote_upload_payload() {
  local role="$1"
  local payload_dir="$2"
  local remote_tmp="$3"
  local ssh_cmd=()
  local stdout_file="${TMP_DIR}/upload-${role}.out"
  local stderr_file="${TMP_DIR}/upload-${role}.err"
  build_ssh_command "$role" ssh_cmd
  if tar -C "$payload_dir" -czf - . | "${ssh_cmd[@]}" "umask 077; rm -rf $(shell_quote "$remote_tmp"); mkdir -p $(shell_quote "$remote_tmp"); tar -xzf - -C $(shell_quote "$remote_tmp")" >"$stdout_file" 2>"$stderr_file"; then
    return 0
  fi
  return 1
}
