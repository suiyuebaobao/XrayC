#!/usr/bin/env bash
# 用途：为 bootstrap-real-release-env.sh 提供环境值写入、数据库发现和订阅解析函数。
# 说明：该文件只定义函数，由主脚本 source 后调用，不直接打印任何敏感值。
# 范围：仅服务真实发布环境草稿生成流程，不应被独立执行。

write_db_discovery_query() {
  local destination="$1"

  cat > "$destination" <<'SQL'
BEGIN READ ONLY;
WITH subscription_access_lines AS (
  SELECT DISTINCT al.id AS access_line_id
  FROM user_subscriptions us
  JOIN plan_line_groups plg ON plg.plan_id = us.plan_id
  JOIN access_lines al ON (
    (
      al.exit_endpoint_id IS NOT NULL
      AND EXISTS (
        SELECT 1
        FROM line_group_exit_endpoints lgee
        WHERE lgee.line_group_id = plg.line_group_id
          AND lgee.exit_endpoint_id = al.exit_endpoint_id
      )
    )
    OR (
      al.exit_endpoint_id IS NULL
      AND al.line_group_id = plg.line_group_id
    )
  )
  WHERE us.active = TRUE
    AND (us.expires_at IS NULL OR us.expires_at > now())
    AND al.enabled = TRUE
), runtime_supported_endpoints AS (
  SELECT ep.id, ep.outbound_type, ep.host, ep.port, ep.created_at
  FROM exit_endpoints ep
  WHERE ep.enabled = TRUE
    AND (
      trim(COALESCE(ep.host, '')) <> ''
      AND ep.port IS NOT NULL
      AND ep.port > 0
      AND (
        ep.outbound_type IN ('socks', 'http')
        OR (ep.outbound_type = 'vless' AND trim(COALESCE(ep.outbound_config->>'uuid', ep.outbound_config->>'id', '')) <> '')
        OR (
          ep.outbound_type = 'trojan'
          AND trim(COALESCE(ep.outbound_config->>'password', '')) <> ''
          AND lower(trim(COALESCE(ep.outbound_config->>'security', 'tls'))) = 'tls'
          AND trim(COALESCE(ep.outbound_config->>'server_name', ep.outbound_config->>'servername', ep.outbound_config->>'sni', '')) <> ''
        )
        OR (
          ep.outbound_type = 'shadowsocks'
          AND trim(COALESCE(ep.outbound_config->>'method', ep.outbound_config->>'cipher', '')) <> ''
          AND trim(COALESCE(ep.outbound_config->>'password', '')) <> ''
        )
        OR (ep.outbound_type = 'hysteria' AND trim(COALESCE(ep.outbound_config->>'password', ep.outbound_config->>'auth', '')) <> '')
      )
    )
), ready AS (
  SELECT al.id AS access_line_id,
         al.access_node_id,
         concat(al.listen_host, ':', al.listen_port::text) AS access_server,
         rse.id AS exit_endpoint_id,
         row_number() over (partition by al.id order by rse.created_at DESC, rse.id::text) AS rn
  FROM access_lines al
  JOIN subscription_access_lines sal ON sal.access_line_id = al.id
  JOIN exit_pool_members epm ON epm.exit_pool_id = al.exit_pool_id
  JOIN runtime_supported_endpoints rse ON rse.id = epm.exit_endpoint_id
  WHERE al.enabled = TRUE
    AND (al.exit_endpoint_id IS NULL OR rse.id = al.exit_endpoint_id)
    AND COALESCE(epm.status, 'healthy') IN ('healthy','unknown','')
    AND COALESCE(epm.allow_new_assignments, TRUE) = TRUE
), chosen AS (
  SELECT * FROM ready WHERE rn = 1 ORDER BY access_line_id::text LIMIT 1
), protocol_endpoint AS (
  SELECT DISTINCT ON (outbound_type)
         outbound_type::text AS outbound_type,
         id::text AS endpoint_id,
         host,
         outbound_config
  FROM runtime_supported_endpoints
  WHERE trim(COALESCE(host,'')) <> ''
    AND port > 0
  ORDER BY outbound_type, created_at DESC
), token AS (
  SELECT token FROM subscription_tokens
  WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
  ORDER BY created_at DESC LIMIT 1
)
SELECT 'ACCESS_NODE_ID=' || access_node_id::text FROM chosen
UNION ALL SELECT 'ACCESS_LINE_ID=' || access_line_id::text FROM chosen
UNION ALL SELECT 'EXIT_ENDPOINT_ID=' || exit_endpoint_id::text FROM chosen
UNION ALL SELECT 'EXPECTED_ACCESS_SERVERS=' || access_server FROM chosen
UNION ALL SELECT 'SUB_TOKEN=' || token FROM token
UNION ALL SELECT 'EXIT_ENDPOINT_ID_SOCKS=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='socks'
UNION ALL SELECT 'THIRD_PARTY_SOCKS_HOST=' || host FROM protocol_endpoint WHERE outbound_type='socks'
UNION ALL SELECT 'EXIT_ENDPOINT_ID_HTTP=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='http'
UNION ALL SELECT 'THIRD_PARTY_HTTP_HOST=' || host FROM protocol_endpoint WHERE outbound_type='http'
UNION ALL SELECT 'EXIT_ENDPOINT_ID_VLESS=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='vless'
UNION ALL SELECT 'THIRD_PARTY_HOST=' || host FROM protocol_endpoint WHERE outbound_type='vless'
UNION ALL SELECT 'THIRD_PARTY_VLESS_HOST=' || host FROM protocol_endpoint WHERE outbound_type='vless'
UNION ALL SELECT 'EXIT_ENDPOINT_ID_TROJAN=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='trojan'
UNION ALL SELECT 'THIRD_PARTY_TROJAN_HOST=' || host FROM protocol_endpoint WHERE outbound_type='trojan'
UNION ALL SELECT 'THIRD_PARTY_TROJAN_SECURITY=' || COALESCE(NULLIF(outbound_config->>'security', ''), 'tls') FROM protocol_endpoint WHERE outbound_type='trojan'
UNION ALL SELECT 'THIRD_PARTY_TROJAN_SNI=' || COALESCE(NULLIF(outbound_config->>'server_name', ''), NULLIF(outbound_config->>'servername', ''), NULLIF(outbound_config->>'sni', '')) FROM protocol_endpoint WHERE outbound_type='trojan'
UNION ALL SELECT 'EXIT_ENDPOINT_ID_SHADOWSOCKS=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='shadowsocks'
UNION ALL SELECT 'THIRD_PARTY_SHADOWSOCKS_HOST=' || host FROM protocol_endpoint WHERE outbound_type='shadowsocks'
UNION ALL SELECT 'EXIT_ENDPOINT_ID_HY2=' || endpoint_id FROM protocol_endpoint WHERE outbound_type='hysteria'
UNION ALL SELECT 'THIRD_PARTY_HY2_HOST=' || host FROM protocol_endpoint WHERE outbound_type='hysteria';
COMMIT;
SQL
}

