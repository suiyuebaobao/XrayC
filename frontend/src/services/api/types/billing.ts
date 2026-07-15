// 本文件定义套餐、订阅、订单、兑换码和用户用量相关的前端 API 类型。
// 这些类型服务后台套餐授权、用户订阅页面和订单兑换流程。
// 文件只包含数据结构，不实现支付、不发起请求、不处理金额换算逻辑。
// 套餐授权只保留分组和倍率；历史数量策略只在后端兼容字段中存在。

export type PlanLineGroupBinding = {
  lineGroupId: string;
  billingMultiplier: number;
};

export type PlanInfo = {
  id: string;
  name: string;
  isDefault: boolean;
  enabled: boolean;
  trafficLimitBytes: number;
  trafficLimitGb: number;
  rateLimitBps: number;
  rateLimitMbps: number;
  rateLimitUpBps: number | null;
  rateLimitDownBps: number | null;
  billingMultiplier: number;
  priceCents: number;
  currency: string;
  durationDays: number;
  sortWeight: number;
  isDeleted: boolean;
  lineGroups: PlanLineGroupBinding[];
  defaultLineGroupId: string;
};

export type AdminPlanPayload = {
  name: string;
  enabled: boolean;
  trafficLimitGb: number;
  // 编辑时若用户未改流量字段，可带上原始精确字节额度，序列化时直接回传避免 GB 往返漂移。
  trafficLimitBytesExact?: number;
  rateLimitMbps: number;
  rateLimitUpMbps: number;
  rateLimitDownMbps: number;
  billingMultiplier: number;
  price: number;
  currency: string;
  durationDays: number;
  sortWeight: number;
};

export type SubscriptionInfo = {
  token: string;
  planName: string;
  expiresAt: string;
  trafficGb: {
    used: number;
    total: number;
  };
  subscriptionUrl: string;
  visibleLines: Array<{
    id: number;
    name: string;
    lineGroupId: string;
    lineGroupName: string;
    region: string;
    server: string;
    port: number;
    protocol: string;
  }>;
};

export type UserUsageSummary = {
  realBytes: number;
  billedBytes: number;
  realGb: number;
  billedGb: number;
};

export type AdminUser = {
  id: number | string;
  account: string;
  name: string;
  email: string;
  role: 'admin' | 'user';
  status: 'active' | 'disabled' | 'locked' | 'pending' | 'unknown';
  planId: string;
  planName: string;
  subscriptionStatus: string;
  subscriptionExpiresAt: string;
  trafficGb: {
    used: number;
    total: number;
  };
  planRateLimitBps: number;
  userRateLimitBps: number | null;
  userRateLimitUpBps: number | null;
  userRateLimitDownBps: number | null;
  effectiveRateLimitBps: number;
  effectiveRateLimitMbps: number;
  createdAt: string;
};

export type AdminUserQuery = {
  page?: number;
  pageSize?: number;
  keyword?: string;
  email?: string;
  status?: string;
  role?: string;
  planId?: string;
};

export type AdminUserPage = {
  items: AdminUser[];
  total: number;
  page: number;
  pageSize: number;
};

export type AdminUserUpdatePayload = {
  email: string;
  status: 'active' | 'disabled';
  role: 'admin' | 'user';
  planId?: string;
  rateLimitBps?: number | null;
  rateLimitUpBps?: number | null;
  rateLimitDownBps?: number | null;
};

export type AdminUserCreatePayload = AdminUserUpdatePayload & {
  password: string;
};

export type AdminUserDeleteResult = {
  deleted: boolean;
  id: number | string;
  email: string;
  deletedUsageLedgerCount: number;
  deletedSnapshotCount: number;
  deletedSessionEventCount: number;
};

export type AdminUsersBatchDeleteResult = {
  deletedCount: number;
  items: AdminUserDeleteResult[];
};

