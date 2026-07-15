#!/usr/bin/env bash
# 这个 helper 负责真实入站矩阵 E2E 的订阅下载。
# 这个 helper 负责临时补齐缺失入站协议并等待配置生效。
# 这个 helper 负责接入节点临时端口放行和清理。
# 这个 helper 负责生成客户端协议配置所需的端口参数。
# 这个 helper 只定义函数，由主脚本 source 后调用。
# 这个 helper 不打印订阅地址、数据库地址或真实端口细节。
# 这个 helper 属于 real-access-inbound-matrix-e2e 的拆分实现。
# 这个 helper 依赖主脚本提供 TMP_DIR、SCRIPT_DIR 和状态函数。
# 这个 helper 依赖 real-e2e-lib 提供安全下载和数据库访问函数。
# 这个 helper 修改时需保持 bash -n 通过。

subscription_download_url() {
  if [[ -n "${SUBSCRIPTION_URL:-}" ]]; then
    printf '%s' "$SUBSCRIPTION_URL"
  else
    printf '%s/sub/%s' "${BASE_URL%/}" "$SUB_TOKEN"
  fi
}

download_subscription() {
  local url
  url="$(subscription_download_url)"
  xrayc_real_e2e_fetch_sensitive_url_to_file "$url" "$TMP_DIR/subscription.yaml" "inbound matrix subscription download failed"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$TMP_DIR/subscription.yaml"
}

missing_subscription_protocols() {
  collect_client_config_preferred_ports
  python3 "${SCRIPT_DIR}/real-access-inbound-matrix-client-configs.py" \
    missing "$TMP_DIR/subscription.yaml" \
    "${CLIENT_CONFIG_PREFERRED_PORT_ARGS[@]}" "${protocol_names[@]}"
}

prepare_missing_subscription_protocols() {
  local missing_csv="$1"
  local access_node_hint listen_host_value selected_access_target

  [[ -n "$missing_csv" ]] || return 0
  if [[ "$AUTO_PREPARE" != "1" ]]; then
    die "subscription is missing required inbound protocol entries"
  fi
  [[ -f "$DB_PREPARE_SCRIPT" ]] || die "DB prepare helper is missing"
  require_command psql
  require_real_env DATABASE_URL
  require_real_env SUB_TOKEN
  access_node_hint="${REAL_ACCESS_INBOUND_MATRIX_ACCESS_NODE_HINT:-$ACCESS_TARGET}"
  listen_host_value="${REAL_ACCESS_INBOUND_MATRIX_LISTEN_HOST:-}"
  if [[ -z "$listen_host_value" ]]; then
    selected_access_target="$(select_active_inventory_access_target "$access_node_hint" || true)"
    if [[ -n "$selected_access_target" ]]; then
      access_node_hint="$selected_access_target"
      ACCESS_TARGET="$selected_access_target"
      listen_host_value="$(inventory_tls_domain_for_access_target "$selected_access_target" || true)"
    fi
  fi
  reject_cloudflare_inventory_target "$access_node_hint" "access"
  # Do not use the stale default target to choose a TLS listen host; when no
  # explicit env override exists, the domain must come from the active target.
  status "preparing_missing_inbound_protocols"
  REAL_ACCESS_INBOUND_MATRIX_ACCESS_NODE_HINT="$access_node_hint" \
  REAL_ACCESS_INBOUND_MATRIX_LISTEN_HOST="$listen_host_value" \
  bash "$DB_PREPARE_SCRIPT" prepare "$missing_csv" "$TEMP_PREP_MANIFEST" >/dev/null \
    || die "temporary inbound protocol preparation failed; details redacted"
  open_prepared_access_ports
  wait_prepared_access_node_applied_config
  wait_prepared_access_ports_listening
  wait_prepared_access_udp_ports_listening
  download_subscription
  missing_csv="$(missing_subscription_protocols)"
  [[ -z "$missing_csv" ]] || die "subscription is still missing required inbound protocol entries after preparation"
  status "temporary_inbound_protocols_ready"
}

