# 后台支付设置 + 多渠道支付架构 设计

日期:2026-06-18
状态:待用户复核

## 1. 目标

把支付配置从环境变量/代码搬到**数据库 + 后台「支付设置」页**,管理员可在 UI 增改;并把支付从单一支付宝扩展为**多 provider 架构**:

- **支付宝当面付**:已实现,本轮把配置从 env 迁到 DB/UI。
- **易支付(epay 协议)聚合支付**:本轮**完整实现**(下单 + MD5 验签 + 回调→开通 + UI 配置)。
- **微信支付**:本轮**只留配置口 + provider 桩**(UI 字段 + trait 实现返回"未启用",功能后续再做)。

不再把任何商户密钥写死在代码/必填 env 里。

## 2. 架构

### 2.1 配置存储(`site_settings.payment`)

仿 `auth_security` 范式,新增 `payment` 段:

```jsonc
{
  "enabled": false,                       // 支付总开关(替代 XRAYC_PAYMENT_ENABLED)
  "default_channel": "",                  // 可空;前端下单未指定时用第一个启用渠道
  "providers": {
    "alipay": {
      "enabled": false,
      "environment": "sandbox",           // sandbox|production,决定网关
      "app_id": "",
      "app_private_key": "",              // write-only
      "alipay_public_key": "",
      "notify_url": ""                    // 空则自动用 站点公网地址 + /api/payment/alipay/notify
    },
    "epay": {
      "enabled": false,
      "api_url": "",                      // 如 https://pay.example.com
      "pid": "",
      "key": "",                          // write-only 商户密钥
      "notify_url": ""                    // 空则自动
    },
    "wechat": {                           // 留口,本轮不实现 provider 逻辑
      "enabled": false,
      "mch_id": "", "app_id": "",
      "api_v3_key": "",                   // write-only
      "cert_serial_no": "", "private_key": "",  // write-only
      "notify_url": ""
    }
  }
}
```

### 2.2 provider 抽象(后端,`crates/api`)

统一 trait,三个 provider 各实现:

```rust
struct PayResult { qr_code: Option<String>, pay_url: Option<String> } // 二维码 或 跳转URL
struct PaidNotice { out_trade_no: String, channel_tx_id: String, amount_cents: i64 } // 验签通过后的已付信息

trait PaymentProvider {
    fn channel(&self) -> &str;                                   // "alipay"|"epay"|"wechat"
    async fn create_payment(&self, out_trade_no, amount_cents, subject) -> Result<PayResult, String>;
    fn verify_notify(&self, params: &BTreeMap<String,String>) -> Option<PaidNotice>; // None=验签失败/非成功态
}
```

- **AlipayProvider**:现有逻辑,改为从 DB 配置构造(不再 `from_env`);`create_payment` = precreate 返回 `qr_code`;`verify_notify` 沿用 RSA2。
- **EpayProvider**:见 §3。
- **WechatProvider(桩)**:`create_payment` 返回 `Err("微信支付暂未启用")`,`verify_notify` 返回 None。

provider 工厂:`fn payment_provider(channel, settings) -> Option<Box<dyn PaymentProvider>>`,从 DB `payment` 段按渠道装配,渠道未启用/配置缺失返回 None。

### 2.3 下单与回调

- **下单** `POST /api/orders {plan_id, channel?}`:总开关关 → 短路;按 `channel`(或 default/第一个启用)取 provider → `create_payment` → 响应带 `qr_code`/`pay_url` + `pay_channel`。失败把错误塞 `payment_error`,不阻断下单。
- **回调**:每渠道一个公开端点 `POST /api/payment/{alipay,epay}/notify`(微信留 `/wechat/notify` 占位返回 failure)。各自 `verify_notify` → 拿 `PaidNotice` → **复用既有 `apply_payment_callback_json` 入账闭环**(`channel_tx_id` 作幂等 `tx_id`、`currency=CNY`、`payment_address` 对齐收款标识、金额校验)→ 回该渠道要求的成功应答(支付宝/epay 都是纯文本 `success`)。

