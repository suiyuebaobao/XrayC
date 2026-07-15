-- 监控中心·节点运行态指标时序表(append-only,非 upsert)。
-- access-agent 经心跳上报本机 CPU/内存/磁盘占用,逐条落库供监控中心展示与趋势。
-- 每条为一次采样快照,record 单条 INSERT,不覆盖历史;读模型用 DISTINCT ON 取每节点最新。
-- cpu_pct_milli 为 CPU 使用率千分比(0..100000,即 0%..100%);字节列均 >=0,写入侧钳制非法值。
-- access_node_id 外键 ON DELETE CASCADE:删节点自动清其全部运行态指标,不留悬挂时序行。
-- worker 保留清理按 collected_at 过期裁剪(同 access_line_metric_snapshots 口径)。
CREATE TABLE IF NOT EXISTS node_runtime_metrics (
    id BIGSERIAL PRIMARY KEY,
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    cpu_pct_milli INTEGER NOT NULL DEFAULT 0,
    mem_used_bytes BIGINT NOT NULL DEFAULT 0,
    mem_total_bytes BIGINT NOT NULL DEFAULT 0,
    disk_used_bytes BIGINT NOT NULL DEFAULT 0,
    disk_total_bytes BIGINT NOT NULL DEFAULT 0,
    collected_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 取每节点最新指标的检索索引:按 (节点, 采集时间倒序) 命中 DISTINCT ON 与裁剪扫描。
CREATE INDEX IF NOT EXISTS idx_node_runtime_metrics_node_collected_at
    ON node_runtime_metrics (access_node_id, collected_at DESC);
