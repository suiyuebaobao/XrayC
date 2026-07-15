#!/usr/bin/env bash
# 用途：提供真实协议矩阵资产准备脚本的通用 shell 函数。
# 范围：仅供 scripts/prepare-real-protocol-matrix-assets.sh source 使用。
# 安全：终端输出只包含 server_N 标签和粗粒度状态。
# 说明：这里不执行主流程，只封装校验、SSH 构造和远端探测逻辑。

record_gap() {
  printf '%s\n' "$1" >> "$GAP_FILE"
}

record_warning() {
  printf '%s\n' "$1" >> "$WARN_FILE"
}

print_status() {
  printf '%-28s %-12s %s\n' "$1" "$2" "$3"
}

require_local_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    record_gap "local ${command_name} command is missing"
  fi
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

exit_if_gaps() {
  local include_warnings="${1:-0}"
  if [[ -s "$GAP_FILE" ]]; then
    echo "real protocol matrix assets: blocked"
    echo "gap summary:"
    sed 's/.*/  - &/' "$GAP_FILE"
    if [[ "$include_warnings" == "1" && -s "$WARN_FILE" ]]; then
      echo "warnings:"
      sed 's/.*/  - &/' "$WARN_FILE"
    fi
    exit 2
  fi
}

validate_account_targets() {
  local idx alias_value host_value user_value port_value
  local password_file_value identity_file_value had_inline_password_value

  if [[ ! "${TARGET_COUNT:-0}" =~ ^[0-9]+$ || "${TARGET_COUNT:-0}" -lt 3 ]]; then
    record_gap "at least three test servers are required"
    return
  fi

  for idx in $(seq 1 "$TARGET_COUNT"); do
    eval "alias_value=\$ALIAS${idx}" \
      "host_value=\$SSH_HOST${idx}" \
      "user_value=\$SSH_USER${idx}" \
      "port_value=\$SSH_PORT${idx}" \
      "password_file_value=\$SSH_PASSWORD_FILE${idx}" \
      "identity_file_value=\$SSH_IDENTITY_FILE${idx}" \
      "had_inline_password_value=\$HAD_INLINE_PASSWORD${idx}"

    [[ "$alias_value" == "server_${idx}" ]] || record_gap "server_${idx} alias normalization failed"
    [[ -n "$host_value" ]] || record_gap "server_${idx} ssh host is missing"
    [[ -n "$user_value" ]] || record_gap "server_${idx} ssh user is missing"
    [[ "$port_value" =~ ^[0-9]+$ ]] || record_gap "server_${idx} ssh port is invalid"
    if [[ -z "$password_file_value" && -z "$identity_file_value" ]]; then
      record_gap "server_${idx} ssh auth file is missing"
    fi
    if [[ -n "$password_file_value" && ! -f "$password_file_value" ]]; then
      record_gap "server_${idx} ssh password file is missing"
    fi
    if [[ -n "$identity_file_value" && ! -f "$identity_file_value" ]]; then
      record_gap "server_${idx} ssh identity file is missing"
    fi
    if [[ -n "$password_file_value" ]] && ! command -v sshpass >/dev/null 2>&1; then
      record_gap "local sshpass command is missing for password-file auth"
    fi
    if [[ "$had_inline_password_value" == "1" ]]; then
      record_warning "server_${idx} used an inline private password; a private 0600 password file was generated"
    fi
  done
}

build_ssh_command() {
  local index="$1"
  local -n out_ref="$2"
  local host user port password_file identity_file
  eval "host=\$SSH_HOST${index}" \
    "user=\$SSH_USER${index}" \
    "port=\$SSH_PORT${index}" \
    "password_file=\$SSH_PASSWORD_FILE${index}" \
    "identity_file=\$SSH_IDENTITY_FILE${index}"

  out_ref=(
    ssh
    -p "$port"
    -o ConnectTimeout=15
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

remote_probe() {
  local index="$1"
  local script="$2"
  local output_file="${TMP_DIR}/remote-${index}.out"
  local err_file="${TMP_DIR}/remote-${index}.err"
  local ssh_cmd=()
  build_ssh_command "$index" ssh_cmd
  if "${ssh_cmd[@]}" "$script" >"$output_file" 2>"$err_file"; then
    head -n 1 "$output_file" | tr -cd '[:alnum:]_.:-'
    return 0
  fi
  return 1
}

role_for_index() {
  case "$1" in
    1) printf 'client' ;;
    2) printf 'relay' ;;
    3) printf 'exit-a' ;;
    4) printf 'exit-b' ;;
    *) printf 'extra' ;;
  esac
}

probe_remote_capabilities() {
  local idx role label docker_status compose_status

  echo "real protocol matrix asset probe"
  print_status "server" "role" "capability"

  for idx in $(seq 1 "$TARGET_COUNT"); do
    role="$(role_for_index "$idx")"
    label="server_${idx}"

    if remote_probe "$idx" "true"; then
      print_status "$label" "$role" "ssh=ok"
    else
      print_status "$label" "$role" "ssh=failed"
      record_gap "${label} ssh check failed; stderr redacted"
      continue
    fi

    if [[ "$CLEANUP_UPSTREAM_LAB" == "1" ]]; then
      continue
    fi

    docker_status="$(remote_probe "$idx" "if command -v docker >/dev/null 2>&1; then if docker version >/dev/null 2>&1 || sudo -n docker version >/dev/null 2>&1; then printf docker_ok; else printf docker_daemon_unavailable; fi; else printf docker_missing; fi" || printf failed)"
    print_status "$label" "$role" "docker=${docker_status}"
    if [[ "$docker_status" != "docker_ok" ]]; then
      record_gap "${label} Docker capability is not ready"
    fi

    compose_status="$(remote_probe "$idx" "if command -v docker >/dev/null 2>&1 && (docker compose version >/dev/null 2>&1 || sudo -n docker compose version >/dev/null 2>&1); then printf compose_v2_ok; elif command -v docker-compose >/dev/null 2>&1 || sudo -n sh -c 'command -v docker-compose >/dev/null 2>&1'; then printf compose_v1_ok; else printf compose_missing; fi" || printf failed)"
    print_status "$label" "$role" "compose=${compose_status}"
    if [[ "$idx" == "2" && "$compose_status" != "compose_v2_ok" && "$compose_status" != "compose_v1_ok" ]]; then
      record_gap "server_2 relay Docker Compose capability is not ready"
    elif [[ "$compose_status" != "compose_v2_ok" && "$compose_status" != "compose_v1_ok" ]]; then
      record_warning "${label} Docker Compose capability is missing"
    fi
  done
}