quote_env() {
  python3 - "$1" <<'PY'
import shlex, sys
print(shlex.quote(sys.argv[1]))
PY
}

set_value() {
  local name="$1"
  local value="${2:-}"
  [[ -n "$value" ]] || return 0
  value_is_placeholder "$value" && return 0
  grep -q "^${name}=" "$values_file" 2>/dev/null && return 0
  printf '%s=%s\n' "$name" "$(quote_env "$value")" >> "$values_file"
}

set_value_or_report_placeholder() {
  local name="$1"
  local value="${2:-}"
  local source_label="$3"

  if [[ -z "$value" ]]; then
    return 0
  fi
  if value_is_placeholder "$value"; then
    echo "bootstrap-real-release-env: ${name} from ${source_label} skipped (placeholder-like value)"
    return 0
  fi
  set_value "$name" "$value"
}

set_placeholder() {
  local name="$1"
  local value="$2"
  grep -q "^${name}=" "$values_file" 2>/dev/null && return 0
  printf '%s=%s\n' "$name" "$(quote_env "$value")" >> "$values_file"
}

lookup_value() {
  local name="$1"
  python3 - "$values_file" "$name" <<'PY'
import shlex
import sys

path, wanted = sys.argv[1], sys.argv[2]
for raw in open(path, "r", encoding="utf-8"):
    raw = raw.strip()
    if not raw or raw.startswith("#") or "=" not in raw:
        continue
    key, value = raw.split("=", 1)
    if key != wanted:
        continue
    try:
        parts = shlex.split(value)
    except ValueError:
        parts = [value]
    print(parts[0] if parts else "")
    break
PY
}

