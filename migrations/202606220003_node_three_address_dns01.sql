-- 节点 3 地址模型:新增 IP 直连地址列 + CF 证书模式放开到 DNS-01。
-- 加列不破坏既有节点;ip_direct_address 给 Reality/Shadowsocks 直连用,可空。
ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS ip_direct_address TEXT;

-- cf_cert_mode CHECK 放开到 ('reuse_direct','dns01'):先 DROP 旧约束再 ADD 新约束。
-- 历史:cf_domain 非空但 cf_cert_mode 仍 reuse_direct 的保留(无 token 兜底);
-- 新节点默认 dns01 由应用层在写入时按 cf_domain 决定。
ALTER TABLE access_nodes
  DROP CONSTRAINT IF EXISTS access_nodes_cf_cert_mode_check;
ALTER TABLE access_nodes
  ADD CONSTRAINT access_nodes_cf_cert_mode_check
  CHECK (cf_cert_mode IN ('reuse_direct', 'dns01'));
