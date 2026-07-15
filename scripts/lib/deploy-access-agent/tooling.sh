#!/usr/bin/env bash
# 用途：提供 deploy-access-agent.sh 的依赖安装和系统准备函数。
# 该文件只定义 tooling 相关函数，由主部署脚本按需调用。

install_packages() {
  local packages=("$@")
  [[ "${#packages[@]}" -gt 0 ]] || return 0

  if command -v apt-get >/dev/null 2>&1; then
    run_root env DEBIAN_FRONTEND=noninteractive apt-get update
    run_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "${packages[@]}"
  elif command -v dnf >/dev/null 2>&1; then
    run_root dnf install -y "${packages[@]}"
  elif command -v yum >/dev/null 2>&1; then
    run_root yum install -y "${packages[@]}"
  elif command -v apk >/dev/null 2>&1; then
    run_root apk add --no-cache "${packages[@]}"
  else
    die "unsupported package manager; install missing tools manually"
  fi
}

install_compose() {
  if docker_run compose version >/dev/null 2>&1 || command -v docker-compose >/dev/null 2>&1; then
    return 0
  fi

  log "installing Docker Compose"
  if command -v apt-get >/dev/null 2>&1; then
    run_root env DEBIAN_FRONTEND=noninteractive apt-get update
    run_root env DEBIAN_FRONTEND=noninteractive apt-get install -y docker-compose-plugin \
      || run_root env DEBIAN_FRONTEND=noninteractive apt-get install -y docker-compose
  elif command -v dnf >/dev/null 2>&1; then
    run_root dnf install -y docker-compose-plugin || run_root dnf install -y docker-compose
  elif command -v yum >/dev/null 2>&1; then
    run_root yum install -y docker-compose-plugin || run_root yum install -y docker-compose
  elif command -v apk >/dev/null 2>&1; then
    run_root apk add --no-cache docker-cli-compose || run_root apk add --no-cache docker-compose
  else
    die "unsupported package manager; install Docker Compose manually"
  fi

  if docker_run compose version >/dev/null 2>&1 || command -v docker-compose >/dev/null 2>&1; then
    return 0
  fi

  local compose_version="${XRAYC_DOCKER_COMPOSE_VERSION:-v2.40.3}"
  local arch asset plugin_path tmp_compose
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) asset="docker-compose-linux-x86_64" ;;
    aarch64|arm64) asset="docker-compose-linux-aarch64" ;;
    *) die "unsupported architecture for Docker Compose static install" ;;
  esac
  plugin_path="/usr/local/lib/docker/cli-plugins/docker-compose"
  tmp_compose="${TMPDIR:-/tmp}/docker-compose-${compose_version}-${asset}.$$"
  log "installing Docker Compose static plugin"
  if ! run_root curl --fail --location --silent --show-error --retry 3 --connect-timeout 20 --max-time 180 \
    --output "$tmp_compose" \
    "https://github.com/docker/compose/releases/download/${compose_version}/${asset}"; then
    run_root rm -f "$tmp_compose" >/dev/null 2>&1 || true
    die "Docker Compose static plugin download failed; curl stderr redacted"
  fi
  run_root install -d -m 0755 "$(dirname "$plugin_path")"
  run_root install -m 0755 "$tmp_compose" "$plugin_path"
  run_root rm -f "$tmp_compose" >/dev/null 2>&1 || true
  for dir in /usr/local/libexec/docker/cli-plugins /usr/libexec/docker/cli-plugins; do
    if run_root install -d -m 0755 "$dir" >/dev/null 2>&1; then
      run_root cp "$plugin_path" "$dir/docker-compose" >/dev/null 2>&1 || true
    fi
  done
}

ensure_tooling() {
  if [[ "$(id -u)" -ne 0 ]] && ! sudo -n true >/dev/null 2>&1; then
    die "root or passwordless sudo is required; the script will not prompt for SSH or sudo passwords"
  fi

  local base_missing=()
  command -v curl >/dev/null 2>&1 || base_missing+=(curl)
  command -v sha256sum >/dev/null 2>&1 || base_missing+=(coreutils)
  if [[ "${#base_missing[@]}" -gt 0 ]]; then
    log "installing missing base tools"
    install_packages "${base_missing[@]}"
  fi

  if ! command -v docker >/dev/null 2>&1; then
    log "installing Docker from distribution packages"
    if command -v apt-get >/dev/null 2>&1; then
      install_packages ca-certificates curl docker.io
    elif command -v apk >/dev/null 2>&1; then
      install_packages docker
    else
      install_packages docker
    fi
  fi

  if command -v systemctl >/dev/null 2>&1; then
    run_root systemctl enable --now docker >/dev/null 2>&1 || true
  elif command -v service >/dev/null 2>&1; then
    run_root service docker start >/dev/null 2>&1 || true
  fi

  docker_run version >/dev/null 2>&1 || die "Docker is not available after installation"
  install_compose
  compose_run version >/dev/null 2>&1 || die "Docker Compose is not available after installation"
}