value_is_placeholder() {
  local value="${1:-}"
  [[ -z "$value" ]] && return 0
  [[ "$value" == *'<'*'>'* ]] && return 0
  [[ "$value" == copy-from-* ]] && return 0
  [[ "$value" == *placeholder* ]] && return 0
  [[ "$value" == *from-private-env* ]] && return 0
  [[ "$value" == *set-in-env* ]] && return 0
  [[ "$value" == *private-target-name* ]] && return 0
  [[ "$value" == *private-inventory-path* ]] && return 0
  [[ "$value" == *path-to-private-inventory* ]] && return 0
  [[ "$value" == *real-control-plane-host* ]] && return 0
  [[ "$value" == *upstream-host* ]] && return 0
  [[ "$value" == *change-me* ]] && return 0
  [[ "$value" == *dummy* ]] && return 0
  [[ "$value" == *your-secret* ]] && return 0
  [[ "$value" == *example* ]] && return 0
  return 1
}

db_env_key_is_allowed() {
  case "$1" in
    ACCESS_NODE_ID|ACCESS_LINE_ID|EXIT_ENDPOINT_ID|EXPECTED_ACCESS_SERVERS|SUB_TOKEN|\
    EXIT_ENDPOINT_ID_SOCKS|THIRD_PARTY_SOCKS_HOST|\
    EXIT_ENDPOINT_ID_HTTP|THIRD_PARTY_HTTP_HOST|\
    EXIT_ENDPOINT_ID_VLESS|THIRD_PARTY_HOST|THIRD_PARTY_VLESS_HOST|\
    EXIT_ENDPOINT_ID_TROJAN|THIRD_PARTY_TROJAN_HOST|THIRD_PARTY_TROJAN_SECURITY|THIRD_PARTY_TROJAN_SNI|\
    EXIT_ENDPOINT_ID_SHADOWSOCKS|THIRD_PARTY_SHADOWSOCKS_HOST|\
    EXIT_ENDPOINT_ID_HY2|THIRD_PARTY_HY2_HOST)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

apply_db_env_file() {
  local source_label="$1"
  local env_file="$2"
  local key=""
  local value=""
  local discovered_count=0

  while IFS='=' read -r key value; do
    [[ -n "${key:-}" && -n "${value:-}" ]] || continue
    db_env_key_is_allowed "$key" || continue
    set_value "$key" "$value"
    discovered_count=$((discovered_count + 1))
  done < "$env_file"

  echo "bootstrap-real-release-env: database discovery via ${source_label} collected ${discovered_count} variable(s)"
  if [[ "$discovered_count" -gt 0 ]]; then
    return 0
  fi
  return 1
}

