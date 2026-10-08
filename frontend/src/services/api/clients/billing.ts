// 本文件维护套餐、订阅、订单、兑换码和销售首页 API 方法。
// 它只组织计费相关请求，不实现真实支付流程，支付接口暂保留入口。
// 用户订阅链接生成统一使用 paths 中的订阅下载模板。
// 套餐授权和订阅配置变更都应从这里调用，避免页面散落请求逻辑。

import type {
  AdminOrderPage,
  AdminOrderQuery,
  AdminPlanPayload,
  AdminUser,
  AdminUserCreatePayload,
  AdminUserDeleteResult,
  AdminUserDeviceIpPage,
  AdminUserDeviceQuery,
  AdminUserPage,
  AdminUserQuery,
  AdminUserSubscription,
  AdminUserTrafficLogPage,
  AdminUserTrafficLogQuery,
  AdminUserUpdatePayload,
  AdminUsersBatchDeleteResult,
  CreateOrderPayload,
  CreateRedeemCodesPayload,
  OrderInfo,
  PlanInfo,
  PlanLineGroupBinding,
  RedeemCodeInfo,
  RedeemPayload,
  RedeemResult,
  SalesLandingConfig,
  SubscriptionSettings,
  UserUsageSummary,
} from '../types';
import { request } from '../http';
import { appendQueryParam, collectionValue, isRecord } from '../primitives';
import {
  normalizeAdminOrderPage,
  normalizeAdminUser,
  normalizeAdminUserDeleteResult,
  normalizeAdminUserPage,
  normalizeAdminUserSubscription,
  normalizeAdminUserTrafficLogPage,
  normalizeOrder,
  normalizePlan,
  normalizeRedeemCode,
  normalizeRedeemResult,
  normalizeSubscription,
  normalizeSubscriptionSettings,
  normalizeUserUsageSummary,
  serializeAdminPlanPayload,
  subscriptionSettingsPayload,
} from '../normalizers/billing';
import { normalizeAdminUserDeviceIpPage } from '../normalizers/userDevices';
import { normalizeSalesLanding, salesLandingPayload } from '../normalizers/sales';
import { plannedApiPaths, rustApiPaths } from '../paths';

