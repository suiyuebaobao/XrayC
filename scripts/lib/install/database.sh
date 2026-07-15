#!/usr/bin/env bash
# ④ 仅数据库角色:postgres(镜像走自家 registry 免墙),公网暴露 5432,密码本地生成。
# 配合 ② 仅前端 / ③ 仅后端 用:装完打印 DATABASE_URL 模板给后端。
# 依赖 common.sh。本头部满足仓库前十行中文注释约束。

install_database() {
  require_docker; port_guard 5432
  local pw; pw="$(gen_secret 16)"
  write_env /opt/xrayc-db/.env "POSTGRES_PASSWORD=${pw}" "XRAYC_REGISTRY=${XRAYC_REGISTRY:?需 --registry}"
  cp "$(dirname "${BASH_SOURCE[0]}")/../../../deploy/install/database.compose.yml" /opt/xrayc-db/docker-compose.yml
  (cd /opt/xrayc-db && docker compose --env-file .env pull && docker compose --env-file .env up -d)
  log "数据库就绪。后端用:DATABASE_URL=postgres://xrayc:${pw}@<本机公网IP>:5432/xrayc"
  log "==== 请记录此 DB 密码:${pw}(仅打印一次)===="
}
