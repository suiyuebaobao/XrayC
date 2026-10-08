# 中转节点「直连 + CF」多模式 + 全协议 设计方案

> 状态:设计稿(代码集成点待 4 个调研 agent 回填后定稿)。脱敏文档,禁写真实 IP/域名/凭据。

**Goal:** 让单个中转节点同时承载「直连」和「Cloudflare(CF)」两种接入方式,并完整支持 5 种协议(Trojan、VLESS-Reality、HY2、Shadowsocks 2022、VLESS-WS),证书与端口按协议/模式自动正确处理,管理员在入口填地址时系统自动识别橙云→CF。

**Architecture(一句话):** 节点提供"对外身份与证书基础设施"(直连地址 + 可选直连证书 + 可选 CF 身份),入口按协议挂到对应身份;直连 vs CF 由入口地址是否橙云自动判定;计费/限速沿用既有 Xray Stats 快照差值,与协议无关。

**Tech Stack:** Rust(Axum/SQLx/access-agent/xray-config)+ PostgreSQL 迁移 + Vue3/Element Plus + Xray 单内核 + certbot(HTTP-01)/ Cloudflare Origin CA 或 DNS-01。

---

## 1. 核心事实与约束(设计前提)

1. **CF 只代理 HTTP(S)**:只有 WS / gRPC / XHTTP + TLS 能过 CF;Reality、HY2(UDP/QUIC)、裸 TCP Trojan、SS 不能过 CF,只能直连。
2. **客户端 ↔ CF 的证书由 Cloudflare 自己出**(Universal SSL),节点不签;节点只需管 **CF 回源证书**(CF↔节点那一跳)。
3. **橙云域名节点无法用 certbot HTTP-01 签证书**(challenge 被 CF 拦);CF 回源证书只能走 **Cloudflare Origin CA**(免费,CF 签发,装节点)或 **certbot DNS-01**(需 CF API Token);若 CF SSL 设 **Full(非 strict)** 则可用节点**自签证书**(回源段在 CF↔节点之间,内层 VLESS/Trojan 仍鉴权)。
4. **灰云域名 / IP 直连**:certbot HTTP-01 能签(灰云 DNS 直指节点 IP)。
5. **协议对证书的强制**(以代码 `validation.rs` 现状为准):Trojan、HY2 强制 TLS + 有效证书;VLESS-WS 走 TLS 时要证书;Reality 借用第三方证书、**不签自有证书**;SS 自带加密、无 TLS。
6. **计费只看流量、与协议无关**:沿用 Xray Stats API 按 user email 的快照差值入账,多 inbound 都纳入统计;本设计不改计费内核。
7. **私有清单事实(脱敏)**:灰云根域 G→节点A(尾段…132,真实 IP,DNS 已实测灰云);橙云子域 O→CF 段(DNS 已实测橙云,回源 IP 不可见);节点C(…69)有未知现役部署、节点D(…123)为生产,默认不碰。

## 2. 协议 × 模式 × 证书 矩阵(实现与校验的总口径)

| 协议 | 直连 | 过 CF | 需证书 | 用哪张证书 | 默认端口 |
|---|---|---|---|---|---|
| VLESS-Reality | ✅ | ❌ | ❌ | 无(借用 dest 证书) | 443 |
| Shadowsocks 2022 | ✅ | ❌ | ❌ | 无(自带加密) | 自定义 |
| Trojan | ✅(raw TLS) | ❌ | ✅ | 直连证书 | 443/自定义 |
| HY2 | ✅ | ❌ | ✅ | 直连证书 | 自定义(UDP) |
| VLESS-WS | ✅ | ✅ | ✅ | 直连→直连证书 / CF→CF 回源证书 | 直连自定义 / CF 用 CF 端口 |

**CF 支持端口**:443 / 2053 / 2083 / 2087 / 2096 / 8443(HTTPS);CF 入口必须落其中之一。

