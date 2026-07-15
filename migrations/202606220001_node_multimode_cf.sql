-- 节点多模式:直连证书域名 + CF 身份。全部可空/带默认,兼容现网 0 数据。
-- 加列不破坏既有纯直连/纯 IP 节点;CF 回源 MVP 复用直连灰云证书。
ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS cert_domain   TEXT,
  ADD COLUMN IF NOT EXISTS acme_email    TEXT,
  ADD COLUMN IF NOT EXISTS cf_enabled    BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS cf_domain     TEXT,
  ADD COLUMN IF NOT EXISTS cf_cert_mode  TEXT NOT NULL DEFAULT 'reuse_direct';

-- 约束:cf_cert_mode 仅允许已实现值(reuse_direct);Origin CA/DNS-01 为后续扩展。
ALTER TABLE access_nodes
  DROP CONSTRAINT IF EXISTS access_nodes_cf_cert_mode_check;
ALTER TABLE access_nodes
  ADD CONSTRAINT access_nodes_cf_cert_mode_check
  CHECK (cf_cert_mode IN ('reuse_direct'));
