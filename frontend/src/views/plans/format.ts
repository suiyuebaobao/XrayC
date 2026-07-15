// 本文件提供套餐分组页面专用的表单工厂和展示格式化函数。
// 它只处理套餐、分组和分组内绑定节点的前端展示文案。
// 这里不发起 API 请求，也不改变后端返回的数据结构。
// 组件和 composable 共用这些函数以减少 PlansPage 体积。
import type { PlanInfo } from '@/services/api';
import type { PlanFormDraft } from '@/views/plans/types';

export function emptyPlanForm(): PlanFormDraft {
  return {
    id: '',
    name: '',
    enabled: true,
    trafficLimitGb: 10,
    rateLimitMbps: 0,
    rateLimitUpMbps: 0,
    rateLimitDownMbps: 0,
    multiplier: 1,
    price: 0,
    currency: 'USDT',
    durationDays: 30,
    sortWeight: 100,
  };
}

export function formFromPlan(plan: PlanInfo): PlanFormDraft {
  return {
    id: plan.id,
    name: plan.name,
    enabled: plan.enabled,
    // 回填精度必须与 PlanFormDialog 的 el-input-number :precision="2" 一致，
    // 否则 toFixed(3) 与控件保留的 2 位之间产生差值，未改流量字段也会触发字节额度漂移。
    trafficLimitGb: Number(totalPlanTrafficGb(plan).toFixed(2)),
    rateLimitMbps: plan.rateLimitMbps,
    // 上下行直接生效（0=不限）。老套餐只有对称 rateLimitMbps、方向列为 null 时，
    // 回填为对称值，让表单显示真实生效速率、保存后转为显式上下行,不丢老数据。
    rateLimitUpMbps:
      plan.rateLimitUpBps != null
        ? Number((plan.rateLimitUpBps / 1_000_000).toFixed(3))
        : plan.rateLimitMbps,
    rateLimitDownMbps:
      plan.rateLimitDownBps != null
        ? Number((plan.rateLimitDownBps / 1_000_000).toFixed(3))
        : plan.rateLimitMbps,
    multiplier: plan.billingMultiplier,
    price: plan.priceCents / 100,
    currency: plan.currency,
    durationDays: plan.durationDays,
    sortWeight: plan.sortWeight,
  };
}

export function formatGb(value: number) {
  if (!Number.isFinite(value)) {
    return '0 GB';
  }

  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} GB`;
}

export function multiplierLabel(value: number) {
  return `${Number(value.toFixed(3))}x`;
}

export function totalPlanTrafficGb(plan: PlanInfo) {
  return plan.trafficLimitGb;
}

export function formatPrice(plan: PlanInfo) {
  const amount = plan.priceCents / 100;
  return `${plan.currency} ${Number(amount.toFixed(2))}`;
}

export function formatMbps(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return '不限速';
  }
  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} Mbps`;
}

export function shortId(id: string) {
  return id ? id.slice(0, 8) : '-';
}
