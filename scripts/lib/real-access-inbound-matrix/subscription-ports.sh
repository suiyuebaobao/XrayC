#!/usr/bin/env bash
# 这个 helper 负责真实入站矩阵 E2E 的临时端口放行、监听等待和清理。
# 这个 helper 只定义函数，由主脚本 source 后调用。
# 这个 helper 依赖 subscription-prepare 提供 infer_access_target_from_template。
# 这个 helper 依赖 inventory-ssh 提供 access target SSH helpers。
# 这个 helper 修改时需保持 bash -n 通过。

prepared_access_ports() {
  [[ -f "$TEMP_PREP_MANIFEST" ]] || return 0
  awk -F= '/^PORT_[A-Z_]+=/ {print $2}' "$TEMP_PREP_MANIFEST" \
    | awk '/^[0-9]+$/ && $1 > 0 && $1 < 65536 {print $1}' \
    | sort -n -u
}

prepared_tcp_access_ports() {
  [[ -f "$TEMP_PREP_MANIFEST" ]] || return 0
  awk -F= '
    /^PORT_[A-Z_]+=/ {
      name = $1
      port = $2
      if (name ~ /(HY2|HYSTERIA)$/) next
      if (port ~ /^[0-9]+$/ && port > 0 && port < 65536) print port
    }
  ' "$TEMP_PREP_MANIFEST" | sort -n -u
}

prepared_udp_access_ports() {
  [[ -f "$TEMP_PREP_MANIFEST" ]] || return 0
  awk -F= '
    /^PORT_[A-Z_]+=/ {
      name = $1
      port = $2
      if (name !~ /(HY2|HYSTERIA)$/) next
      if (port ~ /^[0-9]+$/ && port > 0 && port < 65536) print port
    }
  ' "$TEMP_PREP_MANIFEST" | sort -n -u
}

prepared_firewall_comment() {
  local run_id

  [[ -f "$TEMP_PREP_MANIFEST" ]] || return 1
  run_id="$(awk -F= '$1 == "RUN_ID" {print $2; exit}' "$TEMP_PREP_MANIFEST" | tr -cd 'A-Za-z0-9_.-')"
  [[ -n "$run_id" ]] || return 1
  printf 'xrayc-inbound-matrix-%s' "$run_id"
}

open_prepared_access_ports() {
  local ports tcp_ports udp_ports script_file inferred_access_target firewall_comment
  ports="$(prepared_access_ports | xargs || true)"
  [[ -n "$ports" ]] || return 0
  tcp_ports="$(prepared_tcp_access_ports | xargs || true)"
  udp_ports="$(prepared_udp_access_ports | xargs || true)"
  firewall_comment="$(prepared_firewall_comment)" || die "temporary inbound manifest is missing run id"
  inferred_access_target="$(infer_access_target_from_template || true)"
  if [[ -n "${REAL_ACCESS_INBOUND_MATRIX_ACCESS_TARGET:-}" ]]; then
    if [[ -n "$inferred_access_target" && "$inferred_access_target" != "$ACCESS_TARGET" ]]; then
      die "temporary inbound access target does not match prepared access node"
    fi
  else
    if [[ -n "$inferred_access_target" ]]; then
      ACCESS_TARGET="$inferred_access_target"
    else
      die "temporary inbound access target could not be inferred"
    fi
  fi
  load_inventory_access_target
  build_access_ssh_command
  script_file="$TMP_DIR/open-access-ports.sh"
  TCP_PORTS_VALUE="$tcp_ports" UDP_PORTS_VALUE="$udp_ports" COMMENT_VALUE="$firewall_comment" python3 - "$script_file" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"TCP_PORTS={shlex.quote(os.environ['TCP_PORTS_VALUE'])}\n")
    fh.write(f"UDP_PORTS={shlex.quote(os.environ['UDP_PORTS_VALUE'])}\n")
    fh.write(f"FIREWALL_COMMENT={shlex.quote(os.environ['COMMENT_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
for port in $TCP_PORTS; do
  case "$port" in
    ''|*[!0-9]*) exit 2 ;;
  esac
  if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    exit 2
  fi
  if command -v iptables >/dev/null 2>&1; then
    if ! iptables -C INPUT -p tcp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1; then
      iptables -I INPUT -p tcp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1 || true
    fi
  fi
