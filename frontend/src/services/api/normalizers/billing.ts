// 本文件负责套餐、订阅、订单、兑换码和用户用量的响应标准化。
// 它集中处理流量单位、金额字段、套餐分组和订阅地址兼容。
// 本文件不发起支付、不保存 token，只把后端响应转换成前端类型。
// 新增计费字段时，应同步维护这里的 snake_case/camelCase 映射。

import type {
  AdminOrderPage,
  AdminOrderQuery,
  AdminPlanPayload,
  AdminUser,
  AdminUserDeleteResult,
  AdminUserPage,
  AdminUserQuery,
  AdminUserSubscription,
  AdminUserTrafficLog,
  AdminUserTrafficLogPage,
  AdminUserTrafficLogQuery,
  OrderInfo,
  PlanInfo,
  PlanLineGroupBinding,
  RedeemCodeInfo,
  RedeemResult,
  SubscriptionInfo,
  SubscriptionSettings,
  UserUsageSummary,
} from '../types';
import {
  arrayValue,
  booleanValue,
  bytesToGb,
  collectionValue,
  gbToBytes,
  idValue,
  nullableBytesToGb,
  nullableNumberValue,
  numberValue,
  recordValue,
  stringArrayValue,
  stringValue,
} from '../primitives';
import { normalizeAccessLine } from './routing';

export function normalizeAdminUser(value: unknown): AdminUser {
  const data = recordValue(value);
  const subscription = recordValue(data.subscription ?? data.current_subscription);
  const account = stringValue(data.account ?? data.email ?? data.username);
  const active = booleanValue(data.subscriptionActive ?? data.subscription_active);
  const traffic = data.trafficGb
    ?? data.traffic_gb
    ?? subscription.traffic_gb
    ?? remainingTraffic(data.remainingGb ?? data.remaining_gb);
  const planRateLimitBps = numberValue(data.planRateLimitBps ?? data.plan_rate_limit_bps);
  const userRateLimitRaw = data.userRateLimitBps ?? data.user_rate_limit_bps;
  const userRateLimitBps = userRateLimitRaw === null || userRateLimitRaw === undefined
    ? null
    : numberValue(userRateLimitRaw);
  // 用户方向限速覆盖可空：null=该方向沿用套餐，Some(v)=用户级独立限速。
  const userRateLimitUpBps = nullableBps(data.userRateLimitUpBps ?? data.user_rate_limit_up_bps);
  const userRateLimitDownBps = nullableBps(data.userRateLimitDownBps ?? data.user_rate_limit_down_bps);
  const effectiveRateLimitBps = numberValue(data.effectiveRateLimitBps ?? data.effective_rate_limit_bps)
    || userRateLimitBps
    || planRateLimitBps;
  return {
    id: idValue(data.id ?? data.user_id),
    account,
    name: stringValue(data.name ?? data.display_name ?? data.nickname ?? data.email) || account,
    email: stringValue(data.email),
    role: data.role === 'admin' || data.is_admin === true ? 'admin' : 'user',
    status: normalizeUserStatus(data.status, data.disabled ?? data.is_disabled, data.locked ?? data.is_locked),
    planId: stringValue(data.planId ?? data.plan_id ?? subscription.planId ?? subscription.plan_id),
    planName: stringValue(data.planName ?? data.plan_name ?? subscription.planName ?? subscription.plan_name),
    subscriptionStatus: stringValue(data.subscriptionStatus ?? data.subscription_status ?? subscription.status)
      || (active ? 'active' : ''),
    subscriptionExpiresAt: stringValue(
      data.subscriptionExpiresAt ?? data.subscription_expires_at ?? subscription.expiresAt ?? subscription.expires_at,
    ),
    trafficGb: normalizeTraffic(traffic),
    planRateLimitBps,
    userRateLimitBps,
    userRateLimitUpBps,
    userRateLimitDownBps,
    effectiveRateLimitBps,
    effectiveRateLimitMbps: bpsToMbps(effectiveRateLimitBps),
    createdAt: stringValue(data.createdAt ?? data.created_at),
  };
}

