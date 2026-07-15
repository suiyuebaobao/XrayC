// 本文件负责支付设置与购买渠道响应的标准化与提交序列化。
// 它把后端 snake_case 字段转成前端稳定类型，写值类敏感字段不回显只读 `*_set`。
// 提交时写值字段留空则删除不传，后端保留旧密钥/私钥，避免页面回显覆盖。
// 本文件不发起请求、不保存凭据，只做纯数据转换。

import type {
  AlipayPaymentProvider,
  EpayPaymentProvider,
  PaymentChannelsInfo,
  PaymentSettings,
  WechatPaymentProvider,
} from '../types';
import { arrayValue, booleanValue, recordValue, stringValue } from '../primitives';

function normalizeAlipayProvider(value: unknown): AlipayPaymentProvider {
  const data = recordValue(value);
  const environment = stringValue(data.environment ?? data.env);
  return {
    enabled: booleanValue(data.enabled),
    environment: environment === 'production' ? 'production' : 'sandbox',
    appId: stringValue(data.appId ?? data.app_id),
    appPrivateKey: '',
    appPrivateKeySet: booleanValue(data.appPrivateKeySet ?? data.app_private_key_set),
    alipayPublicKey: stringValue(data.alipayPublicKey ?? data.alipay_public_key),
    notifyUrl: stringValue(data.notifyUrl ?? data.notify_url),
  };
}

function normalizeEpayProvider(value: unknown): EpayPaymentProvider {
  const data = recordValue(value);
  return {
    enabled: booleanValue(data.enabled),
    apiUrl: stringValue(data.apiUrl ?? data.api_url),
    pid: stringValue(data.pid),
    key: '',
    keySet: booleanValue(data.keySet ?? data.key_set),
    notifyUrl: stringValue(data.notifyUrl ?? data.notify_url),
  };
}

function normalizeWechatProvider(value: unknown): WechatPaymentProvider {
  const data = recordValue(value);
  return {
    enabled: booleanValue(data.enabled),
    mchId: stringValue(data.mchId ?? data.mch_id),
    appId: stringValue(data.appId ?? data.app_id),
    apiV3Key: '',
    apiV3KeySet: booleanValue(data.apiV3KeySet ?? data.api_v3_key_set),
    certSerialNo: stringValue(data.certSerialNo ?? data.cert_serial_no),
    privateKey: '',
    privateKeySet: booleanValue(data.privateKeySet ?? data.private_key_set),
    notifyUrl: stringValue(data.notifyUrl ?? data.notify_url),
  };
}

export function normalizePaymentSettings(data: Record<string, unknown>): PaymentSettings {
  const providers = recordValue(data.providers);
  return {
    enabled: booleanValue(data.enabled),
    defaultChannel: stringValue(data.defaultChannel ?? data.default_channel) || 'alipay',
    providers: {
      alipay: normalizeAlipayProvider(providers.alipay),
      epay: normalizeEpayProvider(providers.epay),
      wechat: normalizeWechatProvider(providers.wechat),
    },
  };
}

export function paymentSettingsPayload(payload: PaymentSettings) {
  const alipay: Record<string, unknown> = {
    enabled: payload.providers.alipay.enabled,
    environment: payload.providers.alipay.environment,
    app_id: payload.providers.alipay.appId,
    alipay_public_key: payload.providers.alipay.alipayPublicKey,
    notify_url: payload.providers.alipay.notifyUrl,
  };
  // 写值字段留空表示不修改，删除该键让后端保留旧私钥。
  if (payload.providers.alipay.appPrivateKey) {
    alipay.app_private_key = payload.providers.alipay.appPrivateKey;
  }

  const epay: Record<string, unknown> = {
    enabled: payload.providers.epay.enabled,
    api_url: payload.providers.epay.apiUrl,
    pid: payload.providers.epay.pid,
    notify_url: payload.providers.epay.notifyUrl,
  };
  if (payload.providers.epay.key) {
    epay.key = payload.providers.epay.key;
  }

  const wechat: Record<string, unknown> = {
    enabled: payload.providers.wechat.enabled,
    mch_id: payload.providers.wechat.mchId,
    app_id: payload.providers.wechat.appId,
    cert_serial_no: payload.providers.wechat.certSerialNo,
    notify_url: payload.providers.wechat.notifyUrl,
  };
  if (payload.providers.wechat.apiV3Key) {
    wechat.api_v3_key = payload.providers.wechat.apiV3Key;
  }
  if (payload.providers.wechat.privateKey) {
    wechat.private_key = payload.providers.wechat.privateKey;
  }

  return {
    enabled: payload.enabled,
    default_channel: payload.defaultChannel,
    providers: { alipay, epay, wechat },
  };
}

export function normalizePaymentChannels(data: Record<string, unknown>): PaymentChannelsInfo {
  return {
    enabled: booleanValue(data.enabled),
    channels: arrayValue(data.channels).map((item) => {
      const channel = recordValue(item);
      return {
        channel: stringValue(channel.channel ?? channel.value ?? channel.id),
        label: stringValue(channel.label ?? channel.name) || stringValue(channel.channel),
      };
    }),
  };
}
