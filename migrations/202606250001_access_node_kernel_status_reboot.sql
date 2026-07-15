-- 节点内核能力状态 + 整机重启请求(§7.7.1)。
-- 内核状态:agent 自检 act_connmark 可加载性 + 是否已装新内核待重启,经心跳上报落库供面板显示。
-- 加列不破坏既有节点;kernel_connmark_available 默认 TRUE(旧节点不上报即按可用),
-- kernel_upgrade_pending 默认 FALSE(无待重启)。
ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS kernel_connmark_available BOOLEAN NOT NULL DEFAULT TRUE,
  ADD COLUMN IF NOT EXISTS kernel_upgrade_pending BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS kernel_status_last_report_at TIMESTAMPTZ;

-- 整机重启请求:管理员从面板触发,记一个待执行请求,经心跳命令通道下发给 agent。
-- api 侧不存 SSH 凭据、不直接 SSH;reboot 由 agent 带安全自检后执行(详见 §7.7.1)。
-- reboot_status∈{'','queued','running','success','failed'};reboot_request_id 为本次请求幂等键。
ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS reboot_request_id UUID,
  ADD COLUMN IF NOT EXISTS reboot_requested_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS reboot_completed_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS reboot_status TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS reboot_message TEXT NOT NULL DEFAULT '';

-- 待执行重启请求的检索索引:只覆盖已下请求但未完成的行,供心跳快速取出 reboot_task。
CREATE INDEX IF NOT EXISTS idx_access_nodes_reboot_pending
  ON access_nodes (reboot_requested_at DESC)
  WHERE reboot_request_id IS NOT NULL
    AND reboot_completed_at IS NULL;
