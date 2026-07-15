#!/usr/bin/env bash
# XrayC 一键部署入口:菜单或 --role 选 6 角色,在目标机本地执行。
# 角色:aio 一体机 / frontend 仅前端 / backend 仅后端 / database 仅数据库 / registry 镜像仓库。
# (node 节点不在本脚本范围:节点仍走管理面板的一键 SSH 安装 deploy-access-agent。)
# 镜像统一从自建 registry 拉;装完自带账号 admin(随机密码、打印一次)。
# 薄入口:只做参数装配、菜单、分发;具体实现在 scripts/lib/install/<role>.sh。
# 本头部满足仓库前十行中文注释约束。
set -euo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/install/common.sh
source "${HERE}/scripts/lib/install/common.sh"
for r in registry database backend frontend aio; do
  # shellcheck source=/dev/null
  source "${HERE}/scripts/lib/install/${r}.sh"
done

ROLE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --role) ROLE="$2"; shift 2;;
    --domain) export PANEL_DOMAIN="$2"; shift 2;;
    --registry) export XRAYC_REGISTRY="$2"; shift 2;;
    --registry-domain) export REGISTRY_DOMAIN="$2"; shift 2;;
    --database-url) export DATABASE_URL="$2"; shift 2;;
    --api-upstream) export API_UPSTREAM="$2"; shift 2;;
    --backend-port) export BACKEND_PORT="$2"; shift 2;;
    --public-base-url) export XRAYC_PUBLIC_BASE_URL="$2"; shift 2;;
    *) die "未知参数 $1";;
  esac
done

if [ -z "$ROLE" ]; then
  echo "选择部署角色:"
  select r in "aio 一体机" "frontend 仅前端" "backend 仅后端" "database 仅数据库" "registry 镜像仓库"; do
    [ -n "${r:-}" ] && { ROLE="${r%% *}"; break; }
  done
fi

case "$ROLE" in
  aio)      install_aio;;
  frontend) install_frontend;;
  backend)  install_backend;;
  database) install_database;;
  registry) install_registry;;
  *) die "未知角色 ${ROLE}(aio|frontend|backend|database|registry)";;
esac
