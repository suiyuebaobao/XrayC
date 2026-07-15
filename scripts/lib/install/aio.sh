#!/usr/bin/env bash
# ① 一体机角色:postgres+api+worker+caddy 同机(caddy 默认上游 api:3000)。
# 本地生成 DB 密码/JWT 密钥/admin 随机密码;production 钉死安全姿态;装完打印 admin 一次。
# 依赖 common.sh。本头部满足仓库前十行中文注释约束。

install_aio() {
  require_docker; port_guard 80; port_guard 443
  local panel="${PANEL_DOMAIN:?需 --domain}"
  local dbpw jwt admin_pw
  dbpw="$(gen_secret 16)"; jwt="$(gen_secret 32)"; admin_pw="$(gen_secret 12)"
  write_env /opt/xrayc-aio/.env \
    "XRAYC_REGISTRY=${XRAYC_REGISTRY:?需 --registry}" \
    "POSTGRES_PASSWORD=${dbpw}" \
    "JWT_SECRET=${jwt}" \
    "PANEL_DOMAIN=${panel}" \
    "XRAYC_PUBLIC_BASE_URL=https://${panel}" \
    "XRAYC_BOOTSTRAP_ADMIN_PASSWORD=${admin_pw}"
  cp "$(dirname "${BASH_SOURCE[0]}")/../../../deploy/install/aio.compose.yml" /opt/xrayc-aio/docker-compose.yml
  (cd /opt/xrayc-aio && docker compose --env-file .env pull && docker compose --env-file .env up -d)
  wait_http_200 "https://${panel}/health" 40 || warn "面板 /health 暂未通,检查证书签发"
  log "一体机就绪: https://${panel}"
  log "==== 管理员账号 admin / 密码 ${admin_pw}(仅打印这一次,请保存)===="
}