export function normalizeAdminUserPage(value: unknown, fallback: AdminUserQuery = {}): AdminUserPage {
  const data = recordValue(value);
  const items = collectionValue(data, ['users', 'items', 'data']).map(normalizeAdminUser);
  return {
    items,
    total: numberValue(data.total) || items.length,
    page: numberValue(data.page) || fallback.page || 1,
    pageSize: numberValue(data.pageSize ?? data.page_size) || fallback.pageSize || 20,
  };
}

export function normalizeAdminUserDeleteResult(value: unknown): AdminUserDeleteResult {
  const data = recordValue(value);
  return {
    deleted: booleanValue(data.deleted),
    id: idValue(data.id ?? data.user_id),
    email: stringValue(data.email),
    deletedUsageLedgerCount: numberValue(data.deletedUsageLedgerCount ?? data.deleted_usage_ledger_count),
    deletedSnapshotCount: numberValue(data.deletedSnapshotCount ?? data.deleted_snapshot_count),
    deletedSessionEventCount: numberValue(data.deletedSessionEventCount ?? data.deleted_session_event_count),
  };
}

export function normalizeAdminUserSubscription(
  data: Record<string, unknown>,
  fallbackUserId: number | string,
  subscriptionDownloadPath: string,
): AdminUserSubscription {
  const token = stringValue(data.token ?? data.subscription_token);
  const subscriptionUrl = absoluteUrl(stringValue(data.subscriptionUrl ?? data.subscription_url ?? data.download_url));

  return {
    userId: idValue(data.userId ?? data.user_id ?? fallbackUserId),
    token,
    planName: stringValue(data.planName ?? data.plan_name),
    status: stringValue(data.status ?? data.subscription_status),
    expiresAt: stringValue(data.expiresAt ?? data.expires_at),
    trafficGb: normalizeTraffic(data.trafficGb ?? data.traffic_gb ?? data.traffic),
    subscriptionUrl: subscriptionUrl || (token ? absoluteSubscriptionUrl(token, subscriptionDownloadPath) : ''),
  };
}

export function normalizeAdminUserTrafficLogPage(
  value: unknown,
  query: AdminUserTrafficLogQuery = {},
): AdminUserTrafficLogPage {
  const data = recordValue(value);

  return {
    items: collectionValue(data, ['items', 'logs', 'data']).map(normalizeAdminUserTrafficLog),
    total: numberValue(data.total),
    page: numberValue(data.page) || query.page || 1,
    pageSize: numberValue(data.pageSize ?? data.page_size) || query.pageSize || 50,
  };
}