discover_expected_access_servers_from_subscription() {
  local base_url=""
  local subscription_url=""
  local sub_token=""
  local subscription_file="$tmp_dir/subscription.yaml"
  local status_file="$tmp_dir/subscription.status"
  local servers_file="$tmp_dir/subscription-servers.txt"
  local discovered_servers=""

  base_url="$(lookup_value BASE_URL)"
  subscription_url="$(lookup_value SUBSCRIPTION_URL)"
  sub_token="$(lookup_value SUB_TOKEN)"
  if [[ -z "$subscription_url" && -n "$base_url" && -n "$sub_token" ]]; then
    subscription_url="${base_url%/}/sub/${sub_token}"
  fi
  if [[ "$subscription_url" == /* && -n "$base_url" ]]; then
    subscription_url="${base_url%/}${subscription_url}"
  fi
  [[ -n "$subscription_url" ]] || return 0

  if ! curl --silent --location --max-time "${CURL_TIMEOUT:-15}" \
    --output "$subscription_file" \
    --write-out "%{http_code}" \
    "$subscription_url" > "$status_file" 2>/dev/null; then
    echo "bootstrap-real-release-env: subscription access-server discovery skipped (download failed)"
    return 0
  fi
  case "$(cat "$status_file")" in
    2*) ;;
    *)
      echo "bootstrap-real-release-env: subscription access-server discovery skipped (non-2xx)"
      return 0
      ;;
  esac

  if ! python3 - "$subscription_file" > "$servers_file" <<'PY'
import sys

try:
    import yaml
except Exception:
    raise SystemExit(2)

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)

servers = []
seen = set()
if isinstance(payload, dict):
    proxies = payload.get("proxies")
    if isinstance(proxies, list):
        for proxy in proxies:
            if not isinstance(proxy, dict):
                continue
            server = str(proxy.get("server", "")).strip()
            port = proxy.get("port")
            if not server or port in (None, ""):
                continue
            item = f"{server}:{port}"
            if item not in seen:
                seen.add(item)
                servers.append(item)

if servers:
    print(",".join(servers))
PY
  then
    echo "bootstrap-real-release-env: subscription access-server discovery skipped (YAML parser unavailable or invalid)"
    return 0
  fi

  if [[ -s "$servers_file" ]]; then
    discovered_servers="$(cat "$servers_file")"
    if value_is_placeholder "$discovered_servers"; then
      echo "bootstrap-real-release-env: subscription access-server discovery skipped (placeholder-like value)"
      return 0
    fi
    set_value EXPECTED_ACCESS_SERVERS "$discovered_servers"
    echo "bootstrap-real-release-env: subscription access-server discovery collected EXPECTED_ACCESS_SERVERS"
  else
    echo "bootstrap-real-release-env: subscription access-server discovery skipped (no proxies)"
  fi
}

create_pg_service_from_url() {
  local database_url="$1"
  local service_file="$2"
  local pass_file="$3"
  local database_url_file="$tmp_dir/database-url.txt"

  printf '%s' "$database_url" > "$database_url_file"
  python3 - "$database_url_file" "$service_file" "$pass_file" <<'PY'
import os
import sys
from urllib.parse import parse_qs, unquote, urlparse

url_file, service_file, pass_file = sys.argv[1], sys.argv[2], sys.argv[3]
with open(url_file, "r", encoding="utf-8") as fh:
    raw_url = fh.read().strip()

parsed = urlparse(raw_url)
if parsed.scheme not in {"postgres", "postgresql"}:
    raise SystemExit(2)

host = parsed.hostname or ""
port = parsed.port or 5432
user = unquote(parsed.username or "")
password = unquote(parsed.password or "")
dbname = unquote((parsed.path or "").lstrip("/"))
if not host or not dbname:
    raise SystemExit(2)

def clean(value: str) -> str:
    if "\n" in value or "\r" in value:
        raise SystemExit(2)
    return value

query = parse_qs(parsed.query)
lines = [
    "[xrayc_real_release]",
    f"host={clean(host)}",
    f"port={int(port)}",
    f"dbname={clean(dbname)}",
]
if user:
    lines.append(f"user={clean(user)}")
lines.append(f"passfile={clean(pass_file)}")
for key in ("sslmode", "sslrootcert", "sslcert", "sslkey", "connect_timeout"):
    value = query.get(key, [""])[0]
    if value:
        lines.append(f"{key}={clean(value)}")

with open(service_file, "w", encoding="utf-8") as fh:
    fh.write("\n".join(lines) + "\n")
os.chmod(service_file, 0o600)

def pgpass_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace(":", "\\:")

with open(pass_file, "w", encoding="utf-8") as fh:
    if user and password:
        fh.write(
            ":".join(
                [
                    pgpass_escape(host),
                    str(int(port)),
                    pgpass_escape(dbname),
                    pgpass_escape(user),
                    pgpass_escape(password),
                ]
            )
            + "\n"
        )
os.chmod(pass_file, 0o600)
PY
}

discover_database_from_url() {
  local source_label="$1"
  local database_url="$2"
  local output_file="$tmp_dir/db-${source_label}.env"
  local error_file="$tmp_dir/db-${source_label}.err"
  local service_file="$tmp_dir/db-${source_label}.pgservice"
  local pass_file="$tmp_dir/db-${source_label}.pgpass"

  if value_is_placeholder "$database_url"; then
    echo "bootstrap-real-release-env: database discovery via ${source_label} skipped (placeholder DATABASE_URL)"
    return 1
  fi
  if ! command -v psql >/dev/null 2>&1; then
    echo "bootstrap-real-release-env: database discovery via ${source_label} skipped (psql unavailable)"
    return 1
  fi
  if ! create_pg_service_from_url "$database_url" "$service_file" "$pass_file"; then
    echo "bootstrap-real-release-env: database discovery via ${source_label} skipped (invalid DATABASE_URL)"
    return 1
  fi
  if PGSERVICEFILE="$service_file" PGSERVICE=xrayc_real_release psql -XAtq -v ON_ERROR_STOP=1 -f "$db_query_file" > "$output_file" 2> "$error_file"; then
    if apply_db_env_file "$source_label" "$output_file"; then
      return 0
    fi
    return 1
  fi

  echo "bootstrap-real-release-env: database discovery via ${source_label} skipped (query failed)"
  return 1
}

discover_database_from_compose() {
  local postgres_user="$1"
  local postgres_db="$2"
  local output_file="$tmp_dir/db-compose.env"
  local error_file="$tmp_dir/db-compose.err"

  if docker compose exec -T postgres psql -U "$postgres_user" -d "$postgres_db" -XAtq -v ON_ERROR_STOP=1 -f - < "$db_query_file" > "$output_file" 2> "$error_file"; then
    if apply_db_env_file "local compose" "$output_file"; then
      return 0
    fi
    return 1
  fi

  echo "bootstrap-real-release-env: database discovery via local compose skipped (query failed)"
  return 1
}

count_filled_variables() {
  local env_file="$1"

  python3 - "$env_file" <<'PY'
import shlex
import sys

def is_placeholder(value: str) -> bool:
    patterns = [
        ("<", ">"),
        ("copy-from-", None),
        ("placeholder", None),
        ("from-private-env", None),
        ("set-in-env", None),
        ("private-target-name", None),
        ("private-inventory-path", None),
        ("path-to-private-inventory", None),
        ("real-control-plane-host", None),
        ("upstream-host", None),
        ("change-me", None),
        ("dummy", None),
        ("your-secret", None),
        ("example", None),
    ]
    if not value:
        return True
    for left, right in patterns:
        if right is not None:
            if left in value and right in value:
                return True
        elif left in value:
            return True
    return False

count = 0
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    for raw in fh:
        raw = raw.strip()
        if not raw or raw.startswith("#") or "=" not in raw:
            continue
        _, value = raw.split("=", 1)
        try:
            parsed = shlex.split(value)[0]
        except Exception:
            parsed = value.strip("'\"")
        if not is_placeholder(parsed):
            count += 1
print(count)
PY
}
