#!/usr/bin/env bash
# 用途：执行真实协议上游实验的远端部署、清理和公网出口探测。
# 远端脚本片段会隐藏 stderr，避免泄露主机、凭据或代理连接信息。
# 该文件仅由 scripts/real-protocol-upstream-lab.sh source，不直接执行。

deploy_remote() {
  local index="$1"
  local server_role="$2"
  local payload_dir="${TMP_DIR}/payload-${server_role}"
  local remote_tmp="/tmp/xrayc-real-protocol-upstream-lab-${RUN_ID}-${server_role}"
  local remote_dir="${REMOTE_ROOT}/${server_role}"
  local project_name="${PROJECT_PREFIX}-${server_role}"
  local trojan_cert_domain=""

  if [[ "$server_role" == "tls-exit" ]]; then
    trojan_cert_domain="$(python3 - "$ENV_VALUES_JSON" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    print(json.load(fh).get("TROJAN_SERVER_NAME", ""))
PY
)"
    [[ -n "$ACME_EMAIL" ]] || die "ACME email is required for TLS protocol lab certificates"
  fi

  write_payload "$server_role" "$payload_dir"
  remote_upload_payload "$index" "$payload_dir" "$remote_tmp" || die "${server_role} upload failed; stderr redacted"

  {
    write_remote_assignment REMOTE_TMP "$remote_tmp"
    write_remote_assignment REMOTE_DIR "$remote_dir"
    write_remote_assignment PROJECT_NAME "$project_name"
    write_remote_assignment TROJAN_CERT_DOMAIN "$trojan_cert_domain"
    write_remote_assignment ACME_EMAIL "$ACME_EMAIL"
    cat <<'REMOTE'
set -euo pipefail

install_cert_tools() {
  if command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -y >/dev/null
    apt-get install -y ca-certificates openssl certbot >/dev/null
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y ca-certificates openssl certbot >/dev/null
  elif command -v yum >/dev/null 2>&1; then
    yum install -y ca-certificates openssl certbot >/dev/null
  else
    exit 43
  fi
}

valid_domain_name() {
  case "$1" in
    ""|*"/"*|*":"*|*" "*|*".."*|.*|*.) return 1 ;;
    *) return 0 ;;
  esac
}

ensure_trojan_certificate() {
  [ -n "$TROJAN_CERT_DOMAIN" ] || return 0
  [ -n "$ACME_EMAIL" ] || exit 45
  valid_domain_name "$TROJAN_CERT_DOMAIN" || exit 42
  cert_path="/etc/letsencrypt/live/$TROJAN_CERT_DOMAIN/fullchain.pem"
  key_path="/etc/letsencrypt/live/$TROJAN_CERT_DOMAIN/privkey.pem"
  if command -v openssl >/dev/null 2>&1 \
    && [ -s "$cert_path" ] \
    && [ -s "$key_path" ] \
    && openssl x509 -checkend 604800 -noout -in "$cert_path" >/dev/null 2>&1; then
    return 0
  fi
  if ! command -v certbot >/dev/null 2>&1 || ! command -v openssl >/dev/null 2>&1; then
    install_cert_tools
  fi
  if [ -s "$cert_path" ] \
    && [ -s "$key_path" ] \
    && openssl x509 -checkend 604800 -noout -in "$cert_path" >/dev/null 2>&1; then
    return 0
  fi
  if certbot certonly --standalone --non-interactive --agree-tos \
    --email "$ACME_EMAIL" \
    -d "$TROJAN_CERT_DOMAIN" >/tmp/xrayc-certbot.out 2>/tmp/xrayc-certbot.err; then
    return 0
  fi
  if [ -d /var/www/html ] && certbot certonly --webroot -w /var/www/html \
    --non-interactive --agree-tos --email "$ACME_EMAIL" \
    -d "$TROJAN_CERT_DOMAIN" >/tmp/xrayc-certbot-webroot.out 2>/tmp/xrayc-certbot-webroot.err; then
    return 0
  fi
  exit 44
}

choose_docker() {
  if docker version >/dev/null 2>&1; then
    printf 'docker'
  elif sudo -n docker version >/dev/null 2>&1; then
    printf 'sudo -n docker'
  else
    exit 10
  fi
}