export function normalizeAdminUserTrafficLog(value: unknown): AdminUserTrafficLog {
  const data = recordValue(value);
  const accessLine = recordValue(data.accessLine ?? data.access_line);
  const accessNode = recordValue(data.accessNode ?? data.access_node);
  const exitEndpoint = recordValue(data.exitEndpoint ?? data.exit_endpoint);

  return {
    id: stringValue(data.id),
    userId: idValue(data.userId ?? data.user_id),
    xrayUserKey: stringValue(data.xrayUserKey ?? data.xray_user_key),
    trafficSource: stringValue(data.trafficSource ?? data.traffic_source),
    deltaUplink: numberValue(data.deltaUplink ?? data.delta_uplink),
    deltaDownlink: numberValue(data.deltaDownlink ?? data.delta_downlink),
    deltaTotal: numberValue(data.deltaTotal ?? data.delta_total),
    billedUplink: numberValue(data.billedUplink ?? data.billed_uplink),
    billedDownlink: numberValue(data.billedDownlink ?? data.billed_downlink),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
    billingMultiplier: numberValue(data.billingMultiplier ?? data.billing_multiplier) || 1,
    realGb: numberValue(data.realGb ?? data.real_gb),
    billedGb: numberValue(data.billedGb ?? data.billed_gb),
    collectedAt: stringValue(data.collectedAt ?? data.collected_at),
    recordedAt: stringValue(data.recordedAt ?? data.recorded_at),
    clientIp: stringValue(data.clientIp ?? data.client_ip),
    clientIpHash: stringValue(data.clientIpHash ?? data.client_ip_hash),
    ipObservedAt: stringValue(data.ipObservedAt ?? data.ip_observed_at),
    activeConnectionCount: numberValue(data.activeConnectionCount ?? data.active_connection_count),
    sessionStatus: stringValue(data.sessionStatus ?? data.session_status),
    accessLine: {
      id: stringValue(accessLine.id ?? data.access_line_id),
      name: stringValue(accessLine.name),
      protocol: stringValue(accessLine.protocol),
      transport: stringValue(accessLine.transport),
      listenHost: stringValue(accessLine.listenHost ?? accessLine.listen_host),
      listenPort: numberValue(accessLine.listenPort ?? accessLine.listen_port),
    },
    accessNode: {
      id: stringValue(accessNode.id),
      name: stringValue(accessNode.name),
      publicHost: stringValue(accessNode.publicHost ?? accessNode.public_host),
    },
    exitEndpoint: {
      id: stringValue(exitEndpoint.id),
      name: stringValue(exitEndpoint.name),
      outboundType: stringValue(exitEndpoint.outboundType ?? exitEndpoint.outbound_type),
      host: stringValue(exitEndpoint.host),
      port: numberValue(exitEndpoint.port),
      resourceName: stringValue(exitEndpoint.resourceName ?? exitEndpoint.resource_name),
    },
  };
}

export function normalizeOrder(value: unknown): OrderInfo {
  const data = recordValue(value);
  const user = recordValue(data.user);
  const plan = recordValue(data.plan);
  const amountCents = nullableNumberValue(data.amountCents ?? data.amount_cents);
  const amount = nullableNumberValue(data.amount ?? data.totalAmount ?? data.total_amount ?? data.payAmount ?? data.pay_amount);

  return {
    id: idValue(data.id ?? data.order_id),
    orderNo: stringValue(data.orderNo ?? data.order_no ?? data.number ?? data.id),
    account: stringValue(
      data.userEmail
        ?? data.user_email
        ?? data.account
        ?? data.userAccount
        ?? data.user_account
        ?? user.account
        ?? user.email,
    ),
    planName: stringValue(data.planName ?? data.plan_name ?? plan.name),
    amountCents,
    amount: amount ?? (amountCents === null ? null : amountCents / 100),
    currency: stringValue(data.currency) || 'CNY',
    status: stringValue(data.status) || 'unknown',
    paymentAddress: stringValue(data.paymentAddress ?? data.payment_address),
    expiresAt: stringValue(data.expiresAt ?? data.expires_at),
    durationDays: nullableNumberValue(data.durationDays ?? data.duration_days),
    createdAt: stringValue(data.createdAt ?? data.created_at),
    paidAt: stringValue(data.paidAt ?? data.paid_at),
    qrCode: stringValue(data.qrCode ?? data.qr_code) || undefined,
    payUrl: stringValue(data.payUrl ?? data.pay_url) || undefined,
    payChannel: stringValue(data.payChannel ?? data.pay_channel) || undefined,
    paymentError: stringValue(data.paymentError ?? data.payment_error) || undefined,
  };
}

export function normalizeAdminOrderPage(value: unknown, fallback: AdminOrderQuery): AdminOrderPage {
  const data = recordValue(value);
  return {
    items: collectionValue(data, ['orders', 'items', 'data']).map(normalizeOrder),
    total: numberValue(data.total),
    page: numberValue(data.page) || fallback.page || 1,
    pageSize: numberValue(data.pageSize ?? data.page_size) || fallback.pageSize || 50,
  };
}

