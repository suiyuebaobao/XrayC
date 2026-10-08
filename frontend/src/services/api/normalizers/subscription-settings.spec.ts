// 规则与订阅设置分开提交：保存一个页面不能携带另一页的过期快照。
import { describe, expect, it } from "vitest";
import { subscriptionSettingsPayload } from "./billing";
describe("subscription settings field ownership", () => {
  it("changes only rules when saving the rule editor", () => {
    expect(subscriptionSettingsPayload({ defaultRules: [] })).toEqual({ default_rules: [], rules: [] });
  });
  it("preserves explicit false and zero without sending unrelated fields", () => {
    expect(subscriptionSettingsPayload({ allowLan: false, mixedPort: 0, autoTestEnabled: false })).toEqual({
      allow_lan: false,
      mixed_port: 0,
      auto_test_enabled: false,
    });
  });
  it("does not overwrite rules when saving subscription metadata", () => {
    expect(subscriptionSettingsPayload({ profileName: "Service", updateIntervalHours: 12 })).toEqual({
      profile_name: "Service",
      update_interval_hours: 12,
    });
  });
});
