# 节点 3 地址模型 + 协议按地址过滤 + DNS-01 CF 证书 设计方案

> 状态:设计稿(用户已授权自主,跳过用户审阅门禁,直接进实现)。脱敏,禁写真实 IP/域名/凭据。
> 依赖前序:`2026-06-22-node-multimode-direct-cf-design.md`(已实现 cf_enabled/cert_domain/cf_domain + 真机闭环)。本方案是其 UX/正确性升级。

**Goal:** 把中转节点对外身份摆成 3 行明确地址(IP直连/域名直连/CF),入口协议按地址类型动态过滤只剩可用项,CF 放开到 WS/gRPC/XHTTP 全族(含 Trojan-WS),并给 CF 域名用 DNS-01 签**它自己的**证书(不再复用直连证书)。前后端对齐 + 真机闭环。

**Architecture:** 节点显式持有 IP/域名/CF 三个对外地址(各可选);cf_enabled 由 CF 地址是否非空派生。入口创建时地址从节点自动带出、协议下拉按地址类型过滤,后端护栏兜底。CF 域名证书走 certbot DNS-01(用 CF API Token,token 仅安装期用、写节点本机 ini、不落控制面库)。

---

## 1. 节点 3 地址模型(E)

**DB(access_nodes,加列/派生,不破坏现网):**
- 新增 `ip_direct_address TEXT`(节点公网 IP,给 Reality/Shadowsocks 直连)。
- 保留 `cert_domain TEXT`(= **域名直连地址**,灰云,HTTP-01 签证书 + 直连 TLS 协议用)。
- 保留 `cf_domain TEXT`(= **CF 直连地址**,橙云,DNS-01 签证书)。
- `cf_enabled` **改为派生**:`cf_domain` 非空即为 true(写入侧自动置;UI 去掉独立开关)。保留 DB 列,由 store 在写入时按 cf_domain 计算。
- `cf_cert_mode` CHECK 放开到 `('reuse_direct','dns01')`,默认 `dns01`(有 token 时),无 token 兜底 `reuse_direct`。
- `public_host` 保留(客户连接地址,订阅兼容默认 + 老节点);3 个新地址为主模型。

**约束:**
- 至少填一个对外地址(IP/域名/CF 任一)。
- 填了域名直连地址或 CF 地址 → ACME 邮箱必填(签证书)。纯 IP 节点不需要邮箱。

## 2. CF API Token(不落库,仿 SSH 凭据)

- token 只在**安装/续期请求期**用,**不持久化到控制面 DB**(对齐 SSH 凭据红线)。
- 一键安装/安装指南请求携带 `cf_api_token`(可选);写到节点本机 `/etc/letsencrypt/cloudflare.ini`(权限 0600),供 certbot DNS-01 签发 + 后续 `certbot renew` 自动续期用。
- agent env `XRAYC_CLOUDFLARE_API_TOKEN`(从该 ini 或安装注入),只读不回显、不入心跳、不入日志、不入审计。
- 心跳只上报 CF 证书状态(域名/valid/剩余天数/脱敏),不回传 token/ini 路径。

## 3. 入口协议按地址类型过滤 + 护栏(D)

**地址类型 → 可用协议矩阵(总口径):**

| 地址类型 | 可选协议 | 证书 |
|---|---|---|
| IP 直连 | VLESS-Reality、Shadowsocks 2022 | 无 |
| 域名直连(灰云) | Reality、SS **+ Trojan、HY2、VLESS/Trojan 的 WS/gRPC/XHTTP+TLS** | 灰云证书(HTTP-01) |
| CF(橙云) | **(VLESS 或 Trojan)+ 传输 WS/gRPC/XHTTP + TLS** | CF 域名证书(DNS-01) |

**前端**:入口选地址(或选节点的某行地址)后,协议下拉**只显示该类型可用项**;传输模式同步限定。
**后端护栏(`crates/db/src/validation.rs`,升级 `validate_protocol_cdn_combination`):**
- `cdn_enabled=true` 时:必须 `protocol∈{vless,trojan} && transport∈{ws,grpc,xhttp} && security==tls`(**放开 Trojan-WS/gRPC/XHTTP、VLESS-gRPC/XHTTP**,不再限死 vless-ws)。Reality/SS/HY2/裸TCP + CF → 拒绝。
- 新增:`security==tls` 的非 Reality TLS 协议(trojan/hysteria/vless-tls)在**直连**时要求节点有域名直连地址(cert_domain),纯 IP 直连 → 拒绝(无证书)。
- Reality/SS 在任意地址都放行(免证书)。