export function normalizeRedeemCode(value: unknown): RedeemCodeInfo {
  const data = recordValue(value);
  const plan = recordValue(data.plan);
  const usedAt = stringValue(data.usedAt ?? data.used_at);
  const usedBy = stringValue(
    data.usedByEmail
      ?? data.used_by_email
      ?? data.usedBy
      ?? data.used_by
      ?? data.usedByAccount
      ?? data.used_by_account,
  );
  const rawStatus = stringValue(data.status).toLowerCase();
  const expiresAt = stringValue(data.expiresAt ?? data.expires_at);
  const expired = expiresAt ? new Date(expiresAt).getTime() < Date.now() : false;

  return {
    id: idValue(data.id ?? data.code),
    code: stringValue(data.code),
    planName: stringValue(data.planName ?? data.plan_name ?? plan.name),
    trafficGb: nullableBytesToGb(data.trafficBytes ?? data.traffic_bytes),
    durationDays: nullableNumberValue(data.durationDays ?? data.duration_days),
    status: rawStatus || (usedAt || usedBy ? 'used' : expired ? 'expired' : 'unused'),
    usedBy,
    usedAt,
    expiresAt,
    createdAt: stringValue(data.createdAt ?? data.created_at),
  };
}

export function normalizeRedeemResult(data: Record<string, unknown>): RedeemResult {
  return {
    message: stringValue(data.message) || '兑换成功',
    planName: stringValue(data.planName ?? data.plan_name),
    expiresAt: stringValue(data.expiresAt ?? data.expires_at),
  };
}

export function normalizeSubscriptionSettings(data: Record<string, unknown>): SubscriptionSettings {
  const mixedPort = data.mixedPort ?? data.mixed_port;
  const allowLan = data.allowLan ?? data.allow_lan;

  return {
    mixedPort: mixedPort === undefined ? 7890 : nullableNumberValue(mixedPort),
    allowLan: allowLan === undefined ? false : booleanValue(allowLan),
    mode: normalizeSubscriptionMode(data.mode),
    logLevel: stringValue(data.logLevel ?? data.log_level) || 'info',
    profileName: stringValue(data.profileName ?? data.profile_name ?? data.name) || 'XrayC',
    updateIntervalHours: numberValue(data.updateIntervalHours ?? data.update_interval_hours ?? data.interval_hours) || 24,
    defaultRules: stringArrayValue(data.defaultRules ?? data.default_rules ?? data.rules),
    autoTestEnabled: data.autoTestEnabled === undefined && data.auto_test_enabled === undefined
      ? false
      : booleanValue(data.autoTestEnabled ?? data.auto_test_enabled),
    autoTestName: stringValue(data.autoTestName ?? data.auto_test_name) || '自动选择',
    autoTestUrl: stringValue(data.autoTestUrl ?? data.auto_test_url) || 'http://cp.cloudflare.com/generate_204',
    autoTestIntervalSeconds: numberValue(
      data.autoTestIntervalSeconds ?? data.auto_test_interval_seconds,
    ) || 86400,
    blockUnhealthyLines: booleanValue(data.blockUnhealthyLines ?? data.block_unhealthy_lines),
  };
}

