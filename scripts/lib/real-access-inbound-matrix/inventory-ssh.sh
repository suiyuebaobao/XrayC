#!/usr/bin/env bash
# 这个 helper 负责真实入站矩阵 E2E 的 inventory 解析。
# 这个 helper 负责构建客户端与接入节点 SSH 命令。
# 这个 helper 只定义函数，由主脚本 source 后调用。
# 这个 helper 不直接打印主机、密码文件或私钥路径。
# 这个 helper 校验当前真实服务器清单，并只加载本次选择的 client/access 目标。
# 这个 helper 属于 real-access-inbound-matrix-e2e 的拆分实现。
# 这个 helper 不应被其他脚本直接执行。
# 这个 helper 依赖主脚本提供 TMP_DIR、INVENTORY 等变量。
# 这个 helper 依赖主脚本提供 die_usage 和 require_command。
# 这个 helper 修改时需保持 bash -n 通过。

reject_cloudflare_inventory_target() {
  local target_alias="$1"
  local target_role="$2"

  python3 - "$INVENTORY" "$target_alias" "$target_role" <<'PY'
import json
import sys

inventory, target_alias, target_role = sys.argv[1:4]
target_alias = target_alias.strip()
with open(inventory, "r", encoding="utf-8") as fh:
    payload = json.load(fh)

selected = None
cloudflare_targets = []
cloudflare_markers = ("cloudflare", "orange-cloud", "orange cloud", "橙云")
for target in payload.get("targets", []):
    values = [
        str(target.get("alias", "")).strip(),
        str(target.get("ssh_host", "")).strip(),
        str(target.get("public_host", "")).strip(),
        str(target.get("public_domain", "")).strip(),
        str(target.get("domain", "")).strip(),
        str(target.get("tls_domain", "")).strip(),
        str(target.get("sni", "")).strip(),
        str(target.get("name", "")).strip(),
    ]
    marker_values = values + [
        str(target.get("role", "")),
        str(target.get("note", "")),
        str(target.get("notes", "")),
        str(target.get("remark", "")),
        str(target.get("remarks", "")),
        str(target.get("cdn_provider", "")),
        str(target.get("cdn", "")),
        str(target.get("provider", "")),
    ]
    joined = " ".join(marker_values).lower()
    is_cloudflare = (
        values[0] == "server_4"
        or any(marker in joined for marker in cloudflare_markers)
    )
    if is_cloudflare:
        cloudflare_targets.append({value for value in values if value})
    if target_alias in values:
        selected = target
        selected_is_cloudflare = is_cloudflare
        break

target_lower = target_alias.lower()
if (
    target_alias == "server_4"
    or any(marker in target_lower for marker in cloudflare_markers)
    or any(target_alias in values for values in cloudflare_targets)
):
    raise SystemExit(
        f"{target_role} target is reserved for Cloudflare entry validation; "
        "ordinary inbound matrix must use a direct/non-CF target"
    )

if selected is None:
    raise SystemExit(0)

values = [
    str(selected.get("alias", "")),
    str(selected.get("role", "")),
    str(selected.get("note", "")),
    str(selected.get("notes", "")),
    str(selected.get("remark", "")),
    str(selected.get("remarks", "")),
    str(selected.get("cdn_provider", "")),
    str(selected.get("cdn", "")),
    str(selected.get("provider", "")),
]
joined = " ".join(values).lower()
is_cloudflare = (
    selected_is_cloudflare
    or target_alias == "server_4"
    or "cloudflare" in joined
    or "orange-cloud" in joined
    or "orange cloud" in joined
    or "橙云" in joined
)
if is_cloudflare:
    raise SystemExit(
        f"{target_role} target is reserved for Cloudflare entry validation; "
        "ordinary inbound matrix must use a direct/non-CF target"
    )
PY
}

