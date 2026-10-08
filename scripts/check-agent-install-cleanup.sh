#!/usr/bin/env bash
# 用途：静态检查 Agent 服务器侧安装 E2E 与安装脚本的清理和回滚保护。
# 范围：只扫描本仓库脚本文本，不连接目标服务器或执行 Docker 清理。
# 输入：读取真实发布门禁、服务器侧安装 E2E 和 access-agent 安装脚本。
# 输出：发现缺失保护或禁止命令时逐项报告，全部通过时输出 passed。
# 依赖：依赖 grep 正则匹配关键保护变量和危险操作模式。
# 安全：阻止脚本出现杀 SSH 进程、停 SSH 服务或误清理本地 compose。
# 约束：新增目标服务器清理路径时必须显式区分本次创建和既有资源。
# 行为：所有检查累积失败数，最后统一决定退出码。
# 失败：任何保护缺失或危险模式出现都会让检查返回非零。
# 维护：清理策略变更时同步更新 require_pattern 和 forbidden_pattern。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

failures=0

fail() {
  printf 'agent-install-cleanup-check: %s\n' "$1" >&2
  failures=$((failures + 1))
}

require_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if ! grep -Eq "$pattern" "$file"; then
    fail "${label} missing in ${file}"
  fi
}

forbidden_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if grep -En "$pattern" "$file" >/dev/null; then
    fail "${label} found in ${file}"
  fi
}

require_pattern scripts/check-real-release.sh 'XRAYC_REMOTE_E2E_CLEANUP_ON_FAILURE=1' \
  'real release gate default failure cleanup coverage'

require_pattern scripts/real-remote-access-deploy-e2e.sh 'DEPLOY_ATTEMPTED=0' \
  'server-side cleanup deployment-attempt guard'
require_pattern scripts/real-remote-access-deploy-e2e.sh 'INSTALL_DIR_PREEXISTING=' \
  'server-side cleanup install-dir preexistence guard'
require_pattern scripts/real-remote-access-deploy-e2e.sh 'XRAYC_REMOTE_E2E_CLEANUP_EXISTING_INSTALL_DIR' \
  'explicit override for cleaning preexisting server install dirs'
require_pattern scripts/real-remote-access-deploy-e2e.sh 'INSTALL_DIR_PREEXISTING:-1.*== "0".*cleanup_existing' \
  'server-side cleanup default only removes E2E-created install dirs'

require_pattern scripts/deploy-access-agent.sh 'DEPLOY_CREATED_INSTALL_DIR=0' \
  'deploy rollback created-install-dir guard'
require_pattern scripts/deploy-access-agent.sh 'DEPLOY_COMPOSE_UP_ATTEMPTED=0' \
  'deploy rollback compose-attempt guard'
require_pattern scripts/deploy-access-agent.sh 'DEPLOY_CONTAINER_NAMES_TOUCHED=0' \
  'deploy rollback container-touch guard'
require_pattern scripts/lib/deploy-access-agent/deploy.sh 'restore_preserved_installation' \
  'deploy rollback restores the retained installation'
require_pattern scripts/deploy-access-agent.sh 'cleanup_all "\$status"' \
  'deploy trap passes original exit status into cleanup'
require_pattern scripts/lib/deploy-access-agent/deploy.sh 'local status="\$\{1:-\$\?\}"' \
  'deploy cleanup preserves caller-provided exit status'
require_pattern scripts/deploy-access-agent.sh 'cleanup_tls_certificates_for_force_reinstall' \
  'deploy force reinstall SSL certificate cleanup hook'
require_pattern scripts/lib/deploy-access-agent/tooling.sh 'cleanup_tls_certificates_for_force_reinstall\(\)' \
  'deploy force reinstall SSL certificate cleanup function'
require_pattern scripts/lib/deploy-access-agent/tooling.sh '/etc/letsencrypt/live/\$\{domain\}' \
  'deploy force reinstall removes configured live certificate directory'
require_pattern scripts/lib/deploy-access-agent/tooling.sh '/etc/letsencrypt/archive/\$\{domain\}' \
  'deploy force reinstall removes configured archive certificate directory'
require_pattern scripts/lib/deploy-access-agent/tooling.sh '/etc/letsencrypt/renewal/\$\{domain\}\.conf' \
  'deploy force reinstall removes configured renewal certificate config'

for script in scripts/deploy-access-agent.sh scripts/real-remote-access-deploy-e2e.sh; do
  forbidden_pattern "$script" '\b(pkill|killall)\b[^#\n]*\bssh(d)?\b' \
    'SSH process-kill command'
  forbidden_pattern "$script" '\bsystemctl\b[^#\n]*\b(restart|stop|disable[[:space:]]+--now)\b[^#\n]*\bssh(d)?(\.service)?\b' \
    'SSH service stop/restart command'
done

if grep -En '\b(docker compose|docker-compose|compose_run)\b[^#\n]*\bdown\b' scripts/real-remote-access-deploy-e2e.sh \
  | grep -Ev 'ssh_remote|require_pattern' >/dev/null; then
  fail 'local Docker Compose down found in server-side install E2E script'
fi

if [[ "$failures" -gt 0 ]]; then
  printf 'agent-install-cleanup-check: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi

echo "agent-install-cleanup-check: passed"