**SNI 规则**:
- Reality:SNI = 借用域名(dest),server 可填 IP 或灰云域名(仅作连接地址)。
- Trojan / HY2 / VLESS-WS 直连:SNI = **直连证书域名**(灰云),证书路径 `/etc/letsencrypt/live/{直连证书域名}/`。
- VLESS-WS 过 CF:client SNI = CF 橙云域名(看 CF 边缘证书);节点回源 inbound 证书 = CF 回源证书。

## 3. 节点模型(node 级)

节点新增/明确以下"对外身份"配置:

1. **直连地址**:`direct_ip`(可选)+ `direct_domain`(可选,灰云)。至少一个;两个都填则订阅可下发两条直连线(IP 一条、域名一条)。
2. **直连证书域名** `direct_cert_domain`(灰云域名)+ `acme_email`:**当本节点存在 Trojan/HY2/VLESS-WS 直连入口时必填**;access-agent 用 certbot HTTP-01 自动签、续期、reload。纯 Reality/SS 节点可不填(0 证书、0 域名)。
3. **启用 CF** `cf_enabled`(开关):
   - 开 → `cf_domain`(橙云)+ `cf_cert_mode`(`origin_ca` 手动粘贴 / `dns01` 用 CF Token / `self_signed` 配合 CF Full 非 strict)。CF 回源证书据此获得/装载。
   - 关 → 节点纯直连。
4. **老节点兼容**:`cf_enabled` 默认关、新字段可空;不影响现有纯直连/纯 IP 节点。迁移走加列(非删列),不破坏现网 0 数据。

## 4. 入口模型(entry 级)+ 橙云自动识别

1. **协议选择**:5 协议下拉。
2. **地址自动识别(关键 UX)**:管理员在入口填"对外地址"时,控制面后台 `dig` 该地址并比对 **Cloudflare IP 段**:
   - 命中 CF 段 → 识别为 **CF 模式**:自动置 `cdn_enabled=true`、`cdn_provider=cloudflare`、**限定协议必须 CF 兼容(VLESS-WS;否则报错拦截)**、证书来源切 CF 回源证书、端口限定 CF 支持端口。UI 明确标注「已识别为 CF 模式」。
   - 未命中(IP / 灰云域名)→ **直连模式**:按协议决定证书(Reality/SS 免;Trojan/HY2/WS 用节点直连证书)。
   - **识别结果可被管理员手动覆盖**(DNS / 橙灰状态会变,不做纯静默强切;以"识别即预填 + 可改 + 保存即定"为准)。
   - **CF IP 段比对源**(官方 `https://www.cloudflare.com/ips/`,bundle 进控制面并可定期刷新):
     - IPv4:`173.245.48.0/20 103.21.244.0/22 103.22.200.0/22 103.31.4.0/22 141.101.64.0/18 108.162.192.0/18 190.93.240.0/20 188.114.96.0/20 197.234.240.0/22 198.41.128.0/17 162.158.0.0/15 104.16.0.0/13 104.24.0.0/14 172.64.0.0/13 131.0.72.0/22`
     - IPv6:`2400:cb00::/32 2606:4700::/32 2803:f800::/32 2405:b500::/32 2405:8100::/32 2a06:98c0::/29 2c0f:f248::/32`
3. **护栏(硬校验,落 `validation.rs`)**:
   - CF 模式 + 非 VLESS-WS 协议 → 拒绝(`Trojan/HY2/Reality/SS 不能过 CF`)。
   - CF 模式 → 不得走 HTTP-01;证书指向节点 CF 回源证书。
   - 直连 TLS 协议(Trojan/HY2/WS)→ 节点必须已配 `direct_cert_domain`,否则拒绝并提示去节点补域名。
4. **同节点混挂**:节点可同时有"直连入口"与"CF 入口"(不同 inbound/端口),天然实现"一台节点两种模式"。MVP 以"一入口一模式(按地址自动判定)"为准;"同一入口同时下发直连+CF 两条线"列为后续增强,不阻塞 MVP。

## 5. 证书策略(MVP 精简 + 后续路线)

