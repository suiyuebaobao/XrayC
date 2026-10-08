// 旧设置兼容与关闭开关的往返持久化，防止序列化把 false 误当成未设置。
import { describe, expect, it } from 'vitest';
import { normalizeSalesLanding, salesLandingPayload } from './sales';

describe('portal feature settings compatibility', () => {
  it('keeps existing entry points when old settings have no feature fields', () => {
    expect(normalizeSalesLanding({}).portalFeatures).toEqual({ plans: true, orders: true, redeem: true, invites: true, marketing: true });
  });
  it('preserves explicitly hidden entry points through a save and reload', () => {
    const stored = { portal_features: { plans: false, orders: false, redeem: true, invites: false, marketing: false } };
    const saved = salesLandingPayload(normalizeSalesLanding(stored));
    expect(saved.portal_features).toEqual(stored.portal_features);
    expect(normalizeSalesLanding(saved).portalFeatures).toEqual(stored.portal_features);
  });
});