export function normalizePlan(value: unknown): PlanInfo {
  const data = recordValue(value);
  const rawTrafficLimitBytes = numberValue(data.trafficLimitBytes ?? data.traffic_limit_bytes);
  // 公共套餐来自内存模型(u64::MAX)，管理接口使用数据库 -1 标记。
  const trafficLimitBytes = rawTrafficLimitBytes === Number('18446744073709551615') ? -1 : rawTrafficLimitBytes;
  const rateLimitBps = numberValue(data.rateLimitBps ?? data.rate_limit_bps);
  // 方向限速可空：null=该方向沿用对称 rate_limit_bps，Some(v)=该方向独立限速。
  const rateLimitUpBps = nullableBps(data.rateLimitUpBps ?? data.rate_limit_up_bps);
  const rateLimitDownBps = nullableBps(data.rateLimitDownBps ?? data.rate_limit_down_bps);
  const priceCents = numberValue(data.priceCents ?? data.price_cents ?? data.amountCents ?? data.amount_cents);

  return {
    id: stringValue(data.id),
    name: stringValue(data.name) || '未命名套餐',
    isDefault: data.isDefault === true || data.is_default === true,
    enabled: data.enabled !== false,
    trafficLimitBytes,
    trafficLimitGb: trafficLimitBytes === -1 ? -1 : bytesToGb(trafficLimitBytes),
    rateLimitBps,
    rateLimitMbps: bpsToMbps(rateLimitBps),
    rateLimitUpBps,
    rateLimitDownBps,
    billingMultiplier: numberValue(data.billingMultiplier ?? data.billing_multiplier) || 1,
    priceCents,
    currency: stringValue(data.currency) || 'USDT',
    durationDays: numberValue(data.durationDays ?? data.duration_days) || 30,
    sortWeight: numberValue(data.sortWeight ?? data.sort_weight) || 100,
    isDeleted: data.isDeleted === true || data.is_deleted === true,
    lineGroups: normalizePlanLineGroups(
      data.lineGroups ?? data.line_groups,
      data.lineGroupIds ?? data.line_group_ids,
    ),
    defaultLineGroupId: stringValue(data.defaultLineGroupId ?? data.default_line_group_id),
  };
}

export function serializeAdminPlanPayload(payload: AdminPlanPayload) {
  // 用户未改流量字段时优先回传原始精确字节，避免 bytes→GB→bytes 往返把非整 GB 套餐悄悄改额度。
  const trafficLimitBytes = payload.trafficLimitGb === -1 ? -1 : Number.isFinite(payload.trafficLimitBytesExact)
    ? Math.round(Math.max(0, payload.trafficLimitBytesExact as number))
    : gbToBytes(payload.trafficLimitGb);
  return {
    name: payload.name.trim(),
    enabled: payload.enabled,
    traffic_limit_bytes: trafficLimitBytes,
    rate_limit_bps: mbpsToBps(payload.rateLimitMbps),
    // 套餐上下行直接生效:0=该方向不限,显式下发(不再回退对称)。
    rate_limit_up_bps: mbpsToBps(payload.rateLimitUpMbps),
    rate_limit_down_bps: mbpsToBps(payload.rateLimitDownMbps),
    billing_multiplier: payload.billingMultiplier,
    price_cents: Math.round(payload.price * 100),
    currency: payload.currency.trim().toUpperCase(),
    duration_days: payload.durationDays,
    sort_weight: payload.sortWeight,
  };
}

function bpsToMbps(value: number) {
  return Number((value / 1_000_000).toFixed(3));
}

function mbpsToBps(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }
  return Math.round(value * 1_000_000);
}

// 方向/用户限速可空字段：null/undefined 透传 null（沿用对称限速/套餐），其余归一为非负数。
function nullableBps(value: unknown): number | null {
  return value === null || value === undefined ? null : numberValue(value);
}

export function normalizeSubscription(
  data: Record<string, unknown>,
  subscriptionDownloadPath: string,
): SubscriptionInfo {
  const token = stringValue(data.token ?? data.subscription_token);
  const subscriptionUrl = absoluteUrl(stringValue(data.subscriptionUrl ?? data.subscription_url ?? data.download_url));

  return {
    token,
    planName: stringValue(data.planName ?? data.plan_name) || '当前套餐',
    expiresAt: stringValue(data.expiresAt ?? data.expires_at),
    trafficGb: normalizeTraffic(data.trafficGb ?? data.traffic_gb ?? data.traffic),
    subscriptionUrl: subscriptionUrl || (token ? absoluteSubscriptionUrl(token, subscriptionDownloadPath) : ''),
    visibleLines: arrayValue(data.visibleLines ?? data.visible_lines ?? data.access_lines).map((line) => {
      const normalized = normalizeAccessLine(line);
      return {
        id: normalized.id,
        name: stringValue(recordValue(line).name) || normalized.name,
        lineGroupId: stringValue(recordValue(line).lineGroupId ?? recordValue(line).line_group_id),
        lineGroupName: stringValue(recordValue(line).lineGroupName ?? recordValue(line).line_group_name),
        region: normalized.region,
        server: normalized.listenHost,
        port: normalized.listenPort,
        protocol: normalized.inboundProtocol,
      };
    }),
  };
}

