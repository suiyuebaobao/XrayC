#!/usr/bin/env bash
# 用途：提供真实矩阵中继 E2E 的协议矩阵解析与校验函数。
# 负责协议别名归一化、环境变量后缀映射、出口 IP 校验和完整发布矩阵检查。
# 本文件只读取主入口已加载的环境变量，不输出第三方 endpoint 敏感值。
# 校验失败通过 common.sh 中的 die 终止。

protocol_suffix() {
  case "$1" in
    socks|socks5) printf 'SOCKS' ;;
    http) printf 'HTTP' ;;
    vless) printf 'VLESS' ;;
    trojan) printf 'TROJAN' ;;
    shadowsocks|ss) printf 'SHADOWSOCKS' ;;
    hy2|hysteria|hysteria2) printf 'HY2' ;;
    *) printf '' ;;
  esac
}

normalize_protocol() {
  local protocol="$1"
  protocol="$(printf '%s' "$protocol" | tr '[:upper:]' '[:lower:]' | xargs)"
  case "$protocol" in
    socks5) printf 'socks' ;;
    ss) printf 'shadowsocks' ;;
    hysteria|hysteria2) printf 'hy2' ;;
    *) printf '%s' "$protocol" ;;
  esac
}

protocol_outbound_type() {
  case "$1" in
    hy2) printf 'hysteria' ;;
    *) printf '%s' "$1" ;;
  esac
}

assert_public_ip_literal() {
  local name="$1"
  local value="$2"
  if ! python3 - "$value" <<'PY' >/dev/null 2>&1
import ipaddress
import sys

try:
    ip = ipaddress.ip_address(sys.argv[1])
except ValueError:
    raise SystemExit(1)
raise SystemExit(0 if ip.is_global else 1)
PY
  then
    die "${name} must be a public IPv4 or IPv6 address"
  fi
}

require_matrix_protocols() {
  local item protocol suffix endpoint_var expected_var endpoint_id expected_ip outbound_type
  IFS=',' read -r -a raw_protocols <<< "$PROTOCOLS"
  [[ "${#raw_protocols[@]}" -gt 0 ]] || die "protocol list is empty"
  for item in "${raw_protocols[@]}"; do
    protocol="$(normalize_protocol "$item")"
    [[ -n "$protocol" ]] || die "protocol list contains an empty item"
    suffix="$(protocol_suffix "$protocol")"
    [[ -n "$suffix" ]] || die "protocol list contains an unsupported protocol"
    endpoint_var="EXIT_ENDPOINT_ID_${suffix}"
    expected_var="EXPECTED_EXIT_IP_${suffix}"
    endpoint_id="${!endpoint_var:-}"
    expected_ip="${!expected_var:-}"
    [[ -n "$endpoint_id" ]] || die "${endpoint_var} is required in private matrix env"
    [[ "$endpoint_id" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
      || die "${endpoint_var} must be a UUID"
    [[ -n "$expected_ip" ]] || die "${expected_var} is required in private matrix env"
    assert_public_ip_literal "$expected_var" "$expected_ip"
    outbound_type="$(protocol_outbound_type "$protocol")"
    protocol_names+=("$protocol")
    protocol_suffixes+=("$suffix")
    protocol_endpoint_ids+=("$endpoint_id")
    protocol_outbound_types+=("$outbound_type")
    protocol_expected_ips+=("$expected_ip")
  done
}

require_full_release_protocol_matrix() {
  xrayc_real_e2e_bool_is_true "${XRAYC_REAL_RELEASE:-0}" || return 0
  local required_protocol protocol found
  for required_protocol in socks http vless trojan shadowsocks hy2; do
    found=0
    for protocol in "${protocol_names[@]}"; do
      if [[ "$protocol" == "$required_protocol" ]]; then
        found=1
        break
      fi
    done
    [[ "$found" -eq 1 ]] || die "real release relay matrix requires ${required_protocol}"
  done
  [[ "${#protocol_names[@]}" -eq 6 ]] || die "real release relay matrix requires exactly six protocols"
}

assert_protocol_matrix_endpoints_subscription_eligible() {
  local endpoint_ids="$1"
  local ineligible_count=""

  ineligible_count="$(psql_db -XAtq -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_ids" <<'SQL'
WITH selected AS (
  SELECT endpoint_id, ordinality
  FROM unnest(string_to_array(:'endpoint_ids', ',')::uuid[]) WITH ORDINALITY AS t(endpoint_id, ordinality)
), endpoint_state AS (
  SELECT s.endpoint_id,
         e.enabled AS endpoint_enabled,
         r.enabled AS resource_enabled,
         COALESCE(NULLIF(lower(trim(r.status)), ''), 'healthy') AS resource_status,
         trim(e.host) <> '' AND e.port > 0 AS host_port_ok,
         CASE
           WHEN e.outbound_type IN ('socks', 'http') THEN (
             e.outbound_config = '{}'::jsonb
             OR (
               trim(COALESCE(e.outbound_config->>'username', e.outbound_config->>'user', '')) <> ''
               AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'pass', '')) <> ''
             )
           )
           WHEN e.outbound_type = 'vless' THEN (
             trim(COALESCE(e.outbound_config->>'uuid', e.outbound_config->>'id', '')) <> ''
             AND (
               lower(trim(COALESCE(e.outbound_config->>'security', 'tls'))) IN ('', 'tls', 'none')
               OR (
                 lower(trim(COALESCE(e.outbound_config->>'security', ''))) = 'reality'
                 AND trim(COALESCE(e.outbound_config->>'server_name', e.outbound_config->>'servername', e.outbound_config->>'sni', '')) <> ''
                 AND trim(COALESCE(e.outbound_config->>'public_key', e.outbound_config->>'publicKey', e.outbound_config->>'reality_public_key', '')) <> ''
               )
             )
           )
           WHEN e.outbound_type = 'trojan' THEN trim(COALESCE(e.outbound_config->>'password', '')) <> ''
           WHEN e.outbound_type = 'shadowsocks' THEN (
             trim(COALESCE(e.outbound_config->>'method', e.outbound_config->>'cipher', '')) <> ''
             AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
           )
           WHEN e.outbound_type = 'hysteria' THEN trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'auth', '')) <> ''
           ELSE FALSE
         END AS protocol_config_ok
  FROM selected s
  LEFT JOIN exit_endpoints e ON e.id = s.endpoint_id
  LEFT JOIN exit_resources r ON r.id = e.exit_resource_id
)
SELECT COUNT(*)::text
FROM endpoint_state
WHERE NOT (
  endpoint_enabled = TRUE
  AND resource_enabled = TRUE
  AND resource_status <> 'offline'
  AND host_port_ok = TRUE
  AND protocol_config_ok = TRUE
);
SQL
)"
  [[ "$ineligible_count" == "0" ]] \
    || die "one or more real protocol matrix endpoints are not subscription eligible; refresh endpoint resources before relay E2E"
}

