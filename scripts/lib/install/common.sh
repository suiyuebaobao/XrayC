#!/usr/bin/env bash
# 一键部署公共库:日志、docker 自检、端口/服务冲突闸、密钥生成、.env 写入、健康检查。
# 被 install.sh 与各角色 helper source;不直接执行。
# 端口冲突闸是安全红线:占用者非本项目(陌生服务)一律中止,绝不自动停他人服务。
# 密钥一律本地生成、写 0600 .env、不进 git/日志。
# 本头部满足仓库前十行中文注释约束。
set -euo pipefail

log()  { printf '\033[36m[xrayc]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[xrayc][warn]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31m[xrayc][err]\033[0m %s\n' "$*" >&2; exit 1; }

require_docker() {
  command -v docker >/dev/null 2>&1 || die "未装 docker,请先安装(curl -fsSL https://get.docker.com | sh)"
  docker compose version >/dev/null 2>&1 || die "未装 docker compose v2"
}

# 端口冲突闸:占用者是本项目(xrayc-*)→可走升级;陌生服务→中止,绝不动别人服务。
port_guard() {
  local port="$1"
  local line; line="$(ss -tlnp 2>/dev/null | awk -v p=":${port} " '$4 ~ p {print; exit}')" || true
  [ -z "$line" ] && return 0
  if printf '%s' "$line" | grep -q 'xrayc'; then
    warn "端口 ${port} 被本项目占用,将走升级/覆盖流程"; return 0
  fi
  die "端口 ${port} 被非本项目服务占用,已中止(绝不自动停他人服务)"
}

gen_secret() { openssl rand -hex "${1:-24}"; }

# 写 0600 .env(键值对,目录权限收紧)
write_env() {
  local path="$1"; shift
  install -d -m 700 "$(dirname "$path")"
  umask 077; : > "$path"
  local kv
  for kv in "$@"; do printf '%s\n' "$kv" >> "$path"; done
  chmod 600 "$path"
}

# 轮询某 URL 直到 200(或超次数返回非 0)
wait_http_200() {
  local url="$1" tries="${2:-30}"
  local _
  for _ in $(seq 1 "$tries"); do
    [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$url")" = "200" ] && return 0
    sleep 3
  done
  return 1
}
