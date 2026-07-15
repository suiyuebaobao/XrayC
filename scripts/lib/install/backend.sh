#!/usr/bin/env bash
# ③ 仅后端角色:api+worker(rust-app,镜像走自家 registry),连远程库,暴露端口,自带 admin。
# worker 先起跑迁移,api 后起;production 钉死安全姿态;启动按 env 幂等建账号 admin。
# 装完打印 admin 随机密码一次(仅此一次)。依赖 common.sh。
# 本头部满足仓库前十行中文注释约束。

install_backend() {
  require_docker; port_guard "${BACKEND_PORT:-8000}"
  local jwt admin_pw
  jwt="$(gen_secret 32)"; admin_pw="$(gen_secret 12)"
  write_env /opt/xrayc-backend/.env \
    "XRAYC_REGISTRY=${XRAYC_REGISTRY:?需 --registry}" \
    "DATABASE_URL=${DATABASE_URL:?需 --database-url}" \
    "JWT_SECRET=${jwt}" \
    "XRAYC_PUBLIC_BASE_URL=${XRAYC_PUBLIC_BASE_URL:?需 --public-base-url}" \
    "BACKEND_PORT=${BACKEND_PORT:-8000}" \
    "XRAYC_BOOTSTRAP_ADMIN_PASSWORD=${admin_pw}"
  cp "$(dirname "${BASH_SOURCE[0]}")/../../../deploy/install/backend.compose.yml" /opt/xrayc-backend/docker-compose.yml
  (cd /opt/xrayc-backend && docker compose --env-file .env pull && docker compose --env-file .env up -d)
  wait_http_200 "http://127.0.0.1:${BACKEND_PORT:-8000}/health" 40 || die "后端 /health 未就绪"
  log "后端就绪(暴露端口 ${BACKEND_PORT:-8000})。"
  log "==== 管理员账号 admin / 密码 ${admin_pw}(仅打印这一次,请保存)===="
}
