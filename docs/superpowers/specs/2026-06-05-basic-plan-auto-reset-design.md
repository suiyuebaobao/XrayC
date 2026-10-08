# Basic Plan Auto Reset Design

## Goal

让免费基础套餐到期后自动重置并继续可用；让付费套餐到期后自动切换回基础套餐。基础套餐按套餐配置周期重置，默认种子为 30 天月度口径。付费套餐重复购买继续沿用现有叠加语义：增加额度、延长有效期、不清零已用流量。

## Model

现有 `plans.is_default = TRUE` 就是基础套餐标识，不新增套餐策略字段。用户可用性仍只由 `user_subscriptions.active`、`expires_at`、`limit_bytes` 和 `used_bytes` 决定。

基础套餐到期时，Worker 维护任务将订阅重置为新的基础周期。当前实现使用 `default_plan.duration_days` 计算下一周期；默认基础套餐配置为 30 天，不表达自然月 1 号统一重置：

```text
plan_id = default_plan.id
active = true
used_bytes = 0
limit_bytes = default_plan.traffic_limit_bytes
expires_at = now() + default_plan.duration_days
```

非基础套餐到期时，Worker 维护任务也切回基础套餐并执行同样重置。这样付费套餐到期后不会继续使用付费授权、限速或倍率。

## Boundaries

付费购买和兑换码逻辑不改。它们已经按现有规则执行：

```text
expires_at = max(old_expires_at, now) + paid_duration_days
limit_bytes = old_limit_bytes + paid_traffic_limit_bytes
used_bytes unchanged
```

管理员手动改用户套餐继续保留人工干预语义：切换到指定套餐并清零 `used_bytes`。

## Operational Effects

当维护任务重置或切换任何到期订阅时，必须清理用户运行分配并标记所有中转节点 dirty，确保额度耗尽或付费过期导致的旧运行态被下一轮 agent 配置刷新替换。

公开日志、文档和测试不得包含真实服务器、真实 token 或真实用户信息。