**调研事实**:agent 现仅支持 certbot **HTTP-01/Webroot**(`crates/access-agent/src/runtime/tls.rs`、`scripts/lib/deploy-access-agent/tooling.sh:182` `install_tls_certificates`),**无 DNS-01/Origin CA/CF**;证书路径 `/etc/letsencrypt/live/{域名}/`,由入口 server_name 推导(`validation.rs:318-338`)。

**MVP 决策(YAGNI,直接可真机闭环,零新证书签发机制)**:
- **直连证书**:节点一张灰云域名 LE 证书,沿用现有 certbot **HTTP-01**(已工作)。所有直连 TLS 入口(Trojan/HY2/VLESS-WS 直连)共用它,SNI = 灰云证书域名。Reality/SS 不吃证书。
- **CF 回源证书 = 复用上面这张灰云 LE 证书**,CF 控制台 SSL 设 **Full(非 strict)**:
  - 客户端 ↔ CF:看 CF 边缘证书(CF 自动管,节点不签)。
  - CF ↔ 节点:节点 CF 入口出示灰云 LE 证书,Full(非 strict)不校验主机名/CA → 接受。
  - 因此 **MVP 不需要 Origin CA / DNS-01 / CF API / 自签**,也能端到端跑通真实 CF 流量。
  - 实现上:CF 入口的证书路径指向节点灰云证书(即 cert 用节点 `cert_domain`,而对外公布 `cdn_hostname=cf_domain`)。
- **前置约束(写入文档/UI 提示)**:启用 CF 的节点必须已有直连灰云证书(即必须配 `cert_domain` 并签发成功);CF 控制台需 橙云 + 回源到节点 + SSL=Full。
- 证书路径以"写入侧(agent 落盘 + xray inbound 引用)为唯一真相",真机验证证书文件存在且被 Xray 加载(对齐 no-mock 红线)。

**后续路线(不在 MVP,文档登记)**:CF **Origin CA**(管理员粘贴 CF 源站证书,可设 Full strict)、**DNS-01**(CF API Token,certbot dns-cloudflare,可给橙云域名签真 LE 证书)。届时按调研挂载点 A–H 扩展 DTO/脚本/agent/DB/前端。

## 6. agent 运行态:一节点多入口 / 多端口 / 多证书

- 一个节点的多条入口编译成多个 Xray inbound:Reality(443)、WS-TLS 直连(自定义端口)、WS-TLS-CF 回源(CF 端口如 8443)、Trojan、HY2、SS 各自端口。
- **端口冲突校验**:同节点入口端口唯一;443 只能给一个(通常 Reality 或直连 WS);CF 入口落 CF 支持端口。校验在保存入口时做。
- WS-TLS 直连与 CF 回源若共用一个 inbound(同端口、按 SNI 出不同证书)为可选优化;MVP 可各自独立 inbound/端口,先简单后优化。
- 多证书:同一 Xray TLS inbound 支持多 certificate 条目按 SNI 选择;直连证书与 CF 回源证书分别注入对应 inbound。
- reload 后确认 Xray 实际加载新证书与新端口(运行态校验,非仅写文件)。

## 7. 订阅生成

- **直连线**:`server = direct_ip 或 direct_domain`,`sni = direct_cert_domain`(Reality 为借用域名),`port = 入口端口`,transport 按协议。
- **CF 线**:`server = cf_domain`,`sni = cf_domain`,`port = CF 端口`,transport = WS。
- 仍只暴露中转入口参数,绝不输出出口真实地址/凭据(沿用既有防泄露红线)。

## 8. 测试拓扑(真实闭环,全真)