done
for port in $UDP_PORTS; do
  case "$port" in
    ''|*[!0-9]*) exit 2 ;;
  esac
  if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    exit 2
  fi
  if command -v iptables >/dev/null 2>&1; then
    if ! iptables -C INPUT -p udp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1; then
      iptables -I INPUT -p udp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1 || true
    fi
  fi
done
''')
PY
  access_remote_bash_file "$script_file" || die "temporary inbound firewall preparation failed; details redacted"
  TEMP_OPENED_PORTS="$tcp_ports"
  TEMP_OPENED_UDP_PORTS="$udp_ports"
  status "temporary_inbound_ports_allowed"
}

wait_prepared_access_ports_listening() {
  local ports script_file timeout_seconds poll_seconds

  ports="$(prepared_tcp_access_ports | xargs || true)"
  [[ -n "$ports" ]] || return 0
  declare -p ACCESS_SSH_CMD >/dev/null 2>&1 || die "temporary inbound access target ssh is not initialized"
  [[ -n "${ACCESS_SSH_DEST:-}" ]] || die "temporary inbound access target ssh destination is missing"
  timeout_seconds="${REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_TIMEOUT_SECONDS:-60}"
  poll_seconds="${REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_POLL_SECONDS:-2}"
  [[ "$timeout_seconds" =~ ^[0-9]+$ && "$timeout_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_TIMEOUT_SECONDS must be a positive integer"
  [[ "$poll_seconds" =~ ^[0-9]+$ && "$poll_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_POLL_SECONDS must be a positive integer"

  script_file="$TMP_DIR/wait-access-ports-listening.sh"
  PORTS_VALUE="$ports" TIMEOUT_VALUE="$timeout_seconds" POLL_VALUE="$poll_seconds" \
    python3 - "$script_file" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"PORTS={shlex.quote(os.environ['PORTS_VALUE'])}\n")
    fh.write(f"TIMEOUT_SECONDS={shlex.quote(os.environ['TIMEOUT_VALUE'])}\n")
    fh.write(f"POLL_SECONDS={shlex.quote(os.environ['POLL_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
deadline="$(($(date +%s) + TIMEOUT_SECONDS))"
while [ "$(date +%s)" -le "$deadline" ]; do
  listeners="$(ss -ltn 2>/dev/null || netstat -ltn 2>/dev/null || true)"
  missing=0
  for port in $PORTS; do
    case "$port" in
      ''|*[!0-9]*) exit 2 ;;
    esac
    printf '%s\n' "$listeners" | awk -v port=":${port}" '$0 ~ port {found=1} END {exit found ? 0 : 1}' \
      || missing=1
  done
  if [ "$missing" -eq 0 ]; then
    exit 0
  fi
  sleep "$POLL_SECONDS"
done
exit 1
''')
PY
  access_remote_bash_file "$script_file" >/dev/null 2>&1 \
    || die "temporary inbound prepared TCP ports did not become reachable before traffic check"
  status "temporary_inbound_ports_listening"
}

