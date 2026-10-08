# VLESS 量子加密 + SNI 合并 + 伪装域名分类下拉 — 设计

> 日期 2026-06-28。用户目标:① VLESS 量子加密(后量子,带开关)② 域名直连/CF 把 SNI 并进「选用域名」③ Reality 伪装域名做成分类下拉(后量子/普通两组)④ 标签全中文随模式变。
> **范围红线(用户明确)**:不考虑 sing-box 生态、不做客户端分流——量子加密是入口属性,开了就用,不支持的客户端(sing-box 系)连不上即连不上,不在考虑范围。

## 1. 背景与内核口径

- Xray-core **VLESS Encryption(原生加密)**:`encryption`/`decryption` 字段,格式 `mlkem768x25519plus.<模式>.<ttl|rtt>.[padding].<密钥>`。**密钥交换恒为 ML-KEM-768 + X25519 混合 = 后量子**(协议固定,`plus` 前缀)。
- 我们内核 **26.5.9 已支持**(25.9.5 引入,格式到最新 26.6.27 未变),**不升级内核**。
- `vlessenc` X25519 认证档的密钥**就是 Reality 同款 x25519**(`xray x25519` 两者共用)。
- **决策**:密钥用 **Rust `x25519_dalek` 生成**(复用 Reality 的 `generate_reality_entry_key_pair`),拼成:
  - 服务端 `decryption` = `mlkem768x25519plus.native.600s.<x25519私钥 b64url-nopad>`
  - 客户端 `encryption` = `mlkem768x25519plus.native.0rtt.<x25519公钥 b64url-nopad>`
  - 密钥交换仍是后量子(协议固定);认证用 X25519(数据机密性已后量子,足够防「现在录以后解」)。**以真客户端 E2E 连通为最终判据;若 Rust 拼串不通,退回 shell 调 `xray vlessenc`。**

## 2. 功能一:VLESS 量子加密(带开关)

### 适用范围
- **仅入口(access_entries)的 VLESS**,且**非 Reality**(security ∈ {tls, ''/none});CF 也算(CF=VLESS+TLS+WS/gRPC/XHTTP)。
- **Reality 不给量子开关**(用户定):Reality 要后量子靠选支持后量子的 dest(见功能三),不叠。
- **本机出口不做量子**:本机出口是同机回环出口,无观测者、无「现在录以后解」威胁,后量子无意义(评估结论,文档说明)。

### 数据模型(迁移 access_entries 加列)
- `vless_decryption TEXT`(服务端串,含私钥 → **加密落库**,绝不入订阅/日志/审计)。
- `vless_encryption TEXT`(客户端串,含公钥 → 给客户端,可明文,进订阅)。
- 量子是否开启 = `vless_decryption` 非空派生(不单列 bool,避免不一致)。

### 生成时机
入口创建/编辑,当 protocol=vless ∧ security≠reality ∧ 前端传 `vless_quantum_encryption=true`:Rust 生成 x25519 对 → 拼上面两串 → 存 `vless_decryption`(加密)+ `vless_encryption`。关闭则清空两列。

### 配置/订阅
- 入站:VLESS `decryption` = `vless_decryption`(已有字段 `inbound.rs:67`,原 "none" 改为读该列)。
- 订阅:`vless_proxy_for_line` 输出 `encryption` = `vless_encryption`(原隐含 none)。
- 防泄露:`vless_decryption`(私钥)绝不进订阅;只下发 `vless_encryption`(公钥)。

### 校验
- 量子 + Reality → 拒(InvalidAgentPayload)。
- 量子 + 非 VLESS → 拒。

## 3. 功能二:SNI 合并(分模式)

| 安全模式 | 域名字段处理 |
|---|---|
| **Reality**(IP直连) | 保留独立「**伪装域名**」(分类下拉,见功能三),默认 www.cloudflare.com |
| **TLS(域名直连/CF)** | **合并**:SNI 自动=「选用域名」,前端**不再单独显示** SNI 输入框(后端 server_name 仍按选用域名落库,已有逻辑) |
| **普通 none** | 无 SNI,本就不显示 |

- 后端:server_name 对 TLS 仍= 选用域名(现有 `defaultEntryServerName`/`normalizeEntryProtocolFields` 已做),前端只是隐藏冗余输入框。

## 4. 功能三:伪装域名分类下拉(Reality)

- 前端把 Reality 的「伪装域名」做成**分组下拉 + 可自定义输入**(allow-create),两组(实测 `xray tls ping <域名>` 带 SNI 握手判定,2026-06-28):
  - **【支持后量子 · X25519MLKEM768】**:`www.apple.com, swcdn.apple.com, swdist.apple.com, gateway.icloud.com, www.cloudflare.com, www.amazon.com, aws.amazon.com, dl.google.com, www.google.com, www.amd.com, www.python.org, addons.mozilla.org, www.wikipedia.org, www.yahoo.com, www.paypal.com, www.visa.com, www.qualcomm.com, www.ibm.com`
  - **【普通伪装 · 仅 X25519】**:`www.microsoft.com, www.bing.com, www.nvidia.com, www.samsung.com, www.tesla.com, www.intel.com, www.adobe.com, www.salesforce.com, www.oracle.com`
- 选中即 serverName(dest 仍自动 = serverName:443,现有逻辑)。

## 5. 标签中文化(随模式变)

- Reality → 「**伪装域名**」
- TLS(域名直连/CF)→ 合并进「选用域名」,不单独显示(或只读显示「证书域名」)
- 入口("Server Name")与本机出口("SNI/服务名")统一这套中文口径。

## 6. 前端开关汇总

- VLESS **量子加密** 开关:非 Reality(TLS/CF/none)显示;Reality 隐藏。开关旁提示「开启后仅 Xray/mihomo 系客户端可用」。
- VLESS 安全模式仍是现有下拉(Reality/TLS/none,按连接方式过滤)。

## 7. 测试计划

- 单测:key-gen 串格式、入站 decryption 落库、订阅 encryption 输出、量子+Reality 拒。
- E2E(真机真客户端):量子入口 → 真 Xray 客户端带 `encryption` 串连通 → egress 正确。
- 全量功能:入口三模式 + 量子开/关 + 合并后 SNI + 伪装域名分类。
- 压测:3 轮 × 30min(3 台测试机)。

## 8. 不做 / 范围外

- sing-box 兼容、客户端按 UA 分流:**不做**(用户明确)。
- 内核升级:不升(26.5.9 已支持)。
- ML-KEM-768 认证档:先用 X25519 认证档(数据已后量子);如 E2E 需要再切。
- 本机出口量子加密:不做(回环无意义)。
