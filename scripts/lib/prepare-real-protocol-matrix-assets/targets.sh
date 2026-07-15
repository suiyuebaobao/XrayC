#!/usr/bin/env bash
# 用途：解析私有服务器账号文件并生成真实协议矩阵 inventory。
# 范围：仅供 scripts/prepare-real-protocol-matrix-assets.sh source 使用。
# 安全：解析结果写入 0600 临时文件或私有输出文件，不打印敏感字段。
# 说明：这里封装原脚本内嵌 Python，shell 层只暴露两个 helper 函数。

prepare_targets_from_account_file() {
  local account_file="$1"
  local targets_json="$2"
  local targets_env="$3"
  local tmp_dir="$4"
  local private_dir="$5"
  local run_id="$6"

  python3 - "$account_file" "$targets_json" "$targets_env" "$tmp_dir" "$private_dir" "$run_id" <<'PY'
import json
import os
import shlex
import sys

account_file, targets_json, targets_env, tmp_dir, private_dir, run_id = sys.argv[1:7]

def first_shell_token(value):
    value = str(value or "").strip()
    if not value:
        return ""
    try:
        parsed = shlex.split(value, comments=False, posix=True)
    except ValueError:
        return value.strip("'\"").split()[0] if value.strip("'\"").split() else ""
    return parsed[0] if parsed else ""

def parse_dotenv(path):
    values = {}
    with open(path, "r", encoding="utf-8") as fh:
        for raw in fh:
            line = raw.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            key = key.strip()
            value = value.strip()
            if key.startswith("export "):
                key = key[7:].strip()
            try:
                parsed = shlex.split(value, comments=False, posix=True)
                value = parsed[0] if parsed else ""
            except ValueError:
                value = value.strip("'\"")
            values[key] = value
    return values

def first_nonempty(*values):
    for value in values:
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""

def normalize_item(alias, item):
    item = item or {}
    return {
        "alias": alias,
        "ssh_host": first_shell_token(first_nonempty(item.get("ssh_host"), item.get("host"), item.get("ip"), item.get("server"))),
        "ssh_user": first_nonempty(item.get("ssh_user"), item.get("user"), item.get("username"), "root"),
        "ssh_port": first_nonempty(item.get("ssh_port"), item.get("port"), "22"),
        "ssh_password_file": first_nonempty(item.get("ssh_password_file"), item.get("password_file"), item.get("passfile")),
        "ssh_identity_file": first_nonempty(item.get("ssh_identity_file"), item.get("identity_file"), item.get("key_file")),
        "ssh_password": first_shell_token(first_nonempty(item.get("ssh_password"), item.get("password"), item.get("pass"))),
        "public_domain": first_shell_token(first_nonempty(item.get("public_domain"), item.get("public_host"), item.get("domain"), item.get("tls_domain"), item.get("sni"))),
        "tls_cert_email": first_nonempty(item.get("tls_cert_email"), item.get("acme_email"), item.get("email")),
        "control_plane_url": first_shell_token(first_nonempty(item.get("control_plane_url"), item.get("base_url"))),
    }

def load_json(path):
    with open(path, "r", encoding="utf-8") as fh:
        payload = json.load(fh)
    source = payload.get("servers")
    if isinstance(source, dict):
        out = []
        idx = 1
        while f"server_{idx}" in source:
            out.append(normalize_item(f"server_{idx}", source.get(f"server_{idx}", {})))
            idx += 1
        return out
    targets = payload.get("targets", [])
    if not isinstance(targets, list):
        targets = []
    by_alias = {str(item.get("alias", "")): item for item in targets if isinstance(item, dict)}
    if "server_1" in by_alias:
        out = []
        idx = 1
        while f"server_{idx}" in by_alias:
            out.append(normalize_item(f"server_{idx}", by_alias[f"server_{idx}"]))
            idx += 1
        return out
    return [
        normalize_item(f"server_{idx}", item)
        for idx, item in enumerate(targets, 1)
        if isinstance(item, dict)
    ]

def load_dotenv(path):
    env = parse_dotenv(path)
    out = []
    idx = 1
    while True:
        prefix = f"SERVER_{idx}_"
        lower_prefix = f"server_{idx}_"
        def get(*names):
            candidates = []
            for name in names:
                candidates.extend([prefix + name, lower_prefix + name.lower()])
            return first_nonempty(*(env.get(name) for name in candidates))
        host = first_shell_token(get("SSH_HOST", "HOST", "IP"))
        if not host:
            break
        out.append({
            "alias": f"server_{idx}",
            "ssh_host": host,
            "ssh_user": first_nonempty(get("SSH_USER", "USER", "USERNAME"), "root"),
            "ssh_port": first_nonempty(get("SSH_PORT", "PORT"), "22"),
            "ssh_password_file": get("SSH_PASSWORD_FILE", "PASSWORD_FILE", "PASSFILE"),
            "ssh_identity_file": get("SSH_IDENTITY_FILE", "IDENTITY_FILE", "KEY_FILE"),
            "ssh_password": get("SSH_PASSWORD", "PASSWORD", "PASS"),
            "public_domain": first_shell_token(get("PUBLIC_DOMAIN", "PUBLIC_HOST", "DOMAIN", "TLS_DOMAIN", "SNI")),
            "tls_cert_email": get("TLS_CERT_EMAIL", "ACME_EMAIL", "EMAIL"),
            "control_plane_url": first_shell_token(get("CONTROL_PLANE_URL", "BASE_URL")),
        })
        idx += 1
    return out

def load_plain_triples(path):
    def is_separator(line):
        return len(line) == 1

    def is_url(line):
        return "://" in line

    def looks_like_host(line):
        if is_url(line):
            return False
        if any(ch.isspace() for ch in line):
            return False
        return "." in line or ":" in line

    def looks_like_user(line):
        return bool(line.strip()) and not is_url(line) and not looks_like_host(line)

    def looks_like_server_start(items, pos):
        if pos + 2 >= len(items):
            return False
        return looks_like_host(items[pos]) and looks_like_user(items[pos + 1])

    def split_host_port(value):
        value = str(value or "").strip()
        if value.count(":") == 1:
            host, port = value.rsplit(":", 1)
            if host and port.isdigit():
                parsed = int(port)
                if 1 <= parsed <= 65535:
                    return host, str(parsed)
        return value, "22"

    with open(path, "r", encoding="utf-8") as fh:
        lines = []
        for raw in fh:
            line = raw.strip()
            if not line or line.startswith("#") or is_separator(line):
                continue
            lines.append(line)
    out = []
    pos = 0
    while pos < len(lines):
        if not looks_like_server_start(lines, pos):
            pos += 1
            continue
        chunk = lines[pos:pos + 3]
        pos += 3
        public_domain = ""
        if pos < len(lines) and not looks_like_server_start(lines, pos) and not is_url(lines[pos]):
            public_domain = first_shell_token(lines[pos])
            pos += 1
        idx = len(out) + 1
        ssh_host, ssh_port = split_host_port(chunk[0])
        out.append({
            "alias": f"server_{idx}",
            "ssh_host": ssh_host,
            "ssh_user": chunk[1] if len(chunk) > 1 else "root",
            "ssh_port": ssh_port,
            "ssh_password_file": "",
            "ssh_identity_file": "",
            "ssh_password": first_shell_token(chunk[2]) if len(chunk) > 2 else "",
            "public_domain": public_domain,
            "tls_cert_email": "",
            "control_plane_url": "",
        })
    return out

try:
    targets = load_json(account_file)
except json.JSONDecodeError:
    targets = load_dotenv(account_file)
    if not any(target.get("ssh_host") for target in targets):
        targets = load_plain_triples(account_file)

for target in targets:
    password = target.pop("ssh_password", "")
    if password:
        password_dir = private_dir if not target.get("ssh_password_file") else tmp_dir
        password_path = os.path.join(password_dir, f"real-protocol-matrix.{run_id}.{target['alias']}.ssh-password")
        with open(password_path, "w", encoding="utf-8") as fh:
            fh.write(password)
        os.chmod(password_path, 0o600)
        if not target.get("ssh_password_file"):
            target["ssh_password_file"] = password_path
        target["had_inline_password"] = True
    else:
        target["had_inline_password"] = False

with open(targets_json, "w", encoding="utf-8") as fh:
    json.dump({"targets": targets}, fh, ensure_ascii=False, indent=2)
    fh.write("\n")
os.chmod(targets_json, 0o600)

with open(targets_env, "w", encoding="utf-8") as fh:
    fh.write(f"TARGET_COUNT={len(targets)}\n")
    for idx, target in enumerate(targets, 1):
        for key in ("alias", "ssh_host", "ssh_user", "ssh_port", "ssh_password_file", "ssh_identity_file", "public_domain", "tls_cert_email", "control_plane_url"):
            value = str(target.get(key, ""))
            fh.write(f"{key.upper()}{idx}={shlex.quote(value)}\n")
        fh.write(f"HAD_INLINE_PASSWORD{idx}={1 if target.get('had_inline_password') else 0}\n")
os.chmod(targets_env, 0o600)
PY
}

