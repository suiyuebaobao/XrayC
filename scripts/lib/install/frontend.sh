#!/usr/bin/env bash
# ② 仅前端角色:caddy(含前端 dist)反代到远程后端公网地址。
# 浏览器只连面板域名,Caddy 把 /api·/sub·/health 同源反代到后端,JWT+HttpOnly Cookie 穿透拆分。
# 从模板生成 Caddyfile(上游 = 后端公网 IP:端口),挂载覆盖镜像内置。依赖 common.sh。
# 本头部满足仓库前十行中文注释约束。

install_frontend() {
  require_docker; port_guard 80; port_guard 443
  local panel="${PANEL_DOMAIN:?需 --domain}" upstream="${API_UPSTREAM:?需 --api-upstream(后端IP:端口)}"
  install -d -m 755 /opt/xrayc-frontend
  sed -e "s|__PANEL__|${panel}|g" -e "s|__UPSTREAM__|${upstream}|g" \
    "$(dirname "${BASH_SOURCE[0]}")/../../../deploy/install/frontend.Caddyfile.tpl" \
    > /opt/xrayc-frontend/Caddyfile
  docker rm -f xrayc-frontend >/dev/null 2>&1 || true
  docker run -d --name xrayc-frontend --restart always -p 80:80 -p 443:443 \
    -v /opt/xrayc-frontend/Caddyfile:/etc/caddy/Caddyfile:ro -v xrayc_caddy_data:/data \
    "${XRAYC_REGISTRY:?需 --registry}/xrayc/caddy:trial"
  wait_http_200 "https://${panel}/health" 30 \
    || warn "面板 /health 暂未通,检查后端 ${upstream} 是否可达与证书签发"
  log "前端就绪: https://${panel}(反代 /api 到 ${upstream})"
}