## 4. DNS-01 给 CF 域名签自己证书(替代 reuse_direct)

**事实**:橙云域名 HTTP-01 签不出(CF 拦),但 DNS-01 用 CF Token 写 TXT 验证可签真 LE 证书。复用直连证书只在 SSL=Full(非strict) 下凑合,CF 域名是别的域名时就是"拿错证书"。

**实现(agent 侧):**
- `cf_cert_mode='dns01'` 且节点有 cf_domain + CF token:
  - 安装 `certbot-dns-cloudflare` 插件;写 `cloudflare.ini`(0600,含 token)。
  - `certbot certonly --dns-cloudflare --dns-cloudflare-credentials /etc/letsencrypt/cloudflare.ini --non-interactive --agree-tos -m <acme_email> -d <cf_domain>`。
  - CF 入口 inbound 证书路径锚定 **CF 域名自己的** `/etc/letsencrypt/live/{cf_domain}/`(不再指 cert_domain)。
  - 续期:`certbot renew` 用同一 ini 自动续(DNS-01),reload Xray。
- `cf_cert_mode='reuse_direct'`(无 token 兜底):沿用既有"复用 cert_domain 证书 + 文档要求 CF SSL=Full(非strict)"。
- SSL 模式:DNS-01 真证书下 Full(strict) 与 Full 都成立;本方案不依赖改 SSL 模式(token 仅 DNS 权限),文档建议用户面板设 Full(strict) 更安全。

**证书路径锚定(`routing_entry_cert.rs`)**:CF 入口在 `cf_cert_mode=dns01` 时证书锚定 cf_domain;`reuse_direct` 时锚定 cert_domain。订阅 SNI:CF 线仍 = cf_domain。

## 5. 前后端对齐 + 文档

- 后端:迁移加列;node DTO/store/read_model 带 ip_direct_address + cf_cert_mode 默认 dns01 + cf_enabled 派生;install DTO 加 cf_api_token(不落库);agent 证书子系统加 DNS-01 分支;validation 护栏升级;subscription/cert 锚定按 cf_cert_mode。
- 前端:节点表单换 3 行地址(IP/域名/CF)+ 去掉 cf_enabled 开关(CF 地址填了即启用)+ CF token 输入(可选);入口表单地址从节点带出 + 协议按地址过滤。
- 文档:开发方案/README/方案总口径同步 3 地址模型 + DNS-01 证书口径。

## 6. 真机闭环(对齐后)

- 重建控制面到本分支;更新/重建演示节点。
- 验证:① 节点 3 行地址 UI;② 入口协议按地址过滤(IP 只剩 Reality/SS;CF 出现 Trojan-WS);③ **DNS-01 给 cf.example.com 签出它自己的证书 valid**;④ Trojan-WS 经 CF 真实出站(补之前只测 VLESS-WS 的缺口)+ 直连同时在线 + 计费;⑤ 防泄露。
- no-mock,脱敏记日报;token 只在节点本机 ini,验证后清理。

## 6.5 部署任务可清理 + 卡死自愈(避免 cf-closedloop-test 残留)

**问题**:失败/回滚的一键安装会留下 `status=waiting_for_server` 的部署任务,**永远卡住、UI 无法删除**(无 DELETE 路由),堆积成垃圾记录。
**修正:**
- **新增删除/取消端点**:`DELETE /api/admin/deployment-tasks/:id`(管理员删任意任务记录)或 `POST /api/admin/deployment-tasks/:id/cancel`(把未完成任务标记 failed/cancelled)。前端部署任务列表加"删除/取消"按钮。
- **卡死自愈**:`waiting_for_server` 等非终态任务超过阈值(如 `DEPLOY_READY_TIMEOUT` 的合理倍数)未推进 → worker/扫描把它标记为 `failed`(原因"超时未回连"),不再永久 pending。
- 配合 B(部署脚本对空节点不回滚)→ 一键安装新节点不再产生这种卡死任务。

## 7. 非目标(YAGNI)

- 不做 CF 之外 CDN 商;不做 Origin CA(DNS-01 已够);不自动改 CF SSL 模式(token 无此权限,文档提示用户设)。
- ip/域名/CF 三地址做成独立字段,不做"地址池/多 IP"。
