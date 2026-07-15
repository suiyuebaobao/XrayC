-- Extend the v2 PostgreSQL schema toward the access-node control-plane MVP.
-- This migration keeps existing demo data compatible while adding missing
-- assignment, auth, probe, and operations tables required by the plan.

ALTER TABLE access_nodes
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'unknown',
    ADD COLUMN IF NOT EXISTS agent_version TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS last_traffic_report_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS last_traffic_success_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS config_dirty_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS config_dirty_reason TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS rate_limit_port_start INTEGER NULL,
    ADD COLUMN IF NOT EXISTS rate_limit_port_end INTEGER NULL;

UPDATE access_nodes
SET config_dirty_at = now(),
    config_dirty_reason = CASE WHEN config_dirty_reason = '' THEN 'seed_or_legacy' ELSE config_dirty_reason END
WHERE config_dirty = TRUE AND config_dirty_at IS NULL;

ALTER TABLE access_lines
    ADD COLUMN IF NOT EXISTS region_code TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS region_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS region_flag TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS flow TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS udp_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS xhttp_path TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS xhttp_host TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS xhttp_mode TEXT NOT NULL DEFAULT 'auto',
    ADD COLUMN IF NOT EXISTS identity_mode TEXT NOT NULL DEFAULT 'credential',
    ADD COLUMN IF NOT EXISTS user_key_source TEXT NOT NULL DEFAULT 'xray_email',
    ADD COLUMN IF NOT EXISTS inbound_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS visibility_weight INTEGER NOT NULL DEFAULT 100;

ALTER TABLE exit_resources
    ADD COLUMN IF NOT EXISTS provider_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS ownership TEXT NOT NULL DEFAULT 'third_party',
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'unknown',
    ADD COLUMN IF NOT EXISTS last_probe_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS last_probe_status TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS last_endpoint_error TEXT NOT NULL DEFAULT '';

ALTER TABLE exit_endpoints
    ADD COLUMN IF NOT EXISTS name TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS outbound_proxy_url_encrypted TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS stream_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS probe_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS last_probe_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS last_probe_status TEXT NOT NULL DEFAULT '';

ALTER TABLE exit_pools
    ADD COLUMN IF NOT EXISTS region_code TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS strategy TEXT NOT NULL DEFAULT 'weighted_sticky',
    ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

ALTER TABLE exit_pool_members
    ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 100,
    ADD COLUMN IF NOT EXISTS allow_new_assignments BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

ALTER TABLE access_traffic_snapshots
    ADD COLUMN IF NOT EXISTS access_node_id UUID NULL REFERENCES access_nodes(id) ON DELETE SET NULL;

ALTER TABLE usage_ledgers
    ADD COLUMN IF NOT EXISTS delta_total BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS billed_uplink BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS billed_downlink BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS recorded_at TIMESTAMPTZ NOT NULL DEFAULT now();

UPDATE usage_ledgers
SET delta_total = delta_uplink + delta_downlink,
    billed_downlink = CASE WHEN billed_bytes > 0 THEN billed_bytes ELSE billed_downlink END
WHERE delta_total = 0;

CREATE TABLE IF NOT EXISTS site_settings (
    setting_key TEXT PRIMARY KEY,
    setting_value JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS auth_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scene TEXT NOT NULL,
    target_hash TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS login_guard_states (
    guard_key TEXT PRIMARY KEY,
    failure_count INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS invite_codes (
    code TEXT PRIMARY KEY,
    inviter_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    inviter_email_snapshot TEXT NOT NULL DEFAULT '',
    used_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    used_by_email_snapshot TEXT NOT NULL DEFAULT '',
    is_used BOOLEAN NOT NULL DEFAULT FALSE,
    used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_access_line_assignments (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, line_group_id, access_line_id)
);

CREATE TABLE IF NOT EXISTS user_exit_assignments (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    exit_pool_id UUID NOT NULL REFERENCES exit_pools(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    failover_reason TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (user_id, access_line_id, exit_pool_id)
);

CREATE TABLE IF NOT EXISTS rate_limit_profiles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    upload_mbps NUMERIC(10, 3) NOT NULL DEFAULT 0,
    download_mbps NUMERIC(10, 3) NOT NULL DEFAULT 0,
    burst_mbps NUMERIC(10, 3) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_rate_limit_assignments (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    profile_id UUID NOT NULL REFERENCES rate_limit_profiles(id) ON DELETE RESTRICT,
    runtime_port INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, access_line_id),
    UNIQUE (access_line_id, runtime_port)
);

CREATE TABLE IF NOT EXISTS access_line_metric_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    online_users INTEGER NOT NULL DEFAULT 0,
    active_connections INTEGER NOT NULL DEFAULT 0,
    uplink_rate_bps BIGINT NOT NULL DEFAULT 0,
    downlink_rate_bps BIGINT NOT NULL DEFAULT 0,
    collected_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS access_user_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    xray_user_key TEXT NOT NULL,
    client_ip_hash TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS access_line_probes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_line_id UUID NOT NULL REFERENCES access_lines(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    latency_ms INTEGER NULL,
    error_summary TEXT NOT NULL DEFAULT '',
    probed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS access_exit_probes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    latency_ms INTEGER NULL,
    error_summary TEXT NOT NULL DEFAULT '',
    probed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_auth_challenges_expiry ON auth_challenges(scene, target_hash, expires_at);
CREATE INDEX IF NOT EXISTS idx_usage_ledgers_line_time ON usage_ledgers(access_line_id, collected_at);
CREATE INDEX IF NOT EXISTS idx_usage_ledgers_user_time ON usage_ledgers(user_id, collected_at);
CREATE INDEX IF NOT EXISTS idx_access_traffic_snapshots_node_line ON access_traffic_snapshots(access_node_id, access_line_id);
CREATE INDEX IF NOT EXISTS idx_access_line_metric_snapshots_line_time ON access_line_metric_snapshots(access_line_id, collected_at DESC);
CREATE INDEX IF NOT EXISTS idx_access_user_sessions_line_seen ON access_user_sessions(access_line_id, last_seen_at DESC);
CREATE INDEX IF NOT EXISTS idx_access_line_probes_latest ON access_line_probes(access_line_id, probed_at DESC);
CREATE INDEX IF NOT EXISTS idx_access_exit_probes_latest ON access_exit_probes(access_node_id, exit_endpoint_id, probed_at DESC);
