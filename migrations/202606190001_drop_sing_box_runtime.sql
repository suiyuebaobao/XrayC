-- 彻底移除 sing-box 内核遗留的运行内核字段与控制表。
-- 系统改为 xray 单内核，access_nodes/access_entries/access_lines 不再区分 runtime_core，
-- 节点级 xray/sing-box 启停控制表 access_node_runtime_cores 一并删除。
-- 对照 202606120002（core_type/runtime_core 列）与 202606120003（运行内核控制表）逐项回退。
-- 实时库已确认无 AnyTLS/sing_box 业务数据，直接删列删表不迁移数据。

-- 1. 删除节点级运行内核控制表（含其 CHECK 约束随表一并删除）。
DROP TABLE IF EXISTS access_node_runtime_cores;

-- 2. 删除 access_lines.runtime_core 列及其默认值/非空/CHECK 约束。
ALTER TABLE access_lines
    DROP CONSTRAINT IF EXISTS access_lines_runtime_core_check;

ALTER TABLE access_lines
    DROP COLUMN IF EXISTS runtime_core;

-- 3. 删除 access_entries.runtime_core 列及其默认值/非空/CHECK 约束。
ALTER TABLE access_entries
    DROP CONSTRAINT IF EXISTS access_entries_runtime_core_check;

ALTER TABLE access_entries
    DROP COLUMN IF EXISTS runtime_core;

-- 4. 删除 access_nodes.core_type 列及其默认值/非空/CHECK 约束。
ALTER TABLE access_nodes
    DROP CONSTRAINT IF EXISTS access_nodes_core_type_check;

ALTER TABLE access_nodes
    DROP COLUMN IF EXISTS core_type;
