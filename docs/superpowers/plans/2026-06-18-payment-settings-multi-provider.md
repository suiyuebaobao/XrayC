# 多渠道支付设置 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development(推荐)或 superpowers:executing-plans 逐任务实现。步骤用 `- [ ]` 跟踪。每个任务内部按 TDD:先写失败测试→看红→最小实现→看绿→提交。

**Goal:** 把支付配置从 env 迁到 DB + 后台「支付设置」页,并把支付扩展为多 provider(支付宝迁DB / 易支付epay 新做 / 微信留桩)。

**Architecture:** `site_settings.payment` 段存各渠道配置(私钥 write-only);后端统一 `PaymentProvider` trait,工厂按 DB 装配 provider;下单按渠道出码/跳转,各渠道独立 notify 端点复用既有订单入账闭环。

**Tech Stack:** Rust(Axum/SQLx)、RSA(已用)、md-5 crate(epay)、Vue3/Element Plus、PostgreSQL site_settings。

---

## 文件结构

**db crate**
- `crates/db/src/settings_json.rs`(改):`default_payment_setting()`、`public_payment_settings(value)`(脱敏)。
- `crates/db/src/store/json_util.rs`(改):`preserve_write_only_at(value, current, &[path])` 通用化(现有 SMTP 版改调它)。
- `crates/db/src/store/settings.rs`(改):`payment_settings_json()` / `update_payment_settings_json(value)`。
- `migrations/202606180001_default_payment_setting.sql`(新):种默认 payment 设置。

**api crate**
- `crates/api/src/payment.rs`(新):`PaymentProvider` trait、`PayResult`/`PaidNotice`、`payment_provider(channel,&Value)` 工厂、`channel_receive_address()`。
- `crates/api/src/alipay.rs`(改):`AlipayConfig::from_settings(&Value)`,实现 `PaymentProvider`。
- `crates/api/src/epay.rs`(新):`EpayProvider`(MD5 签名、mapi/submit、verify_notify)。
- `crates/api/src/billing.rs`(改):`attach_payment(data,channel,&settings)`、各渠道 notify handler 走 trait。
- `crates/api/src/admin_settings.rs`(改):`admin_payment_settings`(GET)、`update_admin_payment_settings`(PUT)。
- `crates/api/src/public.rs`(改):`public_payment_channels`(下单页用)。
- `crates/api/src/lib.rs`(改):路由 `/api/admin/payment-settings`、`/api/payment/{epay,wechat}/notify`、`/api/payment/channels`。

**frontend**
- `frontend/src/views/PaymentSettingsPage.vue`(新)+ router + AppShell 菜单。
- `frontend/src/views/UserPlansPage.vue`(改):渠道选择。
- `frontend/src/services/api/{types,normalizers,clients,paths}`(改):payment-settings + channels。

---

## Phase 1 — DB 设置层

### Task 1.1：payment 默认结构 + 脱敏
**Files:** 改 `crates/db/src/settings_json.rs`;测试加在 `crates/db/src/tests/part45.rs`(非 PG)。
- [ ] 写失败单测 `test_default_payment_setting_shape`:`default_payment_setting()` 含 `enabled=false`、`providers.alipay/epay/wechat`,且 `public_payment_settings` 把 `app_private_key`/`key`/`api_v3_key`/`private_key` 替换为布尔 `*_set`(不含明文)。
- [ ] 看红(函数未定义)。
- [ ] 实现 `default_payment_setting()`(按 spec §2.1 JSON)+ `public_payment_settings(value)`(遍历 providers,write-only 字段 → `{field}_set: bool`,删原值)。
- [ ] 看绿。提交 `feat(db): payment settings default + public masking`。

