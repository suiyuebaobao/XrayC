#!/usr/bin/env bash
# 用途：拉取真实矩阵中继订阅并验证只暴露接入入口，不泄露上游出口材料。
# 依赖 common/api/assertions helper 已加载，使用主入口设置的临时目录和协议矩阵上下文。

assert_user_subscription_api_contains_relay_line() {
  local body_file="$1"
  local proxy_name="$2"
  local message="$3"

  python3 - "$body_file" "$proxy_name" <<'PY' || die "$message"
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)
proxy_name = sys.argv[2]
data = payload.get("data", payload) if isinstance(payload, dict) else payload
access_lines = data.get("access_lines") if isinstance(data, dict) else None
if not isinstance(access_lines, list):
    raise SystemExit(1)
matches = [
    line for line in access_lines
    if isinstance(line, dict) and proxy_name in str(line.get("name", "")).strip()
]
unique_names = {
    str(line.get("name", "")).strip()
    for line in matches
    if str(line.get("name", "")).strip()
}
if len(unique_names) != 1:
    raise SystemExit(1)
PY
}

assert_relay_subscription_eligibility() {
  local state_file="$TMP_DIR/relay-subscription-eligibility.tsv"
  local state=""
  local before_body="$TMP_DIR/subscription-before-download.json"

  psql_db -XAtq -F $'\t' -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" \
    -v group_id="$group_id" \
    -v access_line_id="$access_line_id" >"$state_file" <<'SQL'
WITH sub AS (
  SELECT s.plan_id,
         s.active,
         s.expires_at > now() AS not_expired,
         s.limit_bytes - s.used_bytes AS remaining_bytes
  FROM user_subscriptions s
  JOIN users u ON u.id = s.user_id
  WHERE s.user_id = :'user_id'::uuid
    AND u.disabled = FALSE
), eligible AS (
  SELECT l.id
  FROM access_lines l
  JOIN sub ON TRUE
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pools ep ON ep.id = l.exit_pool_id
  WHERE l.id = :'access_line_id'::uuid
    AND EXISTS (
      SELECT 1
      FROM plan_line_groups plg
      WHERE plg.plan_id = sub.plan_id
        AND plg.line_group_id = :'group_id'::uuid
    )
    AND EXISTS (
      SELECT 1
      FROM line_group_binding_nodes lgbn
      WHERE lgbn.line_group_id = :'group_id'::uuid
        AND lgbn.entry_exit_binding_id = l.id
    )
    AND l.enabled = TRUE
    AND sub.active = TRUE
    AND sub.not_expired = TRUE
    AND sub.remaining_bytes > 0
    AND lower(COALESCE(NULLIF(trim(n.status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
    AND ep.enabled = TRUE
    AND EXISTS (
      SELECT 1
      FROM exit_pool_members m
      JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
      JOIN exit_resources r ON r.id = e.exit_resource_id
      WHERE m.exit_pool_id = l.exit_pool_id
        AND (l.exit_endpoint_id IS NULL OR m.exit_endpoint_id = l.exit_endpoint_id)
        AND m.status = 'healthy'
        AND m.allow_new_assignments = TRUE
        AND e.enabled = TRUE
        AND r.enabled = TRUE
        AND COALESCE(NULLIF(lower(trim(r.status)), ''), 'healthy') <> 'offline'
        AND trim(e.host) <> ''
        AND e.port > 0
        AND (
          (
            e.outbound_type IN ('socks', 'http')
            AND (
              e.outbound_config = '{}'::jsonb
              OR (
                trim(COALESCE(e.outbound_config->>'username', e.outbound_config->>'user', '')) <> ''
                AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'pass', '')) <> ''
              )
            )
          )
          OR (
            e.outbound_type = 'vless'
            AND trim(COALESCE(e.outbound_config->>'uuid', e.outbound_config->>'id', '')) <> ''
          )
          OR (
            e.outbound_type = 'trojan'
            AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
          )
          OR (
            e.outbound_type = 'shadowsocks'
            AND trim(COALESCE(e.outbound_config->>'method', e.outbound_config->>'cipher', '')) <> ''
            AND trim(COALESCE(e.outbound_config->>'password', '')) <> ''
          )
          OR (
            e.outbound_type = 'hysteria'
            AND trim(COALESCE(e.outbound_config->>'password', e.outbound_config->>'auth', '')) <> ''
          )
        )
    )
)
SELECT CASE WHEN EXISTS (SELECT 1 FROM eligible) THEN 'ok' ELSE 'fail' END;
SQL
  state="$(tr -d '\r\n' <"$state_file")"
  [[ "$state" == "ok" ]] || die "relay subscription line is not eligible before download; details redacted"

  api_auth_get /api/user/subscription "$before_body" "$user_token"
  assert_user_subscription_api_contains_relay_line \
    "$before_body" \
    "$subscription_proxy_name" \
    "user subscription api must contain exactly one matching access line before subscription download"
}

fetch_and_assert_subscription_redaction() {
  local literal

  assert_relay_subscription_eligibility
  subscription_yaml="$TMP_DIR/subscription.yaml"
  xrayc_real_e2e_fetch_sensitive_url_to_file "${BASE_URL}/sub/${sub_token}" "$subscription_yaml" "subscription download failed"
  grep -Fq "$subscription_proxy_name" "$subscription_yaml" || die "subscription does not include relay line"
  grep -Fq "$PH2" "$subscription_yaml" || die "subscription does not include relay ingress"
  python3 - "$subscription_yaml" "$subscription_proxy_name" <<'PY' || die "subscription must contain exactly one matching relay proxy"
import sys

import yaml

subscription_file, proxy_name = sys.argv[1:3]
with open(subscription_file, "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)
proxies = payload.get("proxies") if isinstance(payload, dict) else None
if not isinstance(proxies, list):
    raise SystemExit(1)
matches = [
    proxy for proxy in proxies
    if isinstance(proxy, dict) and proxy_name in str(proxy.get("name", "")).strip()
]
if len(matches) != 1:
    raise SystemExit(1)
PY
  python3 - "$subscription_yaml" "$endpoint_hosts_file" "$PH2" "$LISTEN_PORT" "$subscription_proxy_name" <<'PY' || die "subscription leaked upstream endpoint material"
import sys

import yaml

subscription_file, endpoints_file, ingress_host, ingress_port, proxy_name = sys.argv[1:6]
with open(subscription_file, "r", encoding="utf-8") as fh:
    payload = yaml.safe_load(fh)
proxies = payload.get("proxies") if isinstance(payload, dict) else None
if not isinstance(proxies, list):
    raise SystemExit(1)
pairs = []
with open(endpoints_file, "r", encoding="utf-8") as fh:
    for line in fh:
        host, _, port = line.rstrip("\n").partition("\t")
        if host and port:
            pairs.append((host, port))
for proxy in proxies:
    if not isinstance(proxy, dict):
        continue
    if proxy_name not in str(proxy.get("name", "")).strip():
        continue
    server = str(proxy.get("server", "")).strip()
    port = str(proxy.get("port", "")).strip()
    for host, endpoint_port in pairs:
        if server == ingress_host and port == ingress_port:
            continue
        if server == host or (server and port and f"{server}:{port}" == f"{host}:{endpoint_port}"):
            raise SystemExit(1)
PY
  printf '%s\n' "${endpoint_sensitive_literals[@]}" > "$TMP_DIR/endpoint-sensitive.txt"
  assert_named_proxy_missing_literals "$subscription_yaml" "$TMP_DIR/endpoint-sensitive.txt" "$subscription_proxy_name" "subscription leaked upstream endpoint secret material" "$PH2" "$LISTEN_PORT"
  assert_private_matrix_material_redacted "$subscription_yaml" "$subscription_proxy_name"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_yaml"

  subscription_after_body="$TMP_DIR/subscription-after.json"
  api_auth_get /api/user/subscription "$subscription_after_body" "$user_token"
  assert_user_subscription_api_contains_relay_line \
    "$subscription_after_body" \
    "$subscription_proxy_name" \
    "user subscription api must contain exactly one matching access line"
  for literal in "${endpoint_host_literals[@]}"; do
    xrayc_real_e2e_assert_subscription_missing_literal "$subscription_after_body" "$literal" "user subscription api leaked upstream endpoint material"
  done
  assert_named_proxy_missing_literals "$subscription_after_body" "$TMP_DIR/endpoint-sensitive.txt" "$subscription_proxy_name" "user subscription api leaked upstream endpoint secret material" "$PH2" "$LISTEN_PORT"
  assert_private_matrix_material_redacted "$subscription_after_body" "$subscription_proxy_name"
  xrayc_real_e2e_assert_subscription_no_sensitive_proxy_urls "$subscription_after_body"
}
