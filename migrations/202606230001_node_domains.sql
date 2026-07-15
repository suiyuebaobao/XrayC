-- node_domains:中转节点的多域名清单(直连/CF 各成列表)。
CREATE TABLE IF NOT EXISTS node_domains (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    domain         TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('direct','cf')),
    cf_cert_mode   TEXT,
    acme_email     TEXT,
    is_primary     BOOLEAN NOT NULL DEFAULT FALSE,
    cert_status    TEXT NOT NULL DEFAULT 'unknown',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (access_node_id, domain)
);
CREATE INDEX IF NOT EXISTS idx_node_domains_node ON node_domains(access_node_id);

-- 回填:旧单域名搬入,标 is_primary。
INSERT INTO node_domains (access_node_id, domain, kind, acme_email, is_primary)
SELECT id, cert_domain, 'direct', acme_email, TRUE
FROM access_nodes WHERE cert_domain IS NOT NULL AND cert_domain <> ''
ON CONFLICT (access_node_id, domain) DO NOTHING;

INSERT INTO node_domains (access_node_id, domain, kind, cf_cert_mode, acme_email, is_primary)
SELECT id, cf_domain, 'cf', cf_cert_mode, acme_email, TRUE
FROM access_nodes WHERE cf_domain IS NOT NULL AND cf_domain <> ''
ON CONFLICT (access_node_id, domain) DO NOTHING;

-- 入口 / 本机出口引用选中的域名(免证书留空)。
ALTER TABLE access_entries ADD COLUMN IF NOT EXISTS node_domain_id UUID
    REFERENCES node_domains(id) ON DELETE SET NULL;
ALTER TABLE exit_endpoints ADD COLUMN IF NOT EXISTS node_domain_id UUID
    REFERENCES node_domains(id) ON DELETE SET NULL;