### Task 1.2：write-only 通用保留
**Files:** 改 `crates/db/src/store/json_util.rs`;测试 `part45.rs`。
- [ ] 写失败单测 `test_preserve_write_only_at_keeps_old_when_blank`:对 `{providers:{alipay:{app_private_key:""}}}` + current 有旧值 → 保留旧值;新值非空 → 覆盖。
- [ ] 实现 `pub(crate) fn preserve_write_only_at(mut value, current, paths: &[&[&str]]) -> Value`(按路径数组逐个:新值空/缺 → 用 current 同路径值)。把现有 `preserve_write_only_smtp_password` 改为内部调用它(路径 `["email_verification","smtp_password"]`),保证 SMTP 行为不变。
- [ ] 看绿(含原 SMTP 测试仍过)。提交。

### Task 1.3：payment 设置读写 store + 迁移
**Files:** 改 `crates/db/src/store/settings.rs`;新 `migrations/202606180001_default_payment_setting.sql`;PG 测试 `crates/db/src/tests/part03.rs` 风格新增分片用例。
- [ ] 迁移:`INSERT INTO site_settings(setting_key,setting_value) VALUES('payment', '<默认JSON>') ON CONFLICT DO NOTHING;`(默认JSON = default_payment_setting 的等价)。
- [ ] 写失败 PG 集成测试 `test_pg_payment_settings_roundtrip_and_write_only`(needs DATABASE_URL):`update_payment_settings_json` 写入 alipay+epay 配置含私钥→`payment_settings_json` 读回→私钥保留;再 PUT 私钥留空→旧私钥仍在;`public_*` 不含明文。
- [ ] 实现 `payment_settings_json()`(读 site_settings 'payment',merge 默认)、`update_payment_settings_json(value)`(merge 默认+current → `preserve_write_only_at` 保护 6 个 write-only 路径 → upsert)。
- [ ] 看绿(容器内带临时 PG)。提交。

---

## Phase 2 — provider 抽象 + 三个 provider

### Task 2.1：trait + 工厂 + 类型
**Files:** 新 `crates/api/src/payment.rs`;`lib.rs` 加 `mod payment;`。
- [ ] 定义(无测试,纯类型):
```rust
pub(crate) struct PayResult { pub qr_code: Option<String>, pub pay_url: Option<String> }
pub(crate) struct PaidNotice { pub out_trade_no: String, pub channel_tx_id: String, pub amount_cents: i64 }
#[async_trait::async_trait]  // 或用 trait + BoxFuture;若不引 async-trait,用枚举分发
pub(crate) trait PaymentProvider: Send + Sync {
    fn channel(&self) -> &'static str;
    async fn create_payment(&self, out_trade_no: &str, amount_cents: i64, subject: &str) -> Result<PayResult, String>;
    fn verify_notify(&self, params: &std::collections::BTreeMap<String,String>) -> Option<PaidNotice>;
}
pub(crate) fn payment_provider(channel: &str, settings: &serde_json::Value) -> Option<Box<dyn PaymentProvider>>;
```
> 注:Rust async trait 用 `async-trait` crate(加 workspace dep)最简;或改成枚举 `enum Provider { Alipay(..), Epay(..) }` 手写 async 方法避免 dyn。**计划采用枚举分发**避免 dyn+async 复杂度:`enum PaymentChannel { Alipay(AlipayConfig), Epay(EpayConfig) }`,impl 上写 `async fn create_payment/verify_notify`。
- [ ] 实现 `payment_channel(channel,&settings) -> Option<PaymentChannel>`:按 channel 取 `providers.{channel}`,`enabled` 且必填项齐 → 构造;否则 None。提交。

### Task 2.2:AlipayConfig 从 DB 装配 + 接 trait
**Files:** 改 `crates/api/src/alipay.rs`;复用现有单测。
- [ ] 写失败单测 `test_alipay_from_settings_builds`:给一段 `providers.alipay` JSON(测试密钥)→ `AlipayConfig::from_settings` 成功;缺 app_id → None。
- [ ] 实现 `from_settings(&Value)`(读 app_id/app_private_key/alipay_public_key/environment→gateway/notify_url);保留 `from_env` 仅作迁移兜底。`environment=="production"` → 生产网关。
- [ ] 现有 4 个 alipay 单测仍过。提交。