disable_legacy_systemd_units() {
  command -v systemctl >/dev/null 2>&1 || return 0

  for unit in xrayc-access-agent.service xrayc-xray.service; do
    if run_root systemctl list-unit-files "$unit" >/dev/null 2>&1; then
      log "disabling legacy ${unit}"
      run_root systemctl disable --now "$unit" >/dev/null 2>&1 || true
    fi
    run_root rm -f "/etc/systemd/system/${unit}"
  done
  run_root systemctl daemon-reload >/dev/null 2>&1 || true
}

ensure_ss_tooling() {
  command -v ss >/dev/null 2>&1 && return 0
  log "installing ss for listen port readiness checks"
  if command -v apt-get >/dev/null 2>&1; then
    install_packages iproute2
  elif command -v dnf >/dev/null 2>&1; then
    install_packages iproute
  elif command -v yum >/dev/null 2>&1; then
    install_packages iproute
  elif command -v apk >/dev/null 2>&1; then
    install_packages iproute2
  else
    die "ss command is required to check expected listen ports"
  fi
  command -v ss >/dev/null 2>&1 || die "ss command is still unavailable after installation"
}

valid_tls_domain() {
  local domain="$1"
  [[ "$domain" =~ ^[A-Za-z0-9.-]+$ ]] || return 1
  [[ "$domain" == *.* ]] || return 1
  [[ "$domain" != "localhost" ]] || return 1
  [[ ! "$domain" =~ ^[0-9.]+$ ]] || return 1
}

prompt_tls_certificate_settings() {
  [[ -t 0 ]] || return 0
  if [[ -z "$TLS_CERT_DOMAINS" ]]; then
    read -r -p "请输入需要自动申请 SSL 证书的域名，多个用空格分隔，留空跳过：" TLS_CERT_DOMAINS
    TLS_CERT_DOMAINS="${TLS_CERT_DOMAINS:-}"
  fi
  if [[ -n "$TLS_CERT_DOMAINS" && -z "$TLS_CERT_EMAIL" ]]; then
    read -r -p "请输入 ACME 邮箱，留空跳过 SSL 证书申请：" TLS_CERT_EMAIL
    TLS_CERT_EMAIL="${TLS_CERT_EMAIL:-}"
  fi
}

cleanup_tls_certificates_for_force_reinstall() {
  [[ "${OVERWRITE_RUNTIME_CONFIG:-false}" == "true" ]] || return 0

  local cleanup_domains="${TLS_CERT_DOMAINS:-} ${EXISTING_TLS_CERT_DOMAINS:-}"
  [[ -n "${cleanup_domains//[[:space:]]/}" ]] || return 0

  local domain
  local seen=" "
  for domain in $cleanup_domains; do
    valid_tls_domain "$domain" || die "XRAYC_TLS_CERT_DOMAINS contains an invalid domain"
    if [[ "$seen" == *" ${domain} "* ]]; then
      continue
    fi
    seen="${seen}${domain} "
    # 证书仍在有效期内就保留,不删不重签:避免 force_reinstall 无谓重烧 Let's Encrypt 限流,
    # 也避免重签失败时把节点弄成无证书(checkend 86400 秒 = 24 小时内不过期即视为有效)。
    local live_cert="/etc/letsencrypt/live/${domain}/fullchain.pem"
    if run_root test -f "$live_cert" \
      && run_root openssl x509 -checkend 86400 -noout -in "$live_cert" >/dev/null 2>&1; then
      log "keeping still-valid TLS certificate for configured domain"
      continue
    fi
    log "removing expired/invalid TLS certificate for configured domain"
    run_root rm -rf \
      "/etc/letsencrypt/live/${domain}" \
      "/etc/letsencrypt/archive/${domain}" \
      "/etc/letsencrypt/renewal/${domain}.conf" \
      >/dev/null 2>&1 || true
  done
}

