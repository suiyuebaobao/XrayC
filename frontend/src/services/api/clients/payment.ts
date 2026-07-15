// 本文件维护支付设置与购买渠道相关 API 方法。
// 它只组织请求参数并调用支付 normalizer，不实现真实支付/下单逻辑。
// 写值类敏感字段（私钥/密钥）由 normalizer 控制不回显、留空不提交。
// 新增支付渠道接口时应放在这里，而不是页面组件里。

import type { PaymentChannelsInfo, PaymentSettings } from '../types';
import { request } from '../http';
import { isRecord } from '../primitives';
import {
  normalizePaymentChannels,
  normalizePaymentSettings,
  paymentSettingsPayload,
} from '../normalizers/payment';
import { plannedApiPaths } from '../paths';

export const paymentApi = {
  async getPaymentSettings(): Promise<PaymentSettings> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminPaymentSettings);
    return normalizePaymentSettings(data);
  },

  async updatePaymentSettings(payload: PaymentSettings): Promise<PaymentSettings> {
    const data = await request<unknown>(plannedApiPaths.adminPaymentSettings, {
      method: 'PUT',
      body: JSON.stringify(paymentSettingsPayload(payload)),
    });
    return isRecord(data) ? normalizePaymentSettings(data) : payload;
  },

  async getPaymentChannels(): Promise<PaymentChannelsInfo> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.paymentChannels);
    return normalizePaymentChannels(data);
  },
};