export function normalizeTraffic(value: unknown) {
  const data = recordValue(value);

  return {
    used: numberValue(data.used ?? data.used_gb),
    total: numberValue(data.total ?? data.total_gb ?? data.limit_gb),
  };
}

export function normalizeUserUsageSummary(data: Record<string, unknown>): UserUsageSummary {
  return {
    realBytes: numberValue(data.realBytes ?? data.real_bytes),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
    realGb: numberValue(data.realGb ?? data.real_gb),
    billedGb: numberValue(data.billedGb ?? data.billed_gb),
  };
}

export function subscriptionSettingsPayload(payload: Partial<SubscriptionSettings>) {
  const fields = {
    mixed_port: payload.mixedPort,
    allow_lan: payload.allowLan,
    mode: payload.mode,
    log_level: payload.logLevel,
    profile_name: payload.profileName,
    update_interval_hours: payload.updateIntervalHours,
    default_rules: payload.defaultRules,
    rules: payload.defaultRules,
    auto_test_enabled: payload.autoTestEnabled,
    auto_test_name: payload.autoTestName,
    auto_test_url: payload.autoTestUrl,
    auto_test_interval_seconds: payload.autoTestIntervalSeconds,
    block_unhealthy_lines: payload.blockUnhealthyLines,
  };
  // 不提交未编辑的字段；false、0 与空规则数组仍是有效更新。
  return Object.fromEntries(Object.entries(fields).filter(([, value]) => value !== undefined));
}

function normalizePlanLineGroups(
  value: unknown,
  fallbackIds: unknown,
): PlanLineGroupBinding[] {
  const groups = arrayValue(value).map((item) => {
    const data = recordValue(item);

    return {
      lineGroupId: stringValue(data.lineGroupId ?? data.line_group_id ?? data.id),
      billingMultiplier: numberValue(data.billingMultiplier ?? data.billing_multiplier) || 1,
    };
  });

  const normalized = groups.filter((binding) => binding.lineGroupId);
  if (normalized.length > 0) {
    return normalized;
  }

  return arrayValue(fallbackIds)
    .map(stringValue)
    .filter(Boolean)
    .map((lineGroupId) => ({
      lineGroupId,
      billingMultiplier: 1,
    }));
}

function normalizeSubscriptionMode(value: unknown): 'rule' | 'global' {
  return stringValue(value).toLowerCase() === 'global' ? 'global' : 'rule';
}

function remainingTraffic(value: unknown) {
  const remaining = numberValue(value);
  if (remaining <= 0) {
    return {};
  }
  return {
    used_gb: 0,
    total_gb: remaining,
  };
}

function normalizeUserStatus(
  statusValue: unknown,
  disabledValue: unknown,
  lockedValue: unknown,
): AdminUser['status'] {
  const status = stringValue(statusValue).toLowerCase();
  if (status === 'active' || status === 'disabled' || status === 'locked' || status === 'pending') {
    return status;
  }

  if (booleanValue(disabledValue)) {
    return 'disabled';
  }

  if (booleanValue(lockedValue)) {
    return 'locked';
  }

  return status ? 'unknown' : 'active';
}

function absoluteSubscriptionUrl(token: string, subscriptionDownloadPath: string) {
  return `${window.location.origin}${subscriptionDownloadPath.replace('{token}', token)}`;
}

function absoluteUrl(value: string) {
  if (!value) {
    return '';
  }
  if (/^https?:\/\//i.test(value)) {
    return value;
  }
  return new URL(value, window.location.origin).toString();
}
