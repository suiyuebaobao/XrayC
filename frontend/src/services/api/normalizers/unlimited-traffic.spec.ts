// 不限流量必须保留 -1 标记，编辑套餐不能把它悄悄改成零额度。
import { describe, expect, it } from 'vitest';
import { normalizePlan, normalizeTraffic, serializeAdminPlanPayload } from './billing';
import { formFromPlan, formatGb } from '@/views/plans/format';

describe('unlimited traffic', () => {
  it('round trips unlimited quota through the plan editor', () => {
    const plan = normalizePlan({ id: 'p', name: 'Unlimited', traffic_limit_bytes: -1 });
    const draft = formFromPlan(plan);
    expect(draft.trafficLimitGb).toBe(-1);
    expect(normalizePlan({ traffic_limit_bytes: Number('18446744073709551615') }).trafficLimitGb).toBe(-1);
    const payload = serializeAdminPlanPayload({
      name: draft.name, enabled: true, trafficLimitGb: draft.trafficLimitGb,
      trafficLimitBytesExact: -1, rateLimitMbps: 0, rateLimitUpMbps: 0,
      rateLimitDownMbps: 0, billingMultiplier: 1, price: 0, currency: 'CNY',
      durationDays: 365, sortWeight: 100,
    });
    expect(payload.traffic_limit_bytes).toBe(-1);
    expect(formatGb(plan.trafficLimitGb)).toBe('不限流量');
    expect(normalizeTraffic({ used_gb: 1234, total_gb: -1 })).toEqual({ used: 1234, total: -1 });
  });
  it('keeps zero quota distinct from unlimited', () => {
    expect(normalizePlan({ traffic_limit_bytes: 0 }).trafficLimitGb).toBe(0);
    expect(formatGb(0)).not.toBe('不限流量');
  });
});