load_inventory_client() {
  reject_cloudflare_inventory_target "$CLIENT_TARGET" "client"
  python3 - "$INVENTORY" "$CLIENT_TARGET" > "$TMP_DIR/client.env" <<'PY'
import json
import os
import shlex
import sys

inventory, client_target = sys.argv[1:3]
with open(inventory, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = payload.get("targets", [])
if len(targets) < 3:
    raise SystemExit("inventory must contain at least three targets")
hosts = []
selected = None
for index, target in enumerate(targets, 1):
    alias = str(target.get("alias", "")).strip()
    if alias != f"server_{index}":
        raise SystemExit("inventory aliases must be sequential server_N labels")
    host = str(target.get("ssh_host", "")).strip()
    user = str(target.get("ssh_user", "") or "root").strip()
    port = str(target.get("ssh_port", 22)).strip()
    password_file = str(target.get("ssh_password_file", "")).strip()
    identity_file = str(target.get("ssh_identity_file", "")).strip()
    if not host or not user or not port:
        raise SystemExit("inventory target is missing SSH fields")
    if not password_file and not identity_file:
        raise SystemExit("inventory target is missing SSH auth")
    for path in (password_file, identity_file):
        if path and not os.path.isfile(path):
            raise SystemExit("inventory references a missing SSH auth file")
    hosts.append(host)
    if alias == client_target:
        selected = {
            "SSH_HOST": host,
            "SSH_USER": user,
            "SSH_PORT": port,
            "SSH_PASSWORD_FILE": password_file,
            "SSH_IDENTITY_FILE": identity_file,
        }
if len(set(hosts)) != len(hosts):
    raise SystemExit("inventory targets must be distinct hosts")
if selected is None:
    raise SystemExit("client target was not found in inventory targets")
for key, value in selected.items():
    print(f"{key}={shlex.quote(str(value))}")
PY
  # shellcheck disable=SC1090
  . "$TMP_DIR/client.env"
}

load_inventory_access_target() {
  reject_cloudflare_inventory_target "$ACCESS_TARGET" "access"
  python3 - "$INVENTORY" "$ACCESS_TARGET" > "$TMP_DIR/access.env" <<'PY'
import json
import os
import shlex
import sys

inventory, access_target = sys.argv[1:3]
with open(inventory, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = payload.get("targets", [])
if len(targets) < 3:
    raise SystemExit("inventory must contain at least three targets")
selected = None
for index, target in enumerate(targets, 1):
    alias = str(target.get("alias", "")).strip()
    if alias != f"server_{index}":
        raise SystemExit("inventory aliases must be sequential server_N labels")
    if alias != access_target:
        continue
    selected = {
        "ACCESS_SSH_HOST": str(target.get("ssh_host", "")).strip(),
        "ACCESS_SSH_USER": str(target.get("ssh_user", "") or "root").strip(),
        "ACCESS_SSH_PORT": str(target.get("ssh_port", 22)).strip(),
        "ACCESS_SSH_PASSWORD_FILE": str(target.get("ssh_password_file", "")).strip(),
        "ACCESS_SSH_IDENTITY_FILE": str(target.get("ssh_identity_file", "")).strip(),
    }
if selected is None:
    raise SystemExit("access target was not found in inventory targets")
if not selected["ACCESS_SSH_HOST"] or not selected["ACCESS_SSH_USER"] or not selected["ACCESS_SSH_PORT"]:
    raise SystemExit("access target is missing SSH fields")
if not selected["ACCESS_SSH_PASSWORD_FILE"] and not selected["ACCESS_SSH_IDENTITY_FILE"]:
    raise SystemExit("access target is missing SSH auth")
for path in (selected["ACCESS_SSH_PASSWORD_FILE"], selected["ACCESS_SSH_IDENTITY_FILE"]):
    if path and not os.path.isfile(path):
        raise SystemExit("inventory references a missing SSH auth file")
for key, value in selected.items():
    print(f"{key}={shlex.quote(str(value))}")
PY
  # shellcheck disable=SC1090
  . "$TMP_DIR/access.env"
}

build_ssh_command() {
  SSH_CMD=(ssh -p "$SSH_PORT" -o BatchMode=no -o LogLevel=ERROR -o StrictHostKeyChecking=accept-new)
  if [[ -n "${SSH_IDENTITY_FILE:-}" ]]; then
    SSH_CMD+=(-i "$SSH_IDENTITY_FILE")
  elif [[ -n "${SSH_PASSWORD_FILE:-}" ]]; then
    require_command sshpass
    SSH_CMD=(sshpass -f "$SSH_PASSWORD_FILE" "${SSH_CMD[@]}")
  fi
  SSH_DEST="${SSH_USER}@${SSH_HOST}"
}

build_access_ssh_command() {
  ACCESS_SSH_CMD=(ssh -p "$ACCESS_SSH_PORT" -o BatchMode=no -o LogLevel=ERROR -o StrictHostKeyChecking=accept-new)
  if [[ -n "${ACCESS_SSH_IDENTITY_FILE:-}" ]]; then
    ACCESS_SSH_CMD+=(-i "$ACCESS_SSH_IDENTITY_FILE")
  elif [[ -n "${ACCESS_SSH_PASSWORD_FILE:-}" ]]; then
    require_command sshpass
    ACCESS_SSH_CMD=(sshpass -f "$ACCESS_SSH_PASSWORD_FILE" "${ACCESS_SSH_CMD[@]}")
  fi
  ACCESS_SSH_DEST="${ACCESS_SSH_USER}@${ACCESS_SSH_HOST}"
}

remote_exec() {
  "${SSH_CMD[@]}" "$SSH_DEST" "$@"
}

remote_bash_file() {
  local script_file="$1"
  "${SSH_CMD[@]}" "$SSH_DEST" bash -s < "$script_file"
}

access_remote_bash_file() {
  local script_file="$1"
  "${ACCESS_SSH_CMD[@]}" "$ACCESS_SSH_DEST" bash -s < "$script_file"
}
