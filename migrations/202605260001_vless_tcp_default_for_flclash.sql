-- FlClash 等移动客户端不兼容 VLESS XHTTP，历史默认入口统一切回 TCP。
ALTER TABLE access_lines
    ALTER COLUMN xhttp_mode SET DEFAULT 'auto';

WITH updated_lines AS (
    UPDATE access_lines
    SET transport = 'tcp',
        udp_packet_encoding = '',
        xhttp_path = '',
        xhttp_host = '',
        xhttp_mode = 'auto'
    WHERE lower(btrim(protocol)) = 'vless'
      AND lower(btrim(transport)) = 'xhttp'
    RETURNING access_node_id
)
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    config_dirty_reason = 'vless_tcp_default_for_flclash'
WHERE id IN (SELECT DISTINCT access_node_id FROM updated_lines);

UPDATE access_lines
SET xhttp_mode = 'auto'
WHERE lower(btrim(transport)) <> 'xhttp'
  AND xhttp_mode <> 'auto';
