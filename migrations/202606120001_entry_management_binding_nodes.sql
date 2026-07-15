-- Entry management introduces first-class inbound entries and entry-exit binding nodes.
-- Existing access_lines stay as the compatibility runtime view during the migration.

CREATE TABLE IF NOT EXISTS access_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    listen_host TEXT NOT NULL DEFAULT '',
    listen_port INTEGER NOT NULL CHECK (listen_port BETWEEN 1 AND 65535),
    protocol TEXT NOT NULL DEFAULT 'vless',
    transport TEXT NOT NULL DEFAULT 'tcp',
    security TEXT NOT NULL DEFAULT '',
    user_uuid TEXT NOT NULL DEFAULT '',
    server_name TEXT NOT NULL DEFAULT '',
    public_key TEXT NOT NULL DEFAULT '',
    short_id TEXT NOT NULL DEFAULT '',
    flow TEXT NOT NULL DEFAULT '',
    udp_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    udp_packet_encoding TEXT NOT NULL DEFAULT '',
    ws_path TEXT NOT NULL DEFAULT '',
    ws_host TEXT NOT NULL DEFAULT '',
    xhttp_path TEXT NOT NULL DEFAULT '',
    xhttp_host TEXT NOT NULL DEFAULT '',
    xhttp_mode TEXT NOT NULL DEFAULT 'auto',
    cdn_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    cdn_provider TEXT NOT NULL DEFAULT '',
    cdn_hostname TEXT NOT NULL DEFAULT '',
    cdn_server TEXT NOT NULL DEFAULT '',
    inbound_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_weight INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS access_entries_node_port_transport_idx
    ON access_entries (
        access_node_id,
        listen_port,
        protocol,
        transport,
        (COALESCE(NULLIF(ws_path, ''), NULLIF(xhttp_path, ''), ''))
    );

CREATE TABLE IF NOT EXISTS access_entry_exit_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_entry_id UUID NOT NULL REFERENCES access_entries(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    exit_pool_id UUID REFERENCES exit_pools(id) ON DELETE SET NULL,
    name TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_weight INTEGER NOT NULL DEFAULT 100,
    remark TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS access_entry_exit_bindings_entry_idx
    ON access_entry_exit_bindings(access_entry_id);

CREATE INDEX IF NOT EXISTS access_entry_exit_bindings_exit_idx
    ON access_entry_exit_bindings(exit_endpoint_id);

CREATE TABLE IF NOT EXISTS line_group_binding_nodes (
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    entry_exit_binding_id UUID NOT NULL REFERENCES access_entry_exit_bindings(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 100,
    PRIMARY KEY (line_group_id, entry_exit_binding_id)
);

INSERT INTO access_entries (
    id, access_node_id, name, listen_host, listen_port, protocol, transport, security,
    user_uuid, server_name, public_key, short_id, flow, udp_enabled, udp_packet_encoding,
    ws_path, ws_host, xhttp_path, xhttp_host, xhttp_mode, inbound_config, enabled, sort_weight
)
SELECT
    al.id,
    al.access_node_id,
    al.name,
    al.listen_host,
    al.listen_port,
    al.protocol,
    al.transport,
    COALESCE(NULLIF(al.inbound_config->>'security', ''), ''),
    al.user_uuid,
    al.server_name,
    al.public_key,
    al.short_id,
    al.flow,
    al.udp_enabled,
    al.udp_packet_encoding,
    CASE WHEN lower(al.transport) = 'ws' THEN al.xhttp_path ELSE '' END,
    CASE WHEN lower(al.transport) = 'ws' THEN al.xhttp_host ELSE '' END,
    al.xhttp_path,
    al.xhttp_host,
    al.xhttp_mode,
    al.inbound_config,
    al.enabled,
    100
FROM access_lines al
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
    ws_path = EXCLUDED.ws_path,
    ws_host = EXCLUDED.ws_host,
    xhttp_path = EXCLUDED.xhttp_path,
    xhttp_host = EXCLUDED.xhttp_host,
    xhttp_mode = EXCLUDED.xhttp_mode,
    inbound_config = EXCLUDED.inbound_config,
    enabled = EXCLUDED.enabled,
    updated_at = now();

INSERT INTO access_entry_exit_bindings (
    id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight
)
SELECT
    al.id,
    al.id,
    al.exit_endpoint_id,
    al.exit_pool_id,
    al.name,
    al.enabled,
    100
FROM access_lines al
WHERE al.exit_endpoint_id IS NOT NULL
ON CONFLICT (id) DO UPDATE SET
    access_entry_id = EXCLUDED.access_entry_id,
    exit_endpoint_id = EXCLUDED.exit_endpoint_id,
    exit_pool_id = EXCLUDED.exit_pool_id,
    name = EXCLUDED.name,
    enabled = EXCLUDED.enabled,
    updated_at = now();

INSERT INTO line_group_binding_nodes (line_group_id, entry_exit_binding_id, position)
SELECT DISTINCT
    lgee.line_group_id,
    b.id,
    100
FROM line_group_exit_endpoints lgee
JOIN access_entry_exit_bindings b ON b.exit_endpoint_id = lgee.exit_endpoint_id
ON CONFLICT (line_group_id, entry_exit_binding_id) DO NOTHING;