export type AdminUserSubscription = {
  userId: number | string;
  token: string;
  planName: string;
  status: string;
  expiresAt: string;
  trafficGb: {
    used: number;
    total: number;
  };
  subscriptionUrl: string;
};

export type AdminUserTrafficLogQuery = {
  page?: number;
  pageSize?: number;
  from?: string;
  to?: string;
  accessLineId?: string;
  exitEndpointId?: string;
};

export type AdminUserTrafficLog = {
  id: string;
  userId: number | string;
  xrayUserKey: string;
  trafficSource: string;
  deltaUplink: number;
  deltaDownlink: number;
  deltaTotal: number;
  billedUplink: number;
  billedDownlink: number;
  billedBytes: number;
  billingMultiplier: number;
  realGb: number;
  billedGb: number;
  collectedAt: string;
  recordedAt: string;
  clientIp: string;
  clientIpHash: string;
  ipObservedAt: string;
  activeConnectionCount: number;
  sessionStatus: string;
  accessLine: {
    id: string;
    name: string;
    protocol: string;
    transport: string;
    listenHost: string;
    listenPort: number;
  };
  accessNode: {
    id: string;
    name: string;
    publicHost: string;
  };
  exitEndpoint: {
    id: string;
    name: string;
    outboundType: string;
    host: string;
    port: number;
    resourceName: string;
  };
};

export type AdminUserTrafficLogPage = {
  items: AdminUserTrafficLog[];
  total: number;
  page: number;
  pageSize: number;
};

export type AdminUserDeviceQuery = {
  page?: number;
  pageSize?: number;
};

export type AdminUserDeviceIp = {
  clientIp: string;
  clientIpHash: string;
  source: 'subscription_pull' | 'node_use' | 'both' | 'unknown';
  firstSeenAt: string;
  lastSeenAt: string;
  subscriptionPullCount: number;
  nodeUseCount: number;
  lastAccessLine: {
    id: string;
    name: string;
    protocol: string;
    transport: string;
  };
  lastAccessNode: {
    id: string;
    name: string;
    publicHost: string;
  };
  lastSessionStatus: string;
  lastActiveConnectionCount: number;
};

export type AdminUserDeviceIpPage = {
  items: AdminUserDeviceIp[];
  total: number;
  page: number;
  pageSize: number;
};

export type OrderInfo = {
  id: number | string;
  orderNo: string;
  account: string;
  planName: string;
  amountCents: number | null;
  amount: number | null;
  currency: string;
  status: string;
  paymentAddress: string;
  expiresAt: string;
  durationDays: number | null;
  createdAt: string;
  paidAt: string;
  qrCode?: string;
  payUrl?: string;
  payChannel?: string;
  paymentError?: string;
};

export type AdminOrderQuery = {
  page?: number;
  pageSize?: number;
  status?: string;
  user?: string;
  email?: string;
  keyword?: string;
  orderNo?: string;
};

export type AdminOrderPage = {
  items: OrderInfo[];
  total: number;
  page: number;
  pageSize: number;
};

export type CreateOrderPayload = {
  planId: string;
  channel?: string;
};

export type RedeemCodeInfo = {
  id: number | string;
  code: string;
  planName: string;
  trafficGb: number | null;
  durationDays: number | null;
  status: string;
  usedBy: string;
  usedAt: string;
  expiresAt: string;
  createdAt: string;
};

export type CreateRedeemCodesPayload = {
  planId: string;
  count: number;
  durationDays: number;
  expiresAt?: string;
};

export type RedeemPayload = {
  code: string;
};

export type RedeemResult = {
  message: string;
  planName: string;
  expiresAt: string;
};

export type SubscriptionSettings = {
  mixedPort: number | null;
  allowLan: boolean;
  mode: 'rule' | 'global';
  logLevel: string;
  profileName: string;
  updateIntervalHours: number;
  defaultRules: string[];
  autoTestEnabled: boolean;
  autoTestName: string;
  autoTestUrl: string;
  autoTestIntervalSeconds: number;
  blockUnhealthyLines: boolean;
};
