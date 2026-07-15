// 本文件定义支付设置与购买渠道相关的前端 API 类型。
// 它服务后台支付设置页和用户购买页的渠道选择，只描述数据结构。
// 写值类敏感字段（私钥/密钥）不回显，改用布尔 `*Set` 标记是否已配置。
// 文件不发起请求、不实现支付流程，仅作为 normalizer 与页面之间的类型契约。

export type AlipayPaymentProvider = {
  enabled: boolean;
  environment: 'sandbox' | 'production';
  appId: string;
  // 应用私钥写值不回显，仅用布尔标记是否已设置；提交时留空表示沿用旧值。
  appPrivateKey: string;
  appPrivateKeySet: boolean;
  alipayPublicKey: string;
  notifyUrl: string;
};

export type EpayPaymentProvider = {
  enabled: boolean;
  apiUrl: string;
  pid: string;
  // 商户密钥写值不回显，仅用布尔标记是否已设置；提交时留空表示沿用旧值。
  key: string;
  keySet: boolean;
  notifyUrl: string;
};

export type WechatPaymentProvider = {
  enabled: boolean;
  mchId: string;
  appId: string;
  // APIv3 密钥与商户私钥写值不回显，仅用布尔标记是否已设置。
  apiV3Key: string;
  apiV3KeySet: boolean;
  certSerialNo: string;
  privateKey: string;
  privateKeySet: boolean;
  notifyUrl: string;
};

export type PaymentSettings = {
  enabled: boolean;
  defaultChannel: string;
  providers: {
    alipay: AlipayPaymentProvider;
    epay: EpayPaymentProvider;
    wechat: WechatPaymentProvider;
  };
};

export type PaymentChannel = {
  channel: string;
  label: string;
};

export type PaymentChannelsInfo = {
  enabled: boolean;
  channels: PaymentChannel[];
};
