-- 共用流量统计来源：明细与已归档小时互不重叠，历史节点身份来自不可变快照。
-- 不新增明细副本、不清理旧日志。小时边界精度由 first/last 明确表示。
CREATE VIEW usage_node_traffic_history AS
SELECT hs.access_node_id, ul.collected_at AS first_collected_at, ul.collected_at AS last_collected_at,
       ul.delta_uplink, ul.delta_downlink, FALSE AS archived
FROM usage_ledgers ul
JOIN usage_routing_snapshots hs ON hs.id=ul.routing_snapshot_id
WHERE ul.traffic_source='access_line'
UNION ALL
SELECT COALESCE(hs.access_node_id, r.access_node_id),
       COALESCE(r.first_collected_at, r.hour_start),
       COALESCE(r.last_collected_at, r.hour_start + interval '1 hour' - interval '1 microsecond'),
       r.delta_uplink, r.delta_downlink, TRUE
FROM usage_hourly_rollups r
JOIN usage_routing_snapshots hs ON hs.id=r.routing_snapshot_id
WHERE r.traffic_source='access_line';