inventory_listen_host_for_access_target() {
  python3 - "$INVENTORY" "$ACCESS_TARGET" <<'PY'
import json
import re
import sys

inventory_path, access_target = sys.argv[1:3]
with open(inventory_path, "r", encoding="utf-8") as fh:
    inventory = json.load(fh)
selected = None
for target in inventory.get("targets", [])[:4]:
    if str(target.get("alias", "")).strip() == access_target:
        selected = target
        break
if not selected:
    raise SystemExit(0)
domain_keys = ("public_domain", "domain", "tls_domain", "sni", "public_host")
for key in domain_keys:
    value = str(selected.get(key, "")).strip()
    if value and not re.match(r"^[0-9]+(\.[0-9]+){3}$", value):
        print(value)
        raise SystemExit(0)
for key in ("public_host", "ssh_host"):
    value = str(selected.get(key, "")).strip()
    if value:
        print(value)
        raise SystemExit(0)
PY
}

inventory_tls_domain_for_access_target() {
  local access_target="$1"

  python3 - "$INVENTORY" "$access_target" <<'PY'
import json
import re
import sys

inventory_path, access_target = sys.argv[1:3]
with open(inventory_path, "r", encoding="utf-8") as fh:
    inventory = json.load(fh)
for target in inventory.get("targets", [])[:4]:
    if str(target.get("alias", "")).strip() != access_target:
        continue
    for key in ("public_domain", "domain", "tls_domain", "sni", "public_host"):
        value = str(target.get(key, "")).strip()
        if value and not re.match(r"^[0-9]+(\.[0-9]+){3}$", value):
            print(value)
            raise SystemExit(0)
raise SystemExit(0)
PY
}