- **节点A(…132,灰云根域 G)**:主测节点。部署 5 协议直连入口(Reality 443、Trojan、HY2、SS、VLESS-WS 直连),用 G 灰云域名 certbot HTTP-01 签直连证书。
- **CF 闭环**:在 G 下新建一个橙云子域(CF 后台/或 Token)回源到 …132,VLESS-WS 走 CF;CF 回源证书用 self_signed+Full 或 Origin CA。**不动 …123 生产 / …69 未知部署**;若必须用 …123 实测 CF,需用户显式授权。
- **验收**:每协议真实客户端真实出站、出口地址符合预期、流量上报并按快照差值扣费、`/metrics`+`/sessions`+`/probes` 真实、运营中心展示;CF 线经 Cloudflare 真实代理可用。no-mock,失败如实记日报。
- **CF 侧外部依赖**:CF dashboard 配置(橙云、回源 IP、SSL 模式)或 CF API Token,执行真实 CF 测试前与用户确认。

## 9. 数据迁移(方案:加列,非破坏)

- `access_nodes` 新增:`direct_ip`、`direct_domain`、`direct_cert_domain`、`acme_email`(若无)、`cf_enabled`、`cf_domain`、`cf_cert_mode`、CF 回源证书引用/状态列。
- `access_entries`:沿用 `cdn_enabled/cdn_hostname/cdn_provider`,新增"识别来源/模式锁定"标记(区分自动识别 vs 手动覆盖)。
- 全部可空 + 默认值,兼容现网。实时库当前 0 CF 数据,无数据迁移。

## 10. 非目标(YAGNI)

- 不做 CF 之外的 CDN 商(provider 仍只 cloudflare)。
- 不做自动改 CF DNS 记录/橙灰切换(除 DNS-01 签证书所需的 TXT,其余 CF 编排留人工/后续)。
- "同一入口同时下发直连+CF 两条线"为后续增强,不在 MVP。

---

## 集成点(调研回填)

### 入口/校验/订阅 ✅
- `migrations/202606120001_entry_management_binding_nodes.sql:4-34` — access_entries 表(cdn_enabled/provider/hostname/server 4 列)。
- `crates/db/src/store/routing_access_entries.rs:78-90` — cdn_hostname 覆盖 listen_host;`:324-431` 绑定出口自动生成 access_lines。**新增挂载点**:`prepare_access_entry` 加 `detect_cdn_mode()`(dig+比对 CF 段);`validate_protocol_cdn_combination()`。
- `crates/db/src/store/routing_entry_reality.rs:22-40` — server_name_for_entry;`:86-222` Reality 自动生成 key/short_id/dest,禁 UDP/WS。
- `crates/db/src/validation.rs:231-244` 协议白名单;`:259-279` 协议×传输矩阵;`:302-406` TLS 自动填证书路径 + Trojan/HY2 强制 TLS(**护栏挂载点**)。
- `crates/core/src/subscription/proxy.rs:68-149` 各协议字段映射;`:185-232` 可订阅条件;`:320-345` security/server_name 推断。**新增**:直连/CF 分化(server/sni 来源)。
- 现状:CDN 层纯存储、无 dig/识别;订阅层不区分直连/CF(仅 listen_host 值不同)。

### 前端 ✅
- 节点表单:`frontend/src/views/access-lines/CreateAccessNodeDialog.vue`(62)、`OneClickAgentInstallDialog.vue`(163,已有 tlsCertDomains+acmeEmail @122-129)、`RelayNodeCard.vue`(159,TLS 续期)。
- 入口表单:`frontend/src/views/AccessEntriesPage.vue`(**501,触基准,改动需拆子组件**)、`access-entries/AccessEntryDialog.vue`(306,CDN 区块 @269-292)。
- 类型/映射:`services/api/types/routing.ts`(475,近上限)、`normalizers/routing.ts`(489,近上限)、`clients/routing.ts`(371)。
- **新增挂载点**:节点表单加 `cf_enabled`+`direct_*`+`cert_domain`;入口选节点联动橙云识别提示;normalizer/payload 同步;RelayNodeCard 加 CF 状态标签。CDN 管理建议拆出子组件以让 AccessEntriesPage 降到基准内。

