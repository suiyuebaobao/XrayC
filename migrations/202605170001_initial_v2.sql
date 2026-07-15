-- XrayC v2 initial PostgreSQL schema.
-- The schema follows the access-node control plane: users connect only to
-- access_lines while exit_endpoints remain private upstream resources.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    xray_user_key TEXT NOT NULL UNIQUE,
    disabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE plans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    traffic_limit_bytes BIGINT NOT NULL DEFAULT 0,
    billing_multiplier NUMERIC(10, 3) NOT NULL DEFAULT 1.000,
    visible_line_count INTEGER NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX plans_single_default ON plans (is_default) WHERE is_default = TRUE;

CREATE TABLE user_subscriptions (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    plan_id UUID NOT NULL REFERENCES plans(id),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_bytes BIGINT NOT NULL DEFAULT 0,
    limit_bytes BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE subscription_tokens (
    token TEXT PRIMARY KEY,
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE access_nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    public_host TEXT NOT NULL,
    agent_token_hash TEXT NOT NULL,
    config_dirty BOOLEAN NOT NULL DEFAULT TRUE,
    desired_config_hash TEXT NULL,
    applied_config_hash TEXT NULL,
    last_heartbeat_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TYPE endpoint_type AS ENUM ('direct', 'socks', 'http', 'vless', 'trojan', 'shadowsocks', 'hysteria');

CREATE TABLE exit_resources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    region_code TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE exit_endpoints (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exit_resource_id UUID NOT NULL REFERENCES exit_resources(id) ON DELETE CASCADE,
    outbound_type endpoint_type NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 0,
    outbound_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE exit_pools (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE exit_pool_members (
    exit_pool_id UUID NOT NULL REFERENCES exit_pools(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    weight INTEGER NOT NULL DEFAULT 100,
    status TEXT NOT NULL DEFAULT 'healthy',
    PRIMARY KEY (exit_pool_id, exit_endpoint_id)
);

CREATE TABLE access_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    exit_pool_id UUID NOT NULL REFERENCES exit_pools(id),
    listen_host TEXT NOT NULL,
    listen_port INTEGER NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'vless',
    transport TEXT NOT NULL DEFAULT 'tcp',
    user_uuid TEXT NOT NULL,
    server_name TEXT NOT NULL DEFAULT '',
    public_key TEXT NOT NULL DEFAULT '',
    short_id TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (access_node_id, listen_host, listen_port)
);

CREATE TABLE line_groups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE line_group_lines (
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    PRIMARY KEY (line_group_id, access_line_id)
);

CREATE TABLE plan_line_groups (
    plan_id UUID NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    visible_line_count INTEGER NULL,
    PRIMARY KEY (plan_id, line_group_id)
);

CREATE TABLE access_traffic_snapshots (
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    xray_user_key TEXT NOT NULL,
    uplink_total BIGINT NOT NULL,
    downlink_total BIGINT NOT NULL,
    collected_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (access_line_id, xray_user_key)
);

CREATE TABLE usage_ledgers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_line_id UUID NOT NULL REFERENCES access_lines(id),
    user_id UUID NOT NULL REFERENCES users(id),
    xray_user_key TEXT NOT NULL,
    traffic_source TEXT NOT NULL DEFAULT 'access_line',
    delta_uplink BIGINT NOT NULL,
    delta_downlink BIGINT NOT NULL,
    billing_multiplier NUMERIC(10, 3) NOT NULL DEFAULT 1.000,
    billed_bytes BIGINT NOT NULL,
    collected_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