compose_down_if_owned() {
  local docker_cmd="$1"
  local compose_cmd="$2"
  if [ -d "$REMOTE_DIR" ]; then
    if [ ! -f "$REMOTE_DIR/.xrayc-real-protocol-upstream-lab" ]; then
      exit 41
    fi
    if [ -f "$REMOTE_DIR/docker-compose.yml" ]; then
      sh -c "$compose_cmd -f \"\$1\" -p \"\$2\" down --remove-orphans >/dev/null 2>&1 || true" sh "$REMOTE_DIR/docker-compose.yml" "$PROJECT_NAME"
    fi
    sh -c "$docker_cmd rm -f \$(\$docker_cmd ps -aq --filter label=com.docker.compose.project=\"$PROJECT_NAME\") >/dev/null 2>&1 || true"
    rm -rf "$REMOTE_DIR"
  fi
}

docker_cmd="$(choose_docker)"
if sh -c "$docker_cmd compose version >/dev/null 2>&1"; then
  compose_cmd="$docker_cmd compose"
elif command -v docker-compose >/dev/null 2>&1; then
  compose_cmd="docker-compose"
elif sudo -n sh -c 'command -v docker-compose >/dev/null 2>&1'; then
  compose_cmd="sudo -n docker-compose"
else
  exit 11
fi

ensure_trojan_certificate
compose_down_if_owned "$docker_cmd" "$compose_cmd"
mkdir -p "$REMOTE_DIR"
cp -R "$REMOTE_TMP/." "$REMOTE_DIR/"
rm -rf "$REMOTE_TMP"
sh -c "$compose_cmd -f \"\$1\" -p \"\$2\" config >/dev/null" sh "$REMOTE_DIR/docker-compose.yml" "$PROJECT_NAME"
sh -c "$compose_cmd -f \"\$1\" -p \"\$2\" up -d --remove-orphans >/dev/null" sh "$REMOTE_DIR/docker-compose.yml" "$PROJECT_NAME"
sh -c "$docker_cmd ps --filter label=com.docker.compose.project=\"$PROJECT_NAME\" --format '{{.Names}}' | grep -q ."
REMOTE
  } | remote_bash "$index" >/dev/null || die "${server_role} deploy failed; stderr redacted"

  status "${server_role} deployed"
}

cleanup_remote() {
  local index="$1"
  local server_role="$2"
  local remote_dir="${REMOTE_ROOT}/${server_role}"
  local project_name="${PROJECT_PREFIX}-${server_role}"

  {
    write_remote_assignment REMOTE_DIR "$remote_dir"
    write_remote_assignment PROJECT_NAME "$project_name"
    cat <<'REMOTE'
set -euo pipefail
if [ ! -d "$REMOTE_DIR" ]; then
  exit 0
fi
if [ ! -f "$REMOTE_DIR/.xrayc-real-protocol-upstream-lab" ]; then
  exit 41
fi
if docker version >/dev/null 2>&1; then
  docker_cmd="docker"
elif sudo -n docker version >/dev/null 2>&1; then
  docker_cmd="sudo -n docker"
else
  rm -rf "$REMOTE_DIR"
  exit 0
fi
if sh -c "$docker_cmd compose version >/dev/null 2>&1"; then
  compose_cmd="$docker_cmd compose"
elif command -v docker-compose >/dev/null 2>&1; then
  compose_cmd="docker-compose"
elif sudo -n sh -c 'command -v docker-compose >/dev/null 2>&1'; then
  compose_cmd="sudo -n docker-compose"
else
  compose_cmd=""
fi
if [ -n "$compose_cmd" ] && [ -f "$REMOTE_DIR/docker-compose.yml" ]; then
  sh -c "$compose_cmd -f \"\$1\" -p \"\$2\" down --remove-orphans >/dev/null 2>&1 || true" sh "$REMOTE_DIR/docker-compose.yml" "$PROJECT_NAME"
fi
sh -c "$docker_cmd rm -f \$(\$docker_cmd ps -aq --filter label=com.docker.compose.project=\"$PROJECT_NAME\") >/dev/null 2>&1 || true"
rm -rf "$REMOTE_DIR"
REMOTE
  } | remote_bash "$index" >/dev/null || die "${server_role} cleanup failed; stderr redacted"

  status "${server_role} cleaned"
}

remote_public_ip() {
  local index="$1"
  local value=""
  value="$(
    {
      write_remote_assignment PUBLIC_IP_URL_REMOTE "$PUBLIC_IP_URL"
      cat <<'REMOTE'
set -euo pipefail
if command -v curl >/dev/null 2>&1; then
  curl -fsS --max-time 15 "$PUBLIC_IP_URL_REMOTE" 2>/dev/null || true
elif command -v wget >/dev/null 2>&1; then
  wget -qO- --timeout=15 "$PUBLIC_IP_URL_REMOTE" 2>/dev/null || true
fi
REMOTE
    } | remote_bash "$index" || true
)"
  printf '%s' "$value" | tr -cd '0-9a-fA-F:.'
}