### Task 2.3:EpayProvider(epay 协议)
**Files:** 新 `crates/api/src/epay.rs`;`lib.rs` `mod epay;`;加 workspace dep `md-5`。
- [ ] 写失败单测:
  - `test_epay_sign_md5`:固定参数 → 期望 MD5(参数升序拼 + 末尾接 key,排除 sign/sign_type/空)。用已知向量断言。
  - `test_epay_verify_notify_accepts_valid_rejects_tamper`:构造 notify 参数 + 正确 sign → 返回 PaidNotice;改 money → None。
  - `test_epay_money_cents`:`"0.01"→1`、`"12.30"→1230`。
- [ ] 实现 `EpayConfig{api_url,pid,key,notify_url}`;`sign(params)`(BTreeMap 升序、filter sign/sign_type/空、`k=v&` 拼接、末尾直接 `+key`、`md5` 小写 hex);`verify_notify`(重算 sign 比对 + `trade_status=="TRADE_SUCCESS"` → PaidNotice{out_trade_no, channel_tx_id=trade_no, amount_cents=money*100});`create_payment`(async:POST `{api_url}/mapi.php` form `pid,type=alipay,out_trade_no,notify_url,name,money,sign,sign_type` → JSON `code==1` 取 `qrcode`/`payurl`;失败回退构造 `{api_url}/submit.php?...签名` 作为 `pay_url`)。charset 同样显式 utf-8。
- [ ] 看绿。提交 `feat(api): epay provider`。

### Task 2.4:微信桩
**Files:** `crates/api/src/payment.rs` 内枚举加 `Wechat` 占位 OR 单独不做 provider,工厂对 wechat 返回 None + handler 返回"暂未启用"。
- [ ] 无需 provider 逻辑:`payment_channel("wechat",..)` 返回 None;notify `/api/payment/wechat/notify` handler 直接回 `failure` + 日志"微信暂未启用"。提交。

---

## Phase 3 — 管理接口

### Task 3.1:GET/PUT payment-settings + 路由
**Files:** 改 `crates/api/src/admin_settings.rs`、`lib.rs`;Axum router 测试参考现有 admin 测试。
- [ ] 写失败测试(handler 级或 PG):`PUT /api/admin/payment-settings` 写入 → `GET` 返回脱敏(私钥字段为 `*_set:true`,无明文);PUT 私钥留空 → 旧值保留(经 store 已测,这里测 handler 串通)。
- [ ] 实现 `admin_payment_settings`(GET:`pg.payment_settings_json()` → `public_payment_settings` → Json)、`update_admin_payment_settings`(PUT:body → `pg.update_payment_settings_json` → 审计 action=`payment_settings.update`,摘要只含各 provider `enabled`/`environment`);`lib.rs` 加 `.route("/api/admin/payment-settings", get(admin_payment_settings).put(update_admin_payment_settings))`。
- [ ] 看绿。提交。

---

## Phase 4 — 下单 / 回调接线

### Task 4.1:下单按渠道出码
**Files:** 改 `crates/api/src/billing.rs`(`create_order`/`attach_payment`)、`dto.rs`(`CreateOrderRequest` 加 `channel: Option<String>`)。
- [ ] `attach_payment(data,&pg,channel)`:读 `pg.payment_settings_json()`;总 `enabled` 关 → 不出码;选 channel(req.channel 或 default_channel 或第一个 enabled provider)→ `payment_channel(..)` → `create_payment(order_no,amount_cents,subject)` → 写 `qr_code`/`pay_url`/`pay_channel`;失败写 `payment_error`。`create_order` 改调它(替代 `attach_alipay_qr`)。`payment_feature_enabled()` 改为读 DB 总开关(env 作兜底)。
- [ ] 测试:mock/集成下单返回带 channel 字段(支付宝沙箱真出码作回归)。提交。