### 节点/证书 ✅
- DTO:`crates/api/src/dto.rs` CreateAccessNodeRequest(:84-91,仅 name/public_host/public_port/ssh_host/agent_token/remark,**无域名/证书字段**)、AgentInstallGuideRequest(:93-116,含 tls_cert_domains/acme_email)、OneClickAgentInstallRequest(:119-150)。
- Handler:`crates/api/src/admin_nodes.rs`(create:21-108、update:133-185 仅基础字段、renew_tls:195-223)、`crates/api/src/deploy.rs`(install guide / one-click / resolve:264-338 推断域名+规范化 tls_cert_domains)。
- 证书流:tls_cert_domains → env `XRAYC_TLS_CERT_DOMAINS` → `tooling.sh:182 install_tls_certificates` → certbot `--standalone`(HTTP-01)→ `/etc/letsencrypt/live/{域名}/`。**节点创建不入库域名**;仅安装指南阶段组织 + 节点 env 持有。
- agent 证书:`crates/access-agent/src/runtime/tls.rs`(collect_tls_targets:35-71 从 env+server_name 采集、certificate_report:100-138 openssl 读有效期、execute_tls_renew_task:187-222 certbot renew、reload:234-247)。`config.rs:146` tls_cert_domains/certbot_binary/openssl_binary。
- DB:access_nodes 表有 tls_certificates JSONB + tls_renew_*(迁移 202606110002),**无 cert_domain/cf_* 列**。store `routing_access_nodes_write.rs`、`routing_nodes.rs`、`agent.rs`(record_agent_tls_status/request_access_node_tls_renewal)。
- **新增挂载点(MVP)**:access_nodes 加列 `cert_domain`、`acme_email`、`cf_enabled`、`cf_domain`、`cf_cert_mode`(默认 reuse_direct);CreateAccessNodeRequest/Update + store + handler 带上;CF 入口 cert 路径指向节点 cert_domain。

### agent 运行态 ✅
- 编译:`crates/xray-config/src/lib.rs:32-111` compile_xray_config(**已支持多 inbound 循环**);`inbound.rs:16-106` 单入口编译、`:272-358` transport+security(Reality 仅 tcp/xhttp/grpc)、`:360-392` apply_inbound_tls_settings(从 line.tls_certificate_file/tls_key_file 注入,单证书)、`:89-93` listen 0.0.0.0 + listen_port、`:460` tag `access-{line_id}`。
- 应用:`crates/access-agent/src/runtime/apply.rs`(apply_node_runtime_config、apply_rendered_runtime_config:149-224 写临时→`xray run -test`→覆盖→reload→回滚、reclaim_stale_xray_instances:226-255 **仅清 Stats 端口**)。
- **端口冲突**:`routing_access_entries.rs:263 create_admin_access_entry` 直接写 listen_port,**无冲突检测**(最高风险)→ 同节点同端口 = Xray bind 失败 = reload 回滚。
- 统计:`stats.rs:24-60` statsquery user>>>、`xray-config/src/stats.rs:15` email `xrayc-line-{line_id}--{key}`、`metrics.rs:41-104` 快照差值。**协议无关、多 inbound 自动纳入、限速按 user 共享**(无需改)。
- **新增挂载点(MVP)**:① 入口保存时端口冲突校验(同节点 enabled 入口端口唯一,443 仅一个);② CF 入口 cert 指向节点 cert_domain 证书;③ CF 入口落 CF 端口(443/8443/2053/2083/2087/2096)。多证书数组/reclaim 多端口/Origin CA 列为后续。

## MVP 范围(YAGNI 收敛,先可真机闭环)

**做**:① 节点加 CF 字段(cert_domain/acme_email/cf_enabled/cf_domain/cf_cert_mode=reuse_direct)+ 迁移加列;② 入口保存端口冲突校验;③ 入口橙云自动识别(dig+CF 段比对)+ 护栏(CF 仅 VLESS-WS);④ CF 回源复用灰云证书 + 文档要求 CF SSL=Full;⑤ 订阅 CF 线下发;⑥ 5 协议直连(多为现成,验证补缺)。
**不做(后续)**:Origin CA / DNS-01 / CF API、per-domain 证书方法、同入口双线、CF DNS 自动编排、多证书数组。