select_active_inventory_access_target() {
  local preferred_hint="${1:-}" output_file

  [[ -n "${DATABASE_URL:-}" && -f "$INVENTORY" ]] || return 0
  output_file="$TMP_DIR/active-access-targets"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" \
    -XAtq \
    -v ON_ERROR_STOP=1 \
    -v "access_node_hint=${preferred_hint}" >"$output_file" 2>/dev/null <<'SQL' || return 0
SELECT COALESCE(n.name, '') || E'\t' || COALESCE(n.public_host, '')
FROM access_nodes n
WHERE n.last_heartbeat_at IS NOT NULL
  AND n.last_heartbeat_at >= now() - interval '120 seconds'
  AND n.config_dirty = FALSE
  AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
  AND COALESCE(n.desired_config_hash, '') <> ''
  AND COALESCE(n.applied_config_hash, '') = COALESCE(n.desired_config_hash, '')
  AND lower(COALESCE(NULLIF(btrim(n.status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
ORDER BY
  CASE
    WHEN lower(NULLIF(btrim(:'access_node_hint'), '')) IS NOT NULL
     AND (
       lower(n.name) = lower(NULLIF(btrim(:'access_node_hint'), ''))
       OR lower(n.name) LIKE '%' || lower(NULLIF(btrim(:'access_node_hint'), ''))
       OR lower(n.public_host) = lower(NULLIF(btrim(:'access_node_hint'), ''))
     )
    THEN 1 ELSE 0
  END DESC,
  n.name ASC,
  n.id ASC
LIMIT 20;
SQL
  python3 - "$INVENTORY" "$output_file" <<'PY'
import json
import sys

inventory_path, probe_path = sys.argv[1:3]
with open(inventory_path, "r", encoding="utf-8") as fh:
    inventory = json.load(fh)
targets = []
for target in inventory.get("targets", [])[:4]:
    alias = str(target.get("alias", "")).strip()
    ssh_host = str(target.get("ssh_host", "")).strip()
    if alias:
        targets.append((alias, ssh_host))
with open(probe_path, "r", encoding="utf-8") as fh:
    for line in fh:
        line = line.strip()
        if not line:
            continue
        parts = line.split("\t", 1)
        node_name = parts[0].strip()
        public_host = parts[1].strip() if len(parts) > 1 else ""
        for alias, ssh_host in targets:
            if node_name == alias or node_name.endswith(alias) or (public_host and public_host == ssh_host):
                print(alias)
                raise SystemExit(0)
raise SystemExit(0)
PY
}

inventory_alias_for_node_probe() {
  local probe_file="$1"

  python3 - "$INVENTORY" "$probe_file" <<'PY'
import json
import sys

inventory_path, probe_path = sys.argv[1:3]
with open(probe_path, "r", encoding="utf-8") as fh:
    line = fh.readline().strip()
if not line:
    raise SystemExit(0)
parts = line.split("\t", 1)
node_name = parts[0].strip()
public_host = parts[1].strip() if len(parts) > 1 else ""
with open(inventory_path, "r", encoding="utf-8") as fh:
    inventory = json.load(fh)
for target in inventory.get("targets", [])[:4]:
    alias = str(target.get("alias", "")).strip()
    ssh_host = str(target.get("ssh_host", "")).strip()
    if not alias:
        continue
    if node_name == alias or node_name.endswith(alias) or (public_host and public_host == ssh_host):
        print(alias)
        raise SystemExit(0)
PY
}

infer_access_target_from_template() {
  local output_file target manifest_access_node_id

  output_file="$TMP_DIR/inferred-access-target"
  if [[ -f "$TEMP_PREP_MANIFEST" ]]; then
    manifest_access_node_id="$(awk -F= '$1 == "ACCESS_NODE_ID" {print $2; exit}' "$TEMP_PREP_MANIFEST")"
  fi
  if [[ -n "${DATABASE_URL:-}" && -n "${manifest_access_node_id:-}" ]]; then
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" \
      -XAtq \
      -v ON_ERROR_STOP=1 \
      -v "access_node_id=${manifest_access_node_id}" >"$output_file" 2>/dev/null <<'SQL' || return 0
SELECT COALESCE(n.name, '') || E'\t' || COALESCE(n.public_host, '')
FROM access_nodes n
WHERE n.id = :'access_node_id'::uuid
LIMIT 1;
SQL
    target="$(inventory_alias_for_node_probe "$output_file" | tr -d '\r\n')"
    value_is_placeholder "$target" || {
      printf '%s' "$target"
      return 0
    }
  fi

  [[ -n "${DATABASE_URL:-}" && -n "${ACCESS_LINE_ID:-}" ]] || return 0
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" \
    -XAtq \
    -v ON_ERROR_STOP=1 \
    -v "access_line_id=${ACCESS_LINE_ID}" >"$output_file" 2>/dev/null <<'SQL' || return 0
SELECT COALESCE(n.name, '') || E'\t' || COALESCE(n.public_host, '')
FROM access_lines l
JOIN access_nodes n ON n.id = l.access_node_id
WHERE l.id = :'access_line_id'::uuid
LIMIT 1;
SQL
  target="$(inventory_alias_for_node_probe "$output_file" | tr -d '\r\n')"
  value_is_placeholder "$target" && return 0
  printf '%s' "$target"
}

wait_prepared_access_node_applied_config() {
  local access_node_id="" timeout_seconds poll_seconds elapsed state output_file

  [[ -f "$TEMP_PREP_MANIFEST" ]] || return 0
  # shellcheck disable=SC1090
  . "$TEMP_PREP_MANIFEST"
  access_node_id="${ACCESS_NODE_ID:-}"
  [[ "$access_node_id" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]] \
    || die "temporary inbound protocol manifest is missing access node id"
  require_real_env DATABASE_URL

  timeout_seconds="${REAL_ACCESS_INBOUND_MATRIX_CONFIG_APPLY_TIMEOUT_SECONDS:-180}"
  poll_seconds="${REAL_ACCESS_INBOUND_MATRIX_CONFIG_APPLY_POLL_SECONDS:-2}"
  [[ "$timeout_seconds" =~ ^[0-9]+$ && "$timeout_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_CONFIG_APPLY_TIMEOUT_SECONDS must be a positive integer"
  [[ "$poll_seconds" =~ ^[0-9]+$ && "$poll_seconds" -gt 0 ]] \
    || die_usage "REAL_ACCESS_INBOUND_MATRIX_CONFIG_APPLY_POLL_SECONDS must be a positive integer"

  elapsed=0
  output_file="$TMP_DIR/access-node-config-state"
  while [[ "$elapsed" -le "$timeout_seconds" ]]; do
    if xrayc_real_e2e_psql_database_url "$DATABASE_URL" \
      -XAtq \
      -v ON_ERROR_STOP=1 \
      -v "access_node_id=${access_node_id}" >"$output_file" 2>/dev/null <<'SQL'
SELECT CASE
  WHEN config_dirty = FALSE
   AND last_heartbeat_at IS NOT NULL
   AND last_heartbeat_at >= now() - interval '120 seconds'
   AND COALESCE(desired_config_hash, '') <> ''
   AND COALESCE(applied_config_hash, '') = COALESCE(desired_config_hash, '')
  THEN 'ok'
  ELSE 'wait'
END
FROM access_nodes
WHERE id = :'access_node_id'::uuid
LIMIT 1;
SQL
    then
      state="$(tr -d '\r\n' <"$output_file")"
      if [[ "$state" == "ok" ]]; then
        status "temporary_inbound_config_applied"
        return 0
      fi
    fi
    sleep "$poll_seconds"
    elapsed=$((elapsed + poll_seconds))
  done
  die "temporary inbound protocol config was not applied before traffic check"
}

build_client_configs() {
  mkdir -p "$TMP_DIR/configs"
  collect_client_config_preferred_ports
  python3 "${SCRIPT_DIR}/real-access-inbound-matrix-client-configs.py" \
    build "$TMP_DIR/subscription.yaml" "$TMP_DIR/configs" "$CLIENT_PORT_BASE" \
    "${CLIENT_CONFIG_PREFERRED_PORT_ARGS[@]}" "${protocol_names[@]}"
}

collect_client_config_preferred_ports() {
  local output_file line protocol port

  CLIENT_CONFIG_PREFERRED_PORT_ARGS=()
  output_file="$TMP_DIR/preferred-client-ports"
  : >"$output_file"

  if [[ -n "${DATABASE_URL:-}" && -n "${ACCESS_LINE_ID:-}" ]]; then
    xrayc_real_e2e_psql_database_url "$DATABASE_URL" \
      -XAtq \
      -v ON_ERROR_STOP=1 \
      -v "access_line_id=${ACCESS_LINE_ID}" >>"$output_file" 2>/dev/null <<'SQL' || true
SELECT CASE lower(protocol::text)
         WHEN 'ss' THEN 'shadowsocks'
         ELSE lower(protocol::text)
       END || '=' || listen_port::text
FROM access_lines
WHERE id = :'access_line_id'::uuid
LIMIT 1;
SQL
  fi

  if [[ -f "$TEMP_PREP_MANIFEST" ]]; then
    awk -F= '
      /^PORT_[A-Z_]+=/ {
        protocol = tolower(substr($1, 6));
        if (protocol == "ss") {
          protocol = "shadowsocks";
        }
        print protocol "=" $2;
      }
    ' "$TEMP_PREP_MANIFEST" >>"$output_file"
  fi

  while IFS='=' read -r protocol port; do
    case "$protocol" in
      vless|trojan|shadowsocks) ;;
      *) continue ;;
    esac
    [[ "$port" =~ ^[0-9]+$ && "$port" -gt 0 && "$port" -lt 65536 ]] || continue
    CLIENT_CONFIG_PREFERRED_PORT_ARGS+=(--preferred-port "${protocol}=${port}")
  done <"$output_file"
}