wait_prepared_access_udp_ports_listening() {
  local ports script_file timeout_seconds poll_seconds

  ports="$(prepared_udp_access_ports | xargs || true)"
  [[ -n "$ports" ]] || return 0
  declare -p ACCESS_SSH_CMD >/dev/null 2>&1 || die "temporary inbound access target ssh is not initialized"
  [[ -n "${ACCESS_SSH_DEST:-}" ]] || die "temporary inbound access target ssh destination is missing"
  timeout_seconds="${REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_TIMEOUT_SECONDS:-60}"
  poll_seconds="${REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_POLL_SECONDS:-2}"
  [[ "$timeout_seconds" =~ ^[0-9]+$ && "$timeout_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_TIMEOUT_SECONDS must be a positive integer"
  [[ "$poll_seconds" =~ ^[0-9]+$ && "$poll_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_PORT_LISTEN_POLL_SECONDS must be a positive integer"

  script_file="$TMP_DIR/wait-access-udp-ports-listening.sh"
  PORTS_VALUE="$ports" TIMEOUT_VALUE="$timeout_seconds" POLL_VALUE="$poll_seconds" \
    python3 - "$script_file" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"PORTS={shlex.quote(os.environ['PORTS_VALUE'])}\n")
    fh.write(f"TIMEOUT_SECONDS={shlex.quote(os.environ['TIMEOUT_VALUE'])}\n")
    fh.write(f"POLL_SECONDS={shlex.quote(os.environ['POLL_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
deadline="$(($(date +%s) + TIMEOUT_SECONDS))"
while [ "$(date +%s)" -le "$deadline" ]; do
  listeners="$(ss -lun 2>/dev/null || netstat -lun 2>/dev/null || true)"
  missing=0
  for port in $PORTS; do
    case "$port" in
      ''|*[!0-9]*) exit 2 ;;
    esac
    printf '%s\n' "$listeners" | awk -v port=":${port}" '$0 ~ port {found=1} END {exit found ? 0 : 1}' \
      || missing=1
  done
  if [ "$missing" -eq 0 ]; then
    exit 0
  fi
  sleep "$POLL_SECONDS"
done
exit 1
''')
PY
  access_remote_bash_file "$script_file" >/dev/null 2>&1 \
    || die "temporary inbound prepared UDP ports did not become reachable before traffic check"
  status "temporary_inbound_udp_ports_listening"
}

cleanup_prepared_access_ports() {
  local script_file firewall_comment
  [[ -n "${TEMP_OPENED_PORTS:-}" || -n "${TEMP_OPENED_UDP_PORTS:-}" ]] || return 0
  declare -p ACCESS_SSH_CMD >/dev/null 2>&1 || return 0
  [[ -n "${ACCESS_SSH_DEST:-}" ]] || return 0
  firewall_comment="$(prepared_firewall_comment)" || return 0
  script_file="$TMP_DIR/cleanup-access-ports.sh"
  TCP_PORTS_VALUE="${TEMP_OPENED_PORTS:-}" UDP_PORTS_VALUE="${TEMP_OPENED_UDP_PORTS:-}" COMMENT_VALUE="$firewall_comment" python3 - "$script_file" <<'PY'
import os
import shlex
import sys

with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f"TCP_PORTS={shlex.quote(os.environ['TCP_PORTS_VALUE'])}\n")
    fh.write(f"UDP_PORTS={shlex.quote(os.environ['UDP_PORTS_VALUE'])}\n")
    fh.write(f"FIREWALL_COMMENT={shlex.quote(os.environ['COMMENT_VALUE'])}\n")
    fh.write(r'''
set -euo pipefail
set +x
if command -v iptables >/dev/null 2>&1; then
  for port in $TCP_PORTS; do
    case "$port" in
      ''|*[!0-9]*) continue ;;
    esac
    while iptables -C INPUT -p tcp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1; do
      iptables -D INPUT -p tcp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1 || break
    done
  done
  for port in $UDP_PORTS; do
    case "$port" in
      ''|*[!0-9]*) continue ;;
    esac
    while iptables -C INPUT -p udp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1; do
      iptables -D INPUT -p udp --dport "$port" -m comment --comment "$FIREWALL_COMMENT" -j ACCEPT >/dev/null 2>&1 || break
    done
  done
fi
''')
PY
  access_remote_bash_file "$script_file" >/dev/null 2>&1 || true
}