write_inventory_json() {
  local source="$1"
  local output="$2"

  python3 - "$source" "$output" <<'PY'
import json
import shlex
import sys

def first_shell_token(value):
    value = str(value or "").strip()
    if not value:
        return ""
    try:
        parsed = shlex.split(value, comments=False, posix=True)
    except ValueError:
        parsed = value.strip("'\"").split()
    return parsed[0] if parsed else ""

source, output = sys.argv[1:3]
with open(source, "r", encoding="utf-8") as fh:
    payload = json.load(fh)
targets = []
for item in payload["targets"]:
    clean = {
        "alias": item["alias"],
        "ssh_host": item["ssh_host"],
        "ssh_user": item["ssh_user"],
        "ssh_port": int(item["ssh_port"]),
    }
    for key in ("ssh_password_file", "ssh_identity_file", "public_domain", "tls_cert_email", "control_plane_url"):
        if item.get(key):
            clean[key] = first_shell_token(item[key]) if key in ("public_domain", "control_plane_url") else item[key]
    if clean.get("public_domain"):
        clean["tls_cert_domains"] = [clean["public_domain"]]
    targets.append(clean)
with open(output, "w", encoding="utf-8") as fh:
    json.dump({"targets": targets}, fh, ensure_ascii=False, indent=2)
    fh.write("\n")
PY
  chmod 600 "$output"
}
