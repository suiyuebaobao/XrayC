#!/usr/bin/env bash
# ⑥ 镜像仓库角色:registry:2 + Caddy HTTPS(公开拉、无鉴权)。
# 落地真机已验证命令:DNS 守卫(等域名解析到本机)→写 Caddyfile→compose up→验 /v2/。
# registry 必须灰云直连(非 CF 代理),否则 CF 100MB 请求上限会卡大镜像层 push。
# 依赖 common.sh 的 require_docker/port_guard/wait_http_200/log/die。
# 本头部满足仓库前十行中文注释约束。

install_registry() {
  require_docker; port_guard 80; port_guard 443
  local domain="${REGISTRY_DOMAIN:?需 --registry-domain}"
  local self; self="$(curl -fsS https://api.ipify.org)"
  log "等 ${domain} 解析到本机 ${self}(ACME 前提)"
  local _
  for _ in $(seq 1 18); do
    [ "$(dig +short "$domain" @8.8.8.8 | tail -n1)" = "$self" ] && break
    sleep 10
  done
  install -d -m 755 /opt/xrayc-registry/data
  printf '%s {\n    reverse_proxy registry:5000\n}\n' "$domain" > /opt/xrayc-registry/Caddyfile
  cp "$(dirname "${BASH_SOURCE[0]}")/../../../deploy/install/registry.compose.yml" /opt/xrayc-registry/docker-compose.yml
  (cd /opt/xrayc-registry && docker compose up -d)
  wait_http_200 "https://${domain}/v2/" 30 || die "registry HTTPS 未就绪"
  log "registry 上线: https://${domain}/v2/(公开拉、无鉴权)"
}
