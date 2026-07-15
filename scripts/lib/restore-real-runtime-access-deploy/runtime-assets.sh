#!/usr/bin/env bash

resolve_restore_runtime_assets() {
  local output_file="$1"
  local current_access_node_id current_access_line_id preferred_exit_endpoint_id

  current_access_node_id="$(uuid_or_empty "${ACCESS_NODE_ID:-}")"
  current_access_line_id="$(uuid_or_empty "${ACCESS_LINE_ID:-}")"
  preferred_exit_endpoint_id="$(uuid_or_empty "${restore_exit_endpoint_id:-}")"
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -Xq -v ON_ERROR_STOP=1 <<'SQL' >/dev/null
DO $$
DECLARE
  v_has_line BOOLEAN;
  v_node_id UUID;
  v_public_host TEXT;
  v_group_id UUID;
  v_endpoint_id UUID;
  v_pool_id UUID;
  v_line_id UUID;
  v_candidate INTEGER;
  v_port INTEGER;
BEGIN
  INSERT INTO access_entries (
    id, access_node_id, name, listen_host, listen_port, protocol, transport,
    security, user_uuid, server_name, public_key, short_id, flow, udp_enabled,
    udp_packet_encoding, ws_path, ws_host, xhttp_path, xhttp_host, xhttp_mode,
    cdn_enabled, cdn_provider, cdn_hostname, cdn_server, inbound_config,
    enabled, sort_weight
  )
  SELECT
    l.id, l.access_node_id, l.name, l.listen_host, l.listen_port, l.protocol, l.transport,
    COALESCE(NULLIF(l.inbound_config->>'security', ''), ''),
    l.user_uuid, l.server_name, l.public_key, l.short_id, l.flow, l.udp_enabled,
    l.udp_packet_encoding, '', '', l.xhttp_path, l.xhttp_host, l.xhttp_mode,
    FALSE, '', '', '', l.inbound_config,
    l.enabled, l.visibility_weight
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pools ep ON ep.id = l.exit_pool_id
  WHERE l.enabled = TRUE
    AND ep.enabled = TRUE
    AND NULLIF(n.applied_config_hash, '') IS NOT NULL
    AND (
      (
        n.config_dirty = FALSE
        AND NULLIF(n.desired_config_hash, '') IS NOT NULL
        AND n.applied_config_hash = n.desired_config_hash
      )
      OR n.config_dirty_reason = 'restore_real_runtime_access_deploy'
    )
    AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
    AND l.line_group_id IS NOT NULL
    AND l.exit_pool_id IS NOT NULL
    AND l.exit_endpoint_id IS NOT NULL
    AND COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) IS NOT NULL
    AND (
      COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) ~ '^[0-9]+(\.[0-9]+){3}$'
      OR lower(COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))) LIKE '%.sslip.io'
    )
    AND NOT EXISTS (
      SELECT 1
      FROM access_entries existing
      WHERE existing.access_node_id = l.access_node_id
        AND existing.listen_port = l.listen_port
        AND lower(existing.protocol) = lower(l.protocol)
        AND lower(existing.transport) = lower(l.transport)
        AND COALESCE(NULLIF(existing.ws_path, ''), NULLIF(existing.xhttp_path, ''), '') =
            COALESCE(NULLIF(l.xhttp_path, ''), '')
        AND existing.id <> l.id
    )
  ON CONFLICT (id) DO UPDATE SET
    access_node_id = EXCLUDED.access_node_id,
    name = EXCLUDED.name,
    listen_host = EXCLUDED.listen_host,
    listen_port = EXCLUDED.listen_port,
    protocol = EXCLUDED.protocol,
    transport = EXCLUDED.transport,
    security = EXCLUDED.security,
    user_uuid = EXCLUDED.user_uuid,
    server_name = EXCLUDED.server_name,
    public_key = EXCLUDED.public_key,
    short_id = EXCLUDED.short_id,
    flow = EXCLUDED.flow,
    udp_enabled = EXCLUDED.udp_enabled,
    udp_packet_encoding = EXCLUDED.udp_packet_encoding,
    xhttp_path = EXCLUDED.xhttp_path,
    xhttp_host = EXCLUDED.xhttp_host,
    xhttp_mode = EXCLUDED.xhttp_mode,
    inbound_config = EXCLUDED.inbound_config,
    enabled = EXCLUDED.enabled,
    sort_weight = EXCLUDED.sort_weight,
    updated_at = now();

  INSERT INTO access_entry_exit_bindings (
    id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight, remark
  )
  SELECT
    l.id, l.id, l.exit_endpoint_id, l.exit_pool_id, l.name, l.enabled, l.visibility_weight, 'restore runtime deploy'
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pools ep ON ep.id = l.exit_pool_id
  WHERE l.enabled = TRUE
    AND ep.enabled = TRUE
    AND NULLIF(n.applied_config_hash, '') IS NOT NULL
    AND (
      (
        n.config_dirty = FALSE
        AND NULLIF(n.desired_config_hash, '') IS NOT NULL
        AND n.applied_config_hash = n.desired_config_hash
      )
      OR n.config_dirty_reason = 'restore_real_runtime_access_deploy'
    )
    AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
    AND l.line_group_id IS NOT NULL
    AND l.exit_pool_id IS NOT NULL
    AND l.exit_endpoint_id IS NOT NULL
    AND (
      COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) ~ '^[0-9]+(\.[0-9]+){3}$'
      OR lower(COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))) LIKE '%.sslip.io'
    )
    AND EXISTS (
      SELECT 1
      FROM access_entries ae
      WHERE ae.id = l.id
    )
  ON CONFLICT (id) DO UPDATE SET
    access_entry_id = EXCLUDED.access_entry_id,
    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
    exit_pool_id = EXCLUDED.exit_pool_id,
    name = EXCLUDED.name,
    enabled = EXCLUDED.enabled,
    sort_weight = EXCLUDED.sort_weight,
    remark = EXCLUDED.remark,
    updated_at = now();

  INSERT INTO line_group_binding_nodes (line_group_id, entry_exit_binding_id, position)
  SELECT l.line_group_id, l.id, l.visibility_weight
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pools ep ON ep.id = l.exit_pool_id
  WHERE l.enabled = TRUE
    AND ep.enabled = TRUE
    AND NULLIF(n.applied_config_hash, '') IS NOT NULL
    AND (
      (
        n.config_dirty = FALSE
        AND NULLIF(n.desired_config_hash, '') IS NOT NULL
        AND n.applied_config_hash = n.desired_config_hash
      )
      OR n.config_dirty_reason = 'restore_real_runtime_access_deploy'
    )
    AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
    AND l.line_group_id IS NOT NULL
    AND l.exit_pool_id IS NOT NULL
    AND l.exit_endpoint_id IS NOT NULL
    AND (
      COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) ~ '^[0-9]+(\.[0-9]+){3}$'
      OR lower(COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))) LIKE '%.sslip.io'
    )
    AND EXISTS (
      SELECT 1
      FROM access_entry_exit_bindings aeb
      WHERE aeb.id = l.id
    )
  ON CONFLICT (line_group_id, entry_exit_binding_id) DO UPDATE SET
    position = EXCLUDED.position;

  SELECT EXISTS (
    SELECT 1
    FROM access_lines l
    JOIN access_nodes n ON n.id = l.access_node_id
    JOIN exit_pools ep ON ep.id = l.exit_pool_id
    WHERE l.enabled = TRUE
      AND ep.enabled = TRUE
      AND NULLIF(n.applied_config_hash, '') IS NOT NULL
      AND (
        (
          n.config_dirty = FALSE
          AND NULLIF(n.desired_config_hash, '') IS NOT NULL
          AND n.applied_config_hash = n.desired_config_hash
        )
        OR n.config_dirty_reason = 'restore_real_runtime_access_deploy'
      )
      AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
      AND COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) IS NOT NULL
      AND (
        COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) ~ '^[0-9]+(\.[0-9]+){3}$'
        OR lower(COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))) LIKE '%.sslip.io'
      )
      AND EXISTS (
        SELECT 1
        FROM access_entries ae
        JOIN access_entry_exit_bindings aeb
          ON aeb.access_entry_id = ae.id
         AND aeb.id = l.id
         AND aeb.enabled = TRUE
        JOIN line_group_binding_nodes lgbn
          ON lgbn.line_group_id = l.line_group_id
         AND lgbn.entry_exit_binding_id = aeb.id
        WHERE ae.id = l.id
          AND ae.access_node_id = l.access_node_id
          AND ae.listen_port = l.listen_port
          AND lower(ae.protocol) = lower(l.protocol)
          AND lower(ae.transport) = lower(l.transport)
          AND COALESCE(NULLIF(ae.ws_path, ''), NULLIF(ae.xhttp_path, ''), '') =
              COALESCE(NULLIF(l.xhttp_path, ''), '')
      )
  ) INTO v_has_line;

  IF v_has_line THEN
    RETURN;
  END IF;

  SELECT id, public_host
  INTO v_node_id, v_public_host
  FROM access_nodes
  WHERE trim(COALESCE(public_host, '')) <> ''
    AND lower(public_host) NOT LIKE '%example%'
    AND lower(public_host) NOT LIKE '%test%'
    AND lower(public_host) NOT LIKE '%placeholder%'
    AND public_host !~ '^(192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)'
    AND (
      public_host ~ '^[0-9]+(\.[0-9]+){3}$'
      OR lower(public_host) LIKE '%.sslip.io'
    )
    AND lower(COALESCE(NULLIF(btrim(status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
    AND NULLIF(applied_config_hash, '') IS NOT NULL
    AND (
      (
        config_dirty = FALSE
        AND NULLIF(desired_config_hash, '') IS NOT NULL
        AND applied_config_hash = desired_config_hash
      )
      OR config_dirty_reason = 'restore_real_runtime_access_deploy'
    )
    AND COALESCE(config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
  ORDER BY last_heartbeat_at DESC NULLS LAST, created_at DESC NULLS LAST
  LIMIT 1;

  SELECT plg.line_group_id
  INTO v_group_id
  FROM plan_line_groups plg
  JOIN line_groups lg ON lg.id = plg.line_group_id
  WHERE lg.enabled = TRUE
  ORDER BY lg.sort_weight, lg.created_at, lg.id::text
  LIMIT 1;

  SELECT e.id
  INTO v_endpoint_id
  FROM exit_endpoints e
  JOIN exit_resources r ON r.id = e.exit_resource_id
  WHERE e.enabled = TRUE
    AND r.enabled = TRUE
    AND trim(COALESCE(e.host, '')) <> ''
    AND e.port IS NOT NULL
    AND e.port > 0
    AND lower(e.host) NOT LIKE '%example%'
    AND lower(e.host) NOT LIKE '%test%'
    AND lower(e.host) NOT LIKE '%placeholder%'
    AND e.host !~ '^(192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)'
    AND e.outbound_type IN ('socks', 'http', 'vless', 'trojan', 'shadowsocks', 'hysteria')
  ORDER BY
    CASE WHEN e.outbound_type = 'socks' THEN 0 ELSE 1 END,
    e.created_at DESC NULLS LAST,
    e.id::text
  LIMIT 1;

  IF v_node_id IS NULL OR v_group_id IS NULL OR v_endpoint_id IS NULL THEN
    RETURN;
  END IF;

  v_port := NULL;
  FOR v_candidate IN 0..2000 LOOP
    v_port := 35443 + v_candidate;
    EXIT WHEN NOT EXISTS (
      SELECT 1
      FROM access_lines l
      WHERE l.access_node_id = v_node_id
        AND l.listen_port = v_port
        AND lower(l.protocol) = 'shadowsocks'
        AND lower(l.transport) = 'tcp'
    ) AND NOT EXISTS (
      SELECT 1
      FROM access_entries ae
      WHERE ae.access_node_id = v_node_id
        AND ae.listen_port = v_port
        AND lower(ae.protocol) = 'shadowsocks'
        AND lower(ae.transport) = 'tcp'
        AND COALESCE(NULLIF(ae.ws_path, ''), NULLIF(ae.xhttp_path, ''), '') = ''
    );
    v_port := NULL;
  END LOOP;
  IF v_port IS NULL THEN
    RAISE EXCEPTION 'no restore listen port is available';
  END IF;

  SELECT exit_pool_id INTO v_pool_id FROM line_groups WHERE id = v_group_id;
  IF v_pool_id IS NULL THEN
    INSERT INTO exit_pools (name, region_code, strategy, enabled)
    VALUES ('real-runtime-restore-pool', 'E2E', 'priority', TRUE)
    RETURNING id INTO v_pool_id;
    UPDATE line_groups SET exit_pool_id = v_pool_id WHERE id = v_group_id;
  END IF;

  INSERT INTO exit_pool_members (
    exit_pool_id, exit_endpoint_id, weight, status, priority, allow_new_assignments
  )
  VALUES (v_pool_id, v_endpoint_id, 100, 'healthy', 1000, TRUE)
  ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
    weight = EXCLUDED.weight,
    status = EXCLUDED.status,
    priority = EXCLUDED.priority,
    allow_new_assignments = EXCLUDED.allow_new_assignments;

  INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
  VALUES (v_group_id, v_endpoint_id)
  ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING;

  INSERT INTO access_lines (
    name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port,
    protocol, transport, user_uuid, server_name, public_key, short_id, enabled,
    inbound_config, udp_enabled, visibility_weight, udp_packet_encoding
  )
  VALUES (
    '真实恢复入口', v_node_id, v_group_id, v_endpoint_id, v_pool_id, v_public_host, v_port,
    'shadowsocks', 'tcp', gen_random_uuid()::text, '', '', '', TRUE,
    jsonb_build_object('method', '2022-blake3-aes-128-gcm', 'password', 'MDEyMzQ1Njc4OWFiY2RlZg=='),
    TRUE, 100000, ''
  )
  RETURNING id INTO v_line_id;

  INSERT INTO access_entries (
    id, access_node_id, name, listen_host, listen_port, protocol, transport,
    security, user_uuid, server_name, public_key, short_id, flow, udp_enabled,
    udp_packet_encoding, ws_path, ws_host, xhttp_path, xhttp_host, xhttp_mode,
    cdn_enabled, cdn_provider, cdn_hostname, cdn_server, inbound_config,
    enabled, sort_weight
  )
  VALUES (
    v_line_id, v_node_id, '真实恢复入口', v_public_host, v_port, 'shadowsocks', 'tcp',
    '', gen_random_uuid()::text, '', '', '', '', TRUE,
    '', '', '', '', '', 'stream-one',
    FALSE, '', '', '', jsonb_build_object('method', '2022-blake3-aes-128-gcm', 'password', 'MDEyMzQ1Njc4OWFiY2RlZg=='),
    TRUE, 100000
  )
  ON CONFLICT (id) DO UPDATE SET
    access_node_id = EXCLUDED.access_node_id,
    name = EXCLUDED.name,
    listen_host = EXCLUDED.listen_host,
    listen_port = EXCLUDED.listen_port,
    protocol = EXCLUDED.protocol,
    transport = EXCLUDED.transport,
    inbound_config = EXCLUDED.inbound_config,
    enabled = EXCLUDED.enabled,
    sort_weight = EXCLUDED.sort_weight,
    updated_at = now();

  INSERT INTO access_entry_exit_bindings (
    id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight, remark
  )
  VALUES (
    v_line_id, v_line_id, v_endpoint_id, v_pool_id, '真实恢复入口', TRUE, 100000, 'restore runtime deploy'
  )
  ON CONFLICT (id) DO UPDATE SET
    access_entry_id = EXCLUDED.access_entry_id,
    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
    exit_pool_id = EXCLUDED.exit_pool_id,
    name = EXCLUDED.name,
    enabled = EXCLUDED.enabled,
    sort_weight = EXCLUDED.sort_weight,
    remark = EXCLUDED.remark,
    updated_at = now();

  INSERT INTO line_group_binding_nodes (line_group_id, entry_exit_binding_id, position)
  VALUES (v_group_id, v_line_id, 100000)
  ON CONFLICT (line_group_id, entry_exit_binding_id) DO UPDATE SET
    position = EXCLUDED.position;
END $$;
SQL
  xrayc_real_e2e_psql_database_url "$DATABASE_URL" -XAtq -F $'\t' -v ON_ERROR_STOP=1 \
    -v current_access_node_id="$current_access_node_id" \
    -v current_access_line_id="$current_access_line_id" \
    -v preferred_exit_endpoint_id="$preferred_exit_endpoint_id" <<'SQL' >"$output_file"
WITH input AS (
  SELECT
    NULLIF(:'current_access_node_id', '')::uuid AS access_node_id,
    NULLIF(:'current_access_line_id', '')::uuid AS access_line_id,
    NULLIF(:'preferred_exit_endpoint_id', '')::uuid AS exit_endpoint_id
), active_lines AS (
  SELECT l.id AS access_line_id,
         l.access_node_id,
         l.exit_endpoint_id,
         l.exit_pool_id,
         l.line_group_id,
         CASE
           WHEN NULLIF(n.public_host, '') IS NOT NULL
            AND (
              NULLIF(l.listen_host, '') IS NULL
              OR lower(l.listen_host) LIKE '%example%'
              OR lower(l.listen_host) LIKE '%test%'
              OR lower(l.listen_host) LIKE '%placeholder%'
            )
             THEN n.public_host
           ELSE COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))
         END AS public_host,
         l.listen_port,
         l.protocol
  FROM access_lines l
  JOIN access_nodes n ON n.id = l.access_node_id
  JOIN exit_pools ep ON ep.id = l.exit_pool_id
  WHERE l.enabled = TRUE
    AND ep.enabled = TRUE
    AND NULLIF(n.applied_config_hash, '') IS NOT NULL
    AND (
      (
        n.config_dirty = FALSE
        AND NULLIF(n.desired_config_hash, '') IS NOT NULL
        AND n.applied_config_hash = n.desired_config_hash
      )
      OR n.config_dirty_reason = 'restore_real_runtime_access_deploy'
    )
    AND COALESCE(n.config_dirty_reason, '') NOT LIKE 'limiter command failed:%'
    AND COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) IS NOT NULL
    AND (
      COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, '')) ~ '^[0-9]+(\.[0-9]+){3}$'
      OR lower(COALESCE(NULLIF(l.listen_host, ''), NULLIF(n.public_host, ''))) LIKE '%.sslip.io'
    )
    AND lower(COALESCE(NULLIF(btrim(n.status), ''), 'unknown')) NOT IN ('disabled', 'offline', 'unavailable')
    AND lower(l.protocol) IN ('vless', 'trojan', 'shadowsocks', 'ss')
    AND EXISTS (
      SELECT 1
      FROM access_entries ae
      JOIN access_entry_exit_bindings aeb
        ON aeb.access_entry_id = ae.id
       AND aeb.id = l.id
       AND aeb.enabled = TRUE
      JOIN line_group_binding_nodes lgbn
        ON lgbn.line_group_id = l.line_group_id
       AND lgbn.entry_exit_binding_id = aeb.id
      WHERE ae.id = l.id
        AND ae.access_node_id = l.access_node_id
        AND ae.listen_port = l.listen_port
        AND lower(ae.protocol) = lower(l.protocol)
        AND lower(ae.transport) = lower(l.transport)
        AND COALESCE(NULLIF(ae.ws_path, ''), NULLIF(ae.xhttp_path, ''), '') =
            COALESCE(NULLIF(l.xhttp_path, ''), '')
    )
), chosen_line AS (
  SELECT l.*
  FROM active_lines l
  CROSS JOIN input i
  ORDER BY
    CASE
      WHEN i.access_line_id IS NOT NULL
       AND l.access_line_id = i.access_line_id
        THEN 0
      WHEN i.access_node_id IS NOT NULL
       AND l.access_node_id = i.access_node_id
        THEN 1
      ELSE 2
    END,
    l.access_line_id::text
  LIMIT 1
), active_endpoints AS (
  SELECT e.id AS exit_endpoint_id,
         e.outbound_type,
         e.created_at
  FROM exit_endpoints e
  JOIN exit_resources r ON r.id = e.exit_resource_id
  WHERE e.enabled = TRUE
    AND r.enabled = TRUE
    AND trim(COALESCE(e.host, '')) <> ''
    AND e.port IS NOT NULL
    AND e.port > 0
    AND lower(e.host) NOT LIKE '%example%'
    AND lower(e.host) NOT LIKE '%test%'
    AND lower(e.host) NOT LIKE '%placeholder%'
    AND e.host !~ '^(192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)'
    AND e.outbound_type IN ('socks', 'http', 'vless', 'trojan', 'shadowsocks', 'hysteria')
), chosen_endpoint AS (
  SELECT e.exit_endpoint_id,
         e.outbound_type
  FROM active_endpoints e
  CROSS JOIN input i
  CROSS JOIN chosen_line l
  LEFT JOIN exit_pool_members epm
    ON epm.exit_pool_id = l.exit_pool_id
   AND epm.exit_endpoint_id = e.exit_endpoint_id
  LEFT JOIN line_group_exit_endpoints lgee
    ON lgee.line_group_id = l.line_group_id
   AND lgee.exit_endpoint_id = e.exit_endpoint_id
  WHERE (
    l.exit_endpoint_id IS NULL
    OR e.exit_endpoint_id = l.exit_endpoint_id
    OR NOT EXISTS (
      SELECT 1
      FROM active_endpoints existing
      WHERE existing.exit_endpoint_id = l.exit_endpoint_id
    )
  )
  ORDER BY
    CASE WHEN i.exit_endpoint_id IS NOT NULL AND e.exit_endpoint_id = i.exit_endpoint_id THEN 0 ELSE 1 END,
    CASE WHEN e.outbound_type = 'socks' THEN 0 ELSE 1 END,
    CASE WHEN epm.exit_endpoint_id IS NOT NULL THEN 0 ELSE 1 END,
    CASE WHEN lgee.exit_endpoint_id IS NOT NULL THEN 0 ELSE 1 END,
    e.created_at DESC NULLS LAST,
    e.exit_endpoint_id::text
  LIMIT 1
)
SELECT 'ACCESS_NODE_ID=' || l.access_node_id::text FROM chosen_line l
UNION ALL SELECT 'ACCESS_LINE_ID=' || l.access_line_id::text FROM chosen_line l
UNION ALL SELECT 'EXIT_ENDPOINT_ID=' || e.exit_endpoint_id::text FROM chosen_endpoint e
UNION ALL SELECT 'RESTORE_EXIT_ENDPOINT_PROTOCOL=' || e.outbound_type::text FROM chosen_endpoint e
UNION ALL SELECT 'EXPECTED_ACCESS_SERVERS=' || l.public_host || ':' || l.listen_port::text FROM chosen_line l;
SQL
}