export const billingApi = {
  async getPlans(): Promise<PlanInfo[]> {
    const data = await request<unknown>(rustApiPaths.plans);
    return collectionValue(isRecord(data) ? data : { items: data }, ['plans', 'items', 'data']).map(normalizePlan);
  },

  async getSalesLanding(): Promise<SalesLandingConfig> {
    const data = await request<unknown>(rustApiPaths.salesLanding);
    return normalizeSalesLanding(data);
  },

  async getAdminPlans(): Promise<PlanInfo[]> {
    const data = await request<unknown>(rustApiPaths.adminPlans);
    return collectionValue(data, ['plans', 'items', 'data']).map(normalizePlan);
  },

  async createAdminPlan(payload: AdminPlanPayload): Promise<PlanInfo> {
    const data = await request<unknown>(rustApiPaths.adminPlans, {
      method: 'POST',
      body: JSON.stringify(serializeAdminPlanPayload(payload)),
    });
    return normalizePlan(data);
  },

  async updateAdminPlan(planId: string, payload: AdminPlanPayload): Promise<PlanInfo> {
    const data = await request<unknown>(`${rustApiPaths.adminPlans}/${encodeURIComponent(planId)}`, {
      method: 'PATCH',
      body: JSON.stringify(serializeAdminPlanPayload(payload)),
    });
    return normalizePlan(data);
  },

  async deleteAdminPlan(planId: string) {
    await request<unknown>(`${rustApiPaths.adminPlans}/${encodeURIComponent(planId)}`, { method: 'DELETE' });
  },

  async replacePlanLineGroups(planId: string, lineGroups: PlanLineGroupBinding[], defaultLineGroupId?: string) {
    const path = rustApiPaths.adminPlanLineGroups.replace('{planId}', planId);
    await request<Record<string, unknown>>(path, {
      method: 'PUT',
      body: JSON.stringify({
        default_line_group_id: defaultLineGroupId || null,
        line_groups: lineGroups.map((binding) => ({
          line_group_id: binding.lineGroupId,
          billing_multiplier: binding.billingMultiplier,
        })),
      }),
    });
  },

  async getAdminSalesLanding(): Promise<SalesLandingConfig> {
    const data = await request<unknown>(rustApiPaths.adminSalesLanding);
    return normalizeSalesLanding(data);
  },

  async updateAdminSalesLanding(payload: SalesLandingConfig): Promise<SalesLandingConfig> {
    const data = await request<unknown>(rustApiPaths.adminSalesLanding, {
      method: 'PUT',
      body: JSON.stringify(salesLandingPayload(payload)),
    });
    return isRecord(data) ? normalizeSalesLanding(data) : payload;
  },

  async getSubscription() {
    const data = await request<Record<string, unknown>>(rustApiPaths.subscription);
    return normalizeSubscription(data, rustApiPaths.subscriptionDownload);
  },

  async getUserUsage(): Promise<UserUsageSummary> {
    const data = await request<Record<string, unknown>>(rustApiPaths.userUsage);
    return normalizeUserUsageSummary(data);
  },

  async resetSubscriptionToken() {
    const data = await request<Record<string, unknown>>(rustApiPaths.resetSubscriptionToken, { method: 'POST' });
    return normalizeSubscription(data, rustApiPaths.subscriptionDownload);
  },

  async getAdminUsers(query: AdminUserQuery = {}): Promise<AdminUser[]> {
    return (await billingApi.getAdminUsersPage(query)).items;
  },

  async getAdminUsersPage(query: AdminUserQuery = {}): Promise<AdminUserPage> {
    const params = new URLSearchParams();
    appendQueryParam(params, 'page', query.page);
    appendQueryParam(params, 'page_size', query.pageSize);
    appendQueryParam(params, 'keyword', query.keyword);
    appendQueryParam(params, 'email', query.email);
    appendQueryParam(params, 'status', query.status);
    appendQueryParam(params, 'role', query.role);
    appendQueryParam(params, 'plan_id', query.planId);
    const path = params.toString() ? `${plannedApiPaths.adminUsers}?${params.toString()}` : plannedApiPaths.adminUsers;
    const data = await request<unknown>(path);
    return normalizeAdminUserPage(data, query);
  },

  async getAdminUser(userId: number | string): Promise<AdminUser> {
    const path = `${plannedApiPaths.adminUsers}/${encodeURIComponent(String(userId))}`;
    const data = await request<unknown>(path);
    return normalizeAdminUser(data);
  },

  async createAdminUser(payload: AdminUserCreatePayload): Promise<AdminUser> {
    const body: Record<string, unknown> = {
      email: payload.email,
      password: payload.password,
      disabled: payload.status === 'disabled',
      is_admin: payload.role === 'admin',
    };
    if (payload.planId) {
      body.plan_id = payload.planId;
    }
    if (payload.rateLimitBps !== undefined) {
      body.rate_limit_bps = payload.rateLimitBps;
    }
    if (payload.rateLimitUpBps !== undefined) {
      body.rate_limit_up_bps = payload.rateLimitUpBps;
    }
    if (payload.rateLimitDownBps !== undefined) {
      body.rate_limit_down_bps = payload.rateLimitDownBps;
    }
    const data = await request<unknown>(plannedApiPaths.adminUsers, {
      method: 'POST',
      body: JSON.stringify(body),
    });
    return normalizeAdminUser(data);
  },

  async updateAdminUser(userId: number | string, payload: AdminUserUpdatePayload): Promise<AdminUser> {
    const path = `${plannedApiPaths.adminUsers}/${encodeURIComponent(String(userId))}`;
    const body: Record<string, unknown> = {
      email: payload.email,
      disabled: payload.status === 'disabled',
      is_admin: payload.role === 'admin',
    };
    if (payload.planId) {
      body.plan_id = payload.planId;
    }
    if (payload.rateLimitBps !== undefined) {
      body.rate_limit_bps = payload.rateLimitBps;
    }
    if (payload.rateLimitUpBps !== undefined) {
      body.rate_limit_up_bps = payload.rateLimitUpBps;
    }
    if (payload.rateLimitDownBps !== undefined) {
      body.rate_limit_down_bps = payload.rateLimitDownBps;
    }
    const data = await request<unknown>(path, {
      method: 'PATCH',
      body: JSON.stringify(body),
    });
    return normalizeAdminUser(data);
  },

  async deleteAdminUser(userId: number | string): Promise<AdminUserDeleteResult> {
    const path = `${plannedApiPaths.adminUsers}/${encodeURIComponent(String(userId))}`;
    const data = await request<unknown>(path, { method: 'DELETE' });
    return normalizeAdminUserDeleteResult(data);
  },

  async deleteAdminUsers(userIds: Array<number | string>): Promise<AdminUsersBatchDeleteResult> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminUsersBatchDelete, {
      method: 'POST',
      body: JSON.stringify({ user_ids: userIds }),
    });
    return {
      deletedCount: Number(data.deleted_count ?? data.deletedCount ?? 0),
      items: collectionValue(data, ['items']).map(normalizeAdminUserDeleteResult),
    };
  },

  async getAdminUserSubscription(userId: number | string): Promise<AdminUserSubscription> {
    const path = plannedApiPaths.adminUserSubscription.replace('{id}', encodeURIComponent(String(userId)));
    const data = await request<Record<string, unknown>>(path);
    return normalizeAdminUserSubscription(data, userId, rustApiPaths.subscriptionDownload);
  },

  async getAdminUserTrafficLogs(
    userId: number | string,
    query: AdminUserTrafficLogQuery = {},
  ): Promise<AdminUserTrafficLogPage> {
    const basePath = rustApiPaths.adminUserTrafficLogs.replace('{id}', encodeURIComponent(String(userId)));
    const params = new URLSearchParams();
    appendQueryParam(params, 'page', query.page);
    appendQueryParam(params, 'page_size', query.pageSize);
    appendQueryParam(params, 'from', query.from);
    appendQueryParam(params, 'to', query.to);
    appendQueryParam(params, 'access_line_id', query.accessLineId);
    appendQueryParam(params, 'exit_endpoint_id', query.exitEndpointId);
    const path = params.toString() ? `${basePath}?${params.toString()}` : basePath;
    const data = await request<unknown>(path);
    return normalizeAdminUserTrafficLogPage(data, query);
  },

  async getAdminUserDevices(
    userId: number | string,
    query: AdminUserDeviceQuery = {},
  ): Promise<AdminUserDeviceIpPage> {
    const basePath = rustApiPaths.adminUserDevices.replace('{id}', encodeURIComponent(String(userId)));
    const params = new URLSearchParams();
    appendQueryParam(params, 'page', query.page);
    appendQueryParam(params, 'page_size', query.pageSize);
    const path = params.toString() ? `${basePath}?${params.toString()}` : basePath;
    const data = await request<unknown>(path);
    return normalizeAdminUserDeviceIpPage(data, query);
  },

  async resetAdminUserSubscriptionToken(userId: number | string): Promise<AdminUserSubscription> {
    const path = plannedApiPaths.adminUserSubscriptionTokenReset.replace('{id}', encodeURIComponent(String(userId)));
    const data = await request<Record<string, unknown>>(path, { method: 'POST' });
    return normalizeAdminUserSubscription(data, userId, rustApiPaths.subscriptionDownload);
  },

  // 管理员直接给目标用户重置密码：无需邮箱验证码，提交后该用户全部登录态失效。
  async resetAdminUserPassword(userId: number | string, newPassword: string): Promise<void> {
    const path = plannedApiPaths.adminUserResetPassword.replace('{id}', encodeURIComponent(String(userId)));
    await request<unknown>(path, {
      method: 'POST',
      body: JSON.stringify({ new_password: newPassword }),
    });
  },

  async getAdminOrders(query: AdminOrderQuery = {}): Promise<AdminOrderPage> {
    const params = new URLSearchParams();
    appendQueryParam(params, 'page', query.page);
    appendQueryParam(params, 'page_size', query.pageSize);
    appendQueryParam(params, 'status', query.status);
    appendQueryParam(params, 'user', query.user);
    appendQueryParam(params, 'email', query.email);
    appendQueryParam(params, 'keyword', query.keyword);
    appendQueryParam(params, 'order_no', query.orderNo);
    const path = params.toString() ? `${plannedApiPaths.adminOrders}?${params.toString()}` : plannedApiPaths.adminOrders;
    const data = await request<unknown>(path);
    return normalizeAdminOrderPage(data, query);
  },

  async getUserOrders(): Promise<OrderInfo[]> {
    const data = await request<unknown>(plannedApiPaths.userOrders);
    return collectionValue(data, ['orders', 'items', 'data']).map(normalizeOrder);
  },

  async createOrder(payload: CreateOrderPayload): Promise<OrderInfo> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.userOrders, {
      method: 'POST',
      body: JSON.stringify({
        plan_id: payload.planId,
        // 渠道留空时不提交，由后端用默认渠道；多渠道时由购买页选中后传入。
        channel: payload.channel || undefined,
      }),
    });
    return normalizeOrder(data);
  },

  async getAdminRedeemCodes(): Promise<RedeemCodeInfo[]> {
    const data = await request<unknown>(plannedApiPaths.adminRedeemCodes);
    return collectionValue(data, ['codes', 'redeemCodes', 'redeem_codes', 'items', 'data']).map(normalizeRedeemCode);
  },

  async createAdminRedeemCodes(payload: CreateRedeemCodesPayload): Promise<RedeemCodeInfo[]> {
    const data = await request<unknown>(plannedApiPaths.adminRedeemCodes, {
      method: 'POST',
      body: JSON.stringify({
        plan_id: payload.planId,
        count: payload.count,
        duration_days: payload.durationDays,
        expires_at: payload.expiresAt || undefined,
      }),
    });
    const created = collectionValue(data, ['codes', 'redeemCodes', 'redeem_codes', 'items', 'data']);
    return created.length > 0 ? created.map(normalizeRedeemCode) : [normalizeRedeemCode(data)];
  },

  async redeem(payload: RedeemPayload): Promise<RedeemResult> {
    const data = await request<unknown>(plannedApiPaths.redeem, {
      method: 'POST',
      body: JSON.stringify(payload),
    });
    return isRecord(data) ? normalizeRedeemResult(data) : { message: '兑换成功', planName: '', expiresAt: '' };
  },

  async getSubscriptionSettings(): Promise<SubscriptionSettings> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminSubscriptionSettings);
    return normalizeSubscriptionSettings(data);
  },

  async updateSubscriptionSettings(payload: Partial<SubscriptionSettings>): Promise<SubscriptionSettings> {
    const data = await request<unknown>(plannedApiPaths.adminSubscriptionSettings, {
      method: 'PUT',
      body: JSON.stringify(subscriptionSettingsPayload(payload)),
    });
    if (!isRecord(data)) throw new Error('订阅设置响应格式不正确');
    return normalizeSubscriptionSettings(data);
  },
};