refresh_protocol_matrix_endpoint_health() {
  local endpoint_ids="$1"
  psql_db -Xq -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_ids" <<'SQL'
WITH selected AS (
  SELECT unnest(string_to_array(:'endpoint_ids', ',')::uuid[]) AS endpoint_id
), touched_resources AS (
  UPDATE exit_resources r
  SET enabled = TRUE,
      status = 'healthy',
      last_probe_status = 'healthy',
      last_probe_at = now()
  FROM exit_endpoints e
  JOIN selected s ON s.endpoint_id = e.id
  WHERE e.exit_resource_id = r.id
  RETURNING r.id
)
UPDATE exit_endpoints e
SET enabled = TRUE,
    last_probe_status = 'healthy',
    last_probe_at = now()
FROM selected s
WHERE e.id = s.endpoint_id;
SQL
}

select_initial_relay_endpoint_id() {
  local endpoint_ids="$1"
  psql_db -XAtq -v ON_ERROR_STOP=1 -v endpoint_ids="$endpoint_ids" <<'SQL'
WITH selected AS (
  SELECT endpoint_id, ordinality
  FROM unnest(string_to_array(:'endpoint_ids', ',')::uuid[]) WITH ORDINALITY AS t(endpoint_id, ordinality)
), eligible AS (
  SELECT s.endpoint_id, s.ordinality
  FROM selected s
  JOIN exit_endpoints e ON e.id = s.endpoint_id
  JOIN exit_resources r ON r.id = e.exit_resource_id
  WHERE e.enabled = TRUE
    AND r.enabled = TRUE
    AND COALESCE(NULLIF(lower(trim(r.status)), ''), 'healthy') <> 'offline'
    AND trim(e.host) <> ''
    AND e.port > 0
  ORDER BY s.ordinality
)
SELECT endpoint_id::text
FROM eligible
ORDER BY ordinality
LIMIT 1;
SQL
}

assert_protocol_endpoint_reachable_from_relay() {
  local protocol="$1"
  local endpoint_id="$2"
  local env_file="$TMP_DIR/endpoint-probe-${protocol}.env"
  local endpoint_row outbound_type host port

  endpoint_row="$(psql_db -XAtq -F $'\t' -v ON_ERROR_STOP=1 -v endpoint_id="$endpoint_id" <<'SQL'
SELECT outbound_type::text,
       host,
       port::text
FROM exit_endpoints
WHERE id = :'endpoint_id'::uuid
LIMIT 1;
SQL
)"
  [[ -n "$endpoint_row" ]] || die "real protocol matrix endpoint is missing before upstream probe"
  IFS=$'\t' read -r outbound_type host port <<< "$endpoint_row"
  [[ -n "$host" && "$port" =~ ^[0-9]+$ ]] || die "real protocol matrix endpoint has invalid host or port before upstream probe"

  ENDPOINT_PROTOCOL_VALUE="$protocol" \
    ENDPOINT_OUTBOUND_TYPE_VALUE="$outbound_type" \
    ENDPOINT_HOST_VALUE="$host" \
    ENDPOINT_PORT_VALUE="$port" \
    PUBLIC_IP_URLS_VALUE="$PUBLIC_IP_URLS_CSV" \
    python3 - "$env_file" <<'PY'
