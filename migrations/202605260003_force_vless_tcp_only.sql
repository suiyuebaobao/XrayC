-- VLESS 入口统一为 TCP；历史 XHTTP/XUDP 字段只保留给非 VLESS 高级模式。
UPDATE access_lines
SET transport = 'tcp',
    udp_packet_encoding = '',
    xhttp_path = '',
    xhttp_host = '',
    xhttp_mode = 'auto'
WHERE lower(btrim(protocol)) = 'vless'
  AND (
      lower(btrim(transport)) <> 'tcp'
      OR lower(btrim(udp_packet_encoding)) = 'xudp'
      OR btrim(xhttp_path) <> ''
      OR btrim(xhttp_host) <> ''
      OR xhttp_mode <> 'auto'
  );

UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'force_vless_tcp_only'
WHERE id IN (
    SELECT DISTINCT access_node_id
    FROM access_lines
    WHERE lower(btrim(protocol)) = 'vless'
);
