-- VLESS Reality must not default SNI to a transit IP; IP SNI breaks client certificate validation.
WITH reality_lines AS (
    SELECT
        id,
        access_node_id,
        inbound_config,
        COALESCE(
            NULLIF(inbound_config->>'dest', ''),
            NULLIF(inbound_config->>'reality_dest', ''),
            NULLIF(inbound_config->>'realityDest', '')
        ) AS raw_dest
    FROM access_lines
    WHERE protocol = 'vless'
      AND lower(COALESCE(inbound_config->>'security', '')) = 'reality'
      AND (
          server_name = ''
          OR server_name ~ '^[0-9]{1,3}(\.[0-9]{1,3}){3}$'
          OR COALESCE(inbound_config->>'server_name', inbound_config->>'serverName', inbound_config->>'sni', inbound_config->>'tls_server_name', '') ~ '^[0-9]{1,3}(\.[0-9]{1,3}){3}$'
      )
),
dest_hosts AS (
    SELECT
        id,
        access_node_id,
        CASE
            WHEN host_part ~ '^[A-Za-z0-9.-]+$'
                 AND host_part LIKE '%.%'
                 AND host_part !~ '^[0-9]{1,3}(\.[0-9]{1,3}){3}$'
            THEN lower(host_part)
            ELSE 'www.cloudflare.com'
        END AS fixed_server_name
    FROM (
        SELECT
            id,
            access_node_id,
            trim(split_part(regexp_replace(regexp_replace(regexp_replace(COALESCE(raw_dest, ''), '^[A-Za-z][A-Za-z0-9+.-]*://', ''), '[/?#].*$', ''), '^.*@', ''), ':', 1)) AS host_part
        FROM reality_lines
    ) parsed
),
updated_lines AS (
    UPDATE access_lines l
    SET server_name = h.fixed_server_name,
        inbound_config = jsonb_set(
            l.inbound_config - 'serverName' - 'sni' - 'tls_server_name',
            '{server_name}',
            to_jsonb(h.fixed_server_name),
            true
        )
    FROM dest_hosts h
    WHERE l.id = h.id
    RETURNING l.access_node_id
)
UPDATE access_nodes n
SET config_dirty = TRUE,
    desired_config_hash = NULL,
    config_dirty_at = now(),
    config_dirty_reason = 'migration_vless_reality_ip_sni'
WHERE n.id IN (SELECT access_node_id FROM updated_lines);