### Task 4.2:统一 notify(alipay 改走 trait + epay 新增)
**Files:** 改 `crates/api/src/billing.rs`、`lib.rs`。
- [ ] `alipay_notify`/`epay_notify` 各:读 settings → `payment_channel(channel)` → `verify_notify(params)` → `Some(PaidNotice)` 则用 `apply_payment_callback_json`(order_no=out_trade_no, tx_id=channel_tx_id, amount_cents, currency=CNY, payment_address=收款标识, status=success)→ 审计 → 回 `success`;`None` 回 `failure`。`lib.rs` 加 epay/wechat notify 路由。收款标识统一改为常量(不再依赖 PAYMENT_RECEIVE_ADDRESS env;订单插入与回调对齐)。
- [ ] 测试:伪造 notify 拒为 failure;epay 合法 notify(单测构造签名)→ 入账(PG 集成);支付宝沙箱真实 notify 回归仍通。提交。

### Task 4.3:公开渠道清单
**Files:** 改 `crates/api/src/public.rs`、`lib.rs`、`dto`。
- [ ] `GET /api/payment/channels`:返回启用的渠道列表(`[{channel:"alipay",label:"支付宝"},{channel:"epay",label:"聚合支付"}]`)+ 总开关,**不含任何密钥**。提交。

---

## Phase 5 — 前端

### Task 5.1:支付设置页
**Files:** 新 `frontend/src/views/PaymentSettingsPage.vue`、改 router/AppShell、`services/api`(types/normalizers/clients/paths)。
- [ ] 类型 + client:`getPaymentSettings()`/`updatePaymentSettings(payload)`(`/api/admin/payment-settings`);normalizer snake↔camel,write-only 字段提交时空则不传。
- [ ] 页面:总开关 + 3 分区卡(支付宝/聚合支付/微信)。支付宝:环境下拉(沙箱/生产)、APPID、应用私钥(write-only 占位"已设置,留空不变")、支付宝公钥、回调地址(可空)。epay:api_url、pid、key(write-only)。微信:字段灰显 + "即将支持"。保存调 update。
- [ ] router `admin/payment-settings` + AppShell 菜单「支付设置」(admin only)。
- [ ] 提交。

### Task 5.2:购买页渠道选择
**Files:** 改 `frontend/src/views/UserPlansPage.vue`、`services/api`。
- [ ] `getPaymentChannels()`;购买时若多渠道弹选择(单渠道直接用);`createOrder({planId,channel})`;`qr_code`→渲染二维码、`pay_url`→新窗口打开/跳转;沿用轮询。
- [ ] 提交。

---

## Phase 6 — 迁移导入 / 集成 / 门禁

### Task 6.1:旧 env 一次性导入
**Files:** 改 `crates/api/src/main.rs`(启动时)或 worker。
- [ ] 启动时:若 `payment` 设置中 alipay 未配置 且存在 `ALIPAY_APP_ID` env → 把 `ALIPAY_*` 写入 DB payment.alipay 并 enabled=true(平滑迁移),日志记一次。之后以 DB 为准。提交。

### Task 6.2:集成 + 真实回归 + 门禁
- [ ] `cargo check --workspace --all-targets`、前端 `vite build`、PG 集成测试通过。
- [ ] `make rebuild` 部署;后台支付设置页填支付宝沙箱配置(改读 DB)→ 真实沙箱 precreate 出码 + notify→开通仍通。
- [ ] epay:若有可用测试商户则真跑下单+回调;否则单测覆盖签名/验签,日报标注"epay 真实回归待真实商户"。
- [ ] 日报 + 提交。

---

## 自审(spec 覆盖)
- §2.1 存储 → T1.1/1.3 ✅;§2.2 trait/工厂 → T2.1 ✅;支付宝迁DB → T2.2 ✅;epay → T2.3 ✅;微信桩 → T2.4 ✅;§2.4 管理接口 + 脱敏 → T1.1/3.1 ✅;下单/回调 → T4.* ✅;前端 → T5.* ✅;迁移 → T6.1 ✅;安全(明文+write-only)→ T1.1/1.2 ✅;测试 → 各任务 + T6.2 ✅。
- 类型一致:`PayResult{qr_code,pay_url}`、`PaidNotice{out_trade_no,channel_tx_id,amount_cents}`、`payment_channel()` 全计划统一。
- 占位:无 TBD。
