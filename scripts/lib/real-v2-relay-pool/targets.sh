#!/usr/bin/env bash

write_relay_pool_targets_env() {
  python3 - "$INVENTORY" "${XRAYC_REAL_TEST_SERVER_COUNT:-}" > "$TMP_DIR/targets.env" <<'PY'
import json
import os
import shlex
import sys

def first_host_token(value):
    value = str(value or "").strip()
    if not value:
        return ""
    try:
        parsed = shlex.split(value, comments=False, posix=True)
    except ValueError:
        parsed = value.strip("'\"").split()
    return parsed[0] if parsed else ""

def public_host_for(target):
    return (
        first_host_token(target.get("public_domain", ""))
        or first_host_token(target.get("public_host", ""))
        or first_host_token(target.get("ssh_host", ""))
    )

def direct_host_for(target):
    return first_host_token(target.get("ssh_host", "")) or public_host_for(target)

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

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    targets = json.load(fh).get("targets", [])
requested_raw = sys.argv[2].strip()
requested = int(requested_raw) if requested_raw else len(targets)
if requested < 3:
    raise SystemExit("inventory must contain at least three targets")
if len(targets) != requested:
    raise SystemExit("inventory target count must match requested target count")
hosts = [str(target.get("ssh_host", "")).strip() for target in targets[:requested]]
if len(set(hosts)) != requested:
    raise SystemExit("inventory targets must be distinct hosts")

print(f"TARGET_COUNT={requested}")
for index in range(1, requested + 1):
    target = targets[index - 1]
    public_host = public_host_for(target)
    values = {
        f"A{index}": target.get("alias", ""),
        f"H{index}": target.get("ssh_host", ""),
        f"PH{index}": public_host,
        f"U{index}": target.get("ssh_user", "root"),
        f"P{index}": str(target.get("ssh_port", 22)),
        f"PF{index}": target.get("ssh_password_file", ""),
    }
    if index == 1:
        values["BASE_URL"] = target.get("control_plane_url", "") or os.environ.get("BASE_URL", "")
    for key, value in values.items():
        if not str(value).strip():
            raise SystemExit(f"inventory target missing {key}")
        print(f"{key}={shlex.quote(str(value))}")
exit_b_source_index = 3
if requested >= 4 and not is_cloudflare_target(targets[3]):
    exit_b_source_index = 4
target = targets[exit_b_source_index - 1]
values = {
    "EXIT_A_REMOTE_INDEX": "3",
    "EXIT_A_PUBLIC_HOST": direct_host_for(targets[2]),
    "EXIT_B_REMOTE_INDEX": str(exit_b_source_index),
    "EXIT_B_ALIAS": target.get("alias", ""),
    "EXIT_B_PUBLIC_HOST": direct_host_for(target),
    "EXIT_B_UDP_TARGET_HOST": direct_host_for(target),
}
for key, value in values.items():
    if not str(value).strip():
        raise SystemExit(f"inventory target missing {key}")
    print(f"{key}={shlex.quote(str(value))}")
PY
}
