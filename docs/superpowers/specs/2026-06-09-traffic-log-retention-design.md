# Traffic Log Retention Design

## Goal

后台增加“详细流量日志保留天数”设置，默认保留 14 天明细。超过保留期的 `usage_ledgers` 明细不能无限增长，但运营统计仍要能看到历史总量、趋势和高低峰。

## Model

保留三层数据口径：

```text
usage_ledgers          = 最近 N 天详细流水，用于用户明细日志和短期排障
usage_daily_rollups    = 日汇总，用于长期总量、排行榜、趋势
usage_hourly_rollups   = 小时汇总，用于长期高低峰和小时窗口
```

`traffic_log_retention` 存在于运营设置 JSON：

```json
{
  "detail_retention_days": 14,
  "prune_enabled": true,
  "delete_batch_size": 5000
}
```

Worker 维护任务先把保留期外的 `usage_ledgers` 写入日/小时汇总表，再按批次删除对应明细。汇总和删除必须按 `ledger_id` 幂等执行，重复维护不能重复计费、不能重复汇总。

## Query Rules

用户详细流量日志只读保留期内的 `traffic_source = 'access_line'` 明细。运营中心、健康统计、线路排行榜、来源统计读取“保留明细 + 历史汇总”，并在同一天或同一小时出现重复窗口时再次聚合，避免图表重复点。

维护清理只影响详细日志存储体量，不修改 `user_subscriptions.used_bytes`，不改变套餐计费结果。

## Boundaries

本功能不新增用户计费字段，不改变订阅输出，不改变 Agent 上报协议。旧数据迁移只新增汇总表和索引。

公开文档只描述统一订阅、统一出口管理和 Caddy 部署口径，不恢复已下线的旧模型或旧操作入口。
