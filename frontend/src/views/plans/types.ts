// 本文件定义套餐分组页面拆分组件共享的本地类型。
// 这些类型只描述页面表单草稿和分组授权字段。
// 它不请求接口、不改写 API 类型，避免影响其它页面。
// 后续套餐页组件之间传参应优先复用这里的类型。

export type PlanLineGroupDraft = {
  lineGroupId: string;
  billingMultiplier: number;
};

export type PlanFormDraft = {
  id: string;
  name: string;
  enabled: boolean;
  trafficLimitGb: number;
  rateLimitMbps: number;
  rateLimitUpMbps: number;
  rateLimitDownMbps: number;
  multiplier: number;
  price: number;
  currency: string;
  durationDays: number;
  sortWeight: number;
};