### 2.4 管理接口

- `GET /api/admin/payment-settings`:返回 `payment` 段,**所有 write-only 字段脱敏**(私钥/key 不回显,返回是否已设置的布尔)。
- `PUT /api/admin/payment-settings`:深合并;write-only 字段空则保留旧值(复用 `preserve_write_only_*` 通用化)。审计只记渠道与启用状态,不记密钥。
- (可选)`POST /api/admin/payment-settings/test {channel}`:用当前配置做一次最小下单干跑(支付宝 precreate / epay mapi),校验密钥是否可用,返回成功/错误。

## 3. 易支付(epay)协议

- **下单**:优先 `POST {api_url}/mapi.php`(API 模式,返回 JSON `{code:1, qrcode|payurl|urlscheme}`)→ 有 `qrcode` 则前端渲染二维码,有 `payurl` 则跳转;mapi 不可用时回退 `{api_url}/submit.php`(页面跳转,返回带签名的跳转 URL)。
- **参数**:`pid, type(alipay|wxpay), out_trade_no, notify_url, return_url, name, money(元,两位小数), sign, sign_type=MD5`。
- **签名**:参数(排除 `sign`、`sign_type`、空值)按 key 升序拼 `a=b&c=d`,**末尾直接拼商户 `key`(不加 &key=)**,MD5 取小写。
- **异步通知**:epay GET/POST `pid, trade_no, out_trade_no, type, name, money, trade_status, sign, sign_type`;按同规则重算 MD5 验签,`trade_status=TRADE_SUCCESS` 即已付;`channel_tx_id=trade_no`,`amount_cents=元*100`。回纯文本 `success`。

## 4. 前端

- 新 `PaymentSettingsPage.vue` + 路由 `admin/payment-settings` + 菜单「支付设置」(仿 `AuthSecurityPage`)。
- 布局:总开关 + 三个分区卡片(支付宝 / 聚合支付(易支付) / 微信)。每区:启用开关 + 字段表单;私钥/key 用 write-only(占位"已设置,留空不变")。微信区标注"即将支持"。
- 购买页 `UserPlansPage`:按 `GET /api/auth/security` 类似的公开渠道清单展示**可用支付方式**,用户选渠道下单;`qr_code` 渲染二维码 / `pay_url` 跳转;沿用现有轮询。

## 5. 安全 / 迁移 / 测试

- **安全**:所有商户密钥**明文存 DB**(字段加密已下线,口径一致),UI write-only 不回显,日志/审计不记明文。
- **迁移**:迁移种一条默认 `payment`(全关、空);首启若检测到旧 `ALIPAY_*` env 则一次性导入对应字段(平滑过渡),之后以 DB 为准。`XRAYC_PAYMENT_ENABLED` 仅作可选硬总闸,UI 开关为主控。
- **测试**:
  - PG 集成:`payment` 设置往返 + write-only 保留;provider 工厂按 DB 装配。
  - 单测:epay/alipay 待签串与签名/验签(往返 + 篡改拒绝);金额换算。
  - 真实回归:支付宝沙箱 precreate 仍出码、notify→开通仍通(改读 DB 后)。epay 若有可用测试商户则真跑,否则用本地 mock + 单测覆盖签名,并在日报标注"epay 真实回归待真实商户"。

## 6. 不在本轮范围

- 微信支付 provider 真实逻辑(仅留配置口 + 桩)。
- 多币种、退款、对账单下载。
- 支付方式的按套餐/按用户细分。

## 7. 验收标准

1. 后台「支付设置」页可增改支付宝/epay/微信配置并持久化;私钥 write-only 不回显。
2. 支付宝配置全部来自 DB(env 不再必填);沙箱 precreate + notify→开通仍通。
3. epay:配置后下单出码/跳转、真实或 mock 回调验签通过→订单已支付→套餐开通,`trade_no` 幂等。
4. 微信区可填可存但下单返回"暂未启用"。
5. 全量门禁(cargo check + 前端 build + PG 集成 + 真实沙箱回归)通过。