# CF 橙云域名走 DNS-01:用 CF API Token 写 TXT 验证,签 cf_domain 自己的 LE 证书。
# 橙云 HTTP-01 会被 CF 拦,只能 DNS-01;token 仅写本机 0600 ini,不回显/不进日志。
# 仅 cf_cert_mode=dns01 且有 cf_domain + token 时执行;灰云直连域名仍走下方 HTTP-01。
install_cf_dns01_certificate() {
  [[ "${CF_CERT_MODE:-reuse_direct}" == "dns01" ]] || return 0
  [[ -n "${CF_DOMAIN:-}" && -n "${CLOUDFLARE_API_TOKEN:-}" ]] || {
    log "CF DNS-01 certificate skipped; cf_domain and Cloudflare API token are both required"
    return 0
  }
  [[ -n "${TLS_CERT_EMAIL:-}" ]] || {
    log "CF DNS-01 certificate skipped; ACME email is required"
    return 0
  }
  valid_tls_domain "$CF_DOMAIN" || die "XRAYC_CF_DOMAIN is not a valid domain"

  # 安装 certbot 与 dns-cloudflare 插件;不同发行版包名不同,装失败兜底用 pip。
  command -v certbot >/dev/null 2>&1 || install_packages ca-certificates openssl certbot
  if ! run_root certbot plugins 2>/dev/null | grep -q dns-cloudflare; then
    log "installing certbot dns-cloudflare plugin"
    install_packages python3-certbot-dns-cloudflare \
      || run_root sh -c 'command -v pip3 >/dev/null 2>&1 && pip3 install --break-system-packages certbot-dns-cloudflare' \
      || true
  fi

  run_root install -d -m 0755 /etc/letsencrypt
  # 写 0600 凭据 ini:先 install 占位再 tee 写入,全程不把 token 回显到终端/日志。
  local cf_ini="/etc/letsencrypt/cloudflare.ini"
  run_root install -m 0600 /dev/null "$cf_ini"
  printf 'dns_cloudflare_api_token = %s\n' "$CLOUDFLARE_API_TOKEN" \
    | run_root tee "$cf_ini" >/dev/null
  run_root chmod 0600 "$cf_ini"

  if run_root test -s "/etc/letsencrypt/live/${CF_DOMAIN}/fullchain.pem" \
    && run_root test -s "/etc/letsencrypt/live/${CF_DOMAIN}/privkey.pem" \
    && run_root openssl x509 -checkend 604800 -noout -in "/etc/letsencrypt/live/${CF_DOMAIN}/fullchain.pem" >/dev/null 2>&1; then
    log "CF DNS-01 certificate already exists for configured cf domain"
    return 0
  fi

  log "requesting CF DNS-01 certificate for configured cf domain"
  if ! run_root certbot certonly --dns-cloudflare \
    --dns-cloudflare-credentials "$cf_ini" \
    --non-interactive --agree-tos -m "$TLS_CERT_EMAIL" -d "$CF_DOMAIN" \
    >"${tmp_dir}/certbot-dns01-cf.out" 2>"${tmp_dir}/certbot-dns01-cf.err"; then
    die "CF DNS-01 certificate request failed; certbot stderr redacted"
  fi
}

install_tls_certificates() {
  prompt_tls_certificate_settings
  install_cf_dns01_certificate
  if [[ -z "$TLS_CERT_DOMAINS" || -z "$TLS_CERT_EMAIL" ]]; then
    log "TLS certificate request skipped; both domain and email are required"
    return 0
  fi
  local domains=()
  local domain
  for domain in $TLS_CERT_DOMAINS; do
    valid_tls_domain "$domain" || die "XRAYC_TLS_CERT_DOMAINS contains an invalid domain"
    domains+=("$domain")
  done
  [[ "${#domains[@]}" -gt 0 ]] || return 0

  command -v certbot >/dev/null 2>&1 || install_packages ca-certificates openssl certbot
  run_root install -d -m 0755 /etc/letsencrypt
  # HTTP-01 standalone 要独占 80/tcp(tls-alpn 回退还要 443/tcp);被别的容器/服务占着时
  # certbot 会以晦涩错误失败。装前先探测占用方并给出明确可执行提示(而非笼统"证书申请失败"),
  # 便于运维定位——只读探测,绝不主动停别人的服务。
  if command -v ss >/dev/null 2>&1; then
    local port80_holder
    port80_holder="$(run_root ss -lntp 2>/dev/null | awk 'NR>1 && $4 ~ /:80$/' | head -1)"
    if [[ -n "$port80_holder" ]]; then
      log "WARNING: 80/tcp 已被占用,HTTP-01 standalone 签证将失败;请先释放 80 口(停掉占端口的容器/服务)再重装。占用方: ${port80_holder}"
    fi
  fi
  for domain in "${domains[@]}"; do
    if run_root test -s "/etc/letsencrypt/live/${domain}/fullchain.pem" \
      && run_root test -s "/etc/letsencrypt/live/${domain}/privkey.pem" \
      && run_root openssl x509 -checkend 604800 -noout -in "/etc/letsencrypt/live/${domain}/fullchain.pem" >/dev/null 2>&1; then
      log "TLS certificate already exists for configured domain"
      continue
    fi
    local email_args=(--email "$TLS_CERT_EMAIL")
    log "requesting TLS certificate for configured domain"
    if run_root certbot certonly --standalone --non-interactive --agree-tos \
      "${email_args[@]}" -d "$domain" >"${tmp_dir}/certbot-${domain}.out" 2>"${tmp_dir}/certbot-${domain}.err"; then
      continue
    fi
    if run_root test -d /var/www/html \
      && run_root certbot certonly --webroot -w /var/www/html --non-interactive --agree-tos \
        "${email_args[@]}" -d "$domain" >"${tmp_dir}/certbot-webroot-${domain}.out" 2>"${tmp_dir}/certbot-webroot-${domain}.err"; then
      continue
    fi
    if ! run_root test -s "/etc/letsencrypt/live/${domain}/fullchain.pem" \
      || ! run_root test -s "/etc/letsencrypt/live/${domain}/privkey.pem"; then
      die "TLS certificate request failed (HTTP-01 standalone 需独占 80/tcp、tls-alpn 回退需 443/tcp;若这两个端口被其他容器/服务占用会失败——请释放后重装。certbot stderr redacted)"
    fi
  done
}