import os
import shlex
from urllib.parse import unquote, urlsplit

output = os.sys.argv[1]
protocol = os.environ["ENDPOINT_PROTOCOL_VALUE"]
outbound_type = os.environ["ENDPOINT_OUTBOUND_TYPE_VALUE"]

PLACEHOLDER_BITS = (
    "<",
    "copy-from-",
    "from-private-env",
    "set-in-env",
    "private-host",
    "provider-console",
    "change-me",
    "placeholder",
)

def ready(name):
    value = os.environ.get(name, "").strip()
    if not value:
        return ""
    lowered = value.lower()
    if any(bit in lowered for bit in PLACEHOLDER_BITS):
        return ""
    return value

suffix = {
    "socks": "SOCKS",
    "http": "HTTP",
}.get(protocol, "")

host = os.environ["ENDPOINT_HOST_VALUE"]
port = os.environ["ENDPOINT_PORT_VALUE"]
scheme = ""
username = ""
password = ""

if suffix:
    raw_url = ready(f"THIRD_PARTY_{suffix}_RAW_URL")
    if raw_url:
        parsed = urlsplit(raw_url)
        scheme = parsed.scheme.lower()
        host = parsed.hostname or host
        port = str(parsed.port or port)
        username = unquote(parsed.username or "")
        password = unquote(parsed.password or "")
    else:
        host = ready(f"THIRD_PARTY_{suffix}_HOST") or host
        port = ready(f"THIRD_PARTY_{suffix}_PORT") or port
        username = ready(f"THIRD_PARTY_{suffix}_USERNAME") or ready(f"THIRD_PARTY_{suffix}_USER")
        password = (
            ready(f"THIRD_PARTY_{suffix}_PASSWORD")
            or ready(f"THIRD_PARTY_{suffix}_PASS")
            or ready(f"THIRD_PARTY_{suffix}_KEY")
        )
    if not scheme:
        scheme = "socks5h" if suffix == "SOCKS" else "http"

values = {
    "ENDPOINT_PROTOCOL": protocol,
    "ENDPOINT_OUTBOUND_TYPE": outbound_type,
    "ENDPOINT_HOST": host,
    "ENDPOINT_PORT": port,
    "ENDPOINT_PROXY_SCHEME": scheme,
    "ENDPOINT_USERNAME": username,
    "ENDPOINT_PASSWORD": password,
    "PUBLIC_IP_URLS_VALUE": os.environ["PUBLIC_IP_URLS_VALUE"],
}
with open(output, "w", encoding="utf-8") as fh:
    for key, value in values.items():
        fh.write(f"{key}={shlex.quote(str(value))}\n")
os.chmod(output, 0o600)
PY

  if ! remote_bash_env 2 "$env_file" <<'REMOTE'
tmp_config=""
cleanup_probe() {
  [ -z "$tmp_config" ] || rm -f "$tmp_config"
}
trap cleanup_probe EXIT

probe_public_ip_proxy() {
  proxy_url="$1"
  tmp_config="$(mktemp)"
  chmod 600 "$tmp_config"
  printf 'proxy = "%s"\n' "$proxy_url" > "$tmp_config"
  if [ -n "${ENDPOINT_USERNAME:-}" ]; then
    printf 'proxy-user = "%s:%s"\n' "$ENDPOINT_USERNAME" "$ENDPOINT_PASSWORD" >> "$tmp_config"
  fi
  IFS=',' read -r -a urls <<< "$PUBLIC_IP_URLS_VALUE"
  for url in "${urls[@]}"; do
    [ -n "$url" ] || continue
    if curl --fail --silent --show-error --max-time 15 --config "$tmp_config" "$url" >/dev/null 2>&1; then
      return 0
    fi
  done
  return 1
}

probe_tcp_connect() {
  ENDPOINT_HOST="$ENDPOINT_HOST" ENDPOINT_PORT="$ENDPOINT_PORT" python3 - <<'PY'
import os
import socket

host = os.environ["ENDPOINT_HOST"]
port = int(os.environ["ENDPOINT_PORT"])
with socket.create_connection((host, port), timeout=10):
    pass
PY
}

case "$ENDPOINT_OUTBOUND_TYPE" in
  socks)
    probe_public_ip_proxy "${ENDPOINT_PROXY_SCHEME:-socks5h}://${ENDPOINT_HOST}:${ENDPOINT_PORT}"
    ;;
  http)
    probe_public_ip_proxy "${ENDPOINT_PROXY_SCHEME:-http}://${ENDPOINT_HOST}:${ENDPOINT_PORT}"
    ;;
  hysteria)
    # HY2 is UDP; a generic TCP probe would produce false failures.
    exit 0
    ;;
  *)
    probe_tcp_connect
    ;;
esac
REMOTE
  then
    die "real protocol matrix upstream probe failed for protocol=${protocol}"
  fi
  status "upstream_probe_ok protocol=${protocol}"
}
