# 节点直连+CF多模式 全协议 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: 用 superpowers:subagent-driven-development 逐任务执行,每任务后两阶段复核(先 spec 合规、再代码质量)。步骤用 `- [ ]`。
> 设计依据:`docs/superpowers/specs/2026-06-22-node-multimode-direct-cf-design.md`(含协议×模式×证书矩阵、CF IP 段、集成点 file:line)。

**Goal:** 单中转节点同时承载直连 + Cloudflare(CF),完整支持 Trojan / VLESS-Reality / HY2 / Shadowsocks 2022 / VLESS-WS,证书与端口按协议自动正确,入口填橙云地址时自动识别为 CF。

**Architecture:** 节点新增"直连证书域名 + 可选 CF 身份(cf_domain,回源复用灰云证书 + CF SSL=Full)";入口按地址是否橙云(dig+CF 段比对)自动判直连/CF,护栏限定 CF 仅 VLESS-WS;计费沿用 Xray Stats 快照差值(协议无关、现成)。

**Tech Stack:** Rust(Axum/SQLx/access-agent/xray-config)+ PostgreSQL 迁移 + Vue3/Element Plus。

**门禁:** 每阶段 `cargo test -p <crate>` 绿;阶段间 `cargo check --workspace`;终态 fmt+clippy(-D warnings)+ 全 workspace 测试(真实 PG)+ 前端 build + 真机闭环。

**容器构建命令(主控集成用):**
```
docker run --rm -v /root/suiyue/XrayC:/work -w /work -e CARGO_TARGET_DIR=/ct -e CARGO_HOME=/ch \
  -v xrayc-cargo-reg2:/ch -v xrayc-cargo-tgt2:/ct rust:1-bookworm bash -c "cargo ..."
```

---

## Phase 1 — DB 迁移:access_nodes 加节点身份/CF 列

**Files:**
- Create: `migrations/202606220001_node_multimode_cf.sql`
- Test: `crates/db/src/tests/` 既有节点测试(加断言新列默认值)

- [ ] **Step 1: 写迁移(加列,不破坏)**
```sql
-- 节点多模式:直连证书域名 + CF 身份。全部可空/带默认,兼容现网 0 数据。
ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS cert_domain   TEXT,
  ADD COLUMN IF NOT EXISTS acme_email    TEXT,
  ADD COLUMN IF NOT EXISTS cf_enabled    BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS cf_domain     TEXT,
  ADD COLUMN IF NOT EXISTS cf_cert_mode  TEXT NOT NULL DEFAULT 'reuse_direct';
-- 约束:cf_cert_mode 仅允许已实现值
ALTER TABLE access_nodes
  ADD CONSTRAINT access_nodes_cf_cert_mode_check
  CHECK (cf_cert_mode IN ('reuse_direct'));
```
- [ ] **Step 2: 集成测试确认迁移可正向应用**:在临时 PG 跑 `sqlx migrate run`,查询 `information_schema.columns` 断言 5 列存在、`cf_enabled` 默认 false、`cf_cert_mode` 默认 'reuse_direct'。
- [ ] **Step 3: commit** `feat: add node multimode/cf columns migration`

## Phase 2 — DB 类型/Store:节点读写带新字段

**Files:**
- Modify: `crates/db/src/types.rs`(AdminAccessNodeInput:112-118、AdminAccessNodeUpdate:120-127 加 cert_domain/acme_email/cf_enabled/cf_domain;节点摘要读取结构)
- Modify: `crates/db/src/store/routing_access_nodes_write.rs`(insert:85-138 带新列)、`routing_nodes.rs`(update:9-63)、节点读 `routing_read.rs`
- Test: `crates/db/src/tests/`(节点 CRUD 集成测试)

- [ ] **Step 1: 失败测试**:`test_create_access_node_persists_cert_and_cf_fields` —— 用含 cert_domain/acme_email/cf_enabled=true/cf_domain 的输入创建节点,读回断言全部落库。
- [ ] **Step 2: 跑测试确认失败**(字段/列未接)。
- [ ] **Step 3: 实现**:types 结构加字段;insert/update SQL 带新列绑定;节点读模型 select 出新列。保持 Option/默认兼容老数据。
- [ ] **Step 4: 跑测试通过**。
- [ ] **Step 5: 负向**:不传 CF 字段时 cf_enabled=false、cf_domain 空,创建成功。
- [ ] **Step 6: commit** `feat: persist node cert_domain and cf fields`

## Phase 3 — API DTO/Handler:节点创建/更新/安装指南带新字段

**Files:**
- Modify: `crates/api/src/dto.rs`(CreateAccessNodeRequest:84-91、节点 update DTO 加 cert_domain/acme_email/cf_enabled/cf_domain)
- Modify: `crates/api/src/admin_nodes.rs`(create:21-108、update:133-185 透传)
- Modify: `crates/api/src/deploy.rs`(resolve_agent_install_guide_request:264-338:把节点 cert_domain 并入 tls_cert_domains,使 certbot 申请直连证书)
- Test: `crates/api/`(handler 测试 / axum-test)

- [ ] **Step 1: 失败测试**:`test_create_node_accepts_cf_fields` 经 handler 创建带 CF 字段节点,DB 落库;`test_install_guide_includes_cert_domain` 断言安装指南 tls_cert_domains 含节点 cert_domain。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**:DTO 加字段→store input;安装指南 resolve 时若节点 cf_enabled 且 cert_domain 非空,确保 cert_domain ∈ tls_cert_domains。
- [ ] **Step 4: 通过**。
- [ ] **Step 5: commit** `feat: node cf fields in api + cert_domain into install guide`

## Phase 4 — 入口端口冲突校验(高风险补齐)

**Files:**
- Modify: `crates/db/src/store/routing_access_entries.rs`(create_admin_access_entry:263-322,prepare 阶段加校验)
- Test: `crates/db/src/tests/`(入口集成测试)

- [ ] **Step 1: 失败测试**:`test_create_entry_rejects_duplicate_port_on_same_node` —— 同 access_node_id 已有 enabled 入口占 8443,再建 8443 入口应 `Err`(DbError 含"端口已被占用");不同节点同端口允许;停用入口不算占用。
- [ ] **Step 2: 确认失败**(当前无检测,会成功)。
- [ ] **Step 3: 实现**:写入前 `SELECT 1 FROM access_entries WHERE access_node_id=$1 AND listen_port=$2 AND enabled=true [AND id<>$自身]`;命中返回错误。更新入口换端口同样校验。
- [ ] **Step 4: 通过**。
- [ ] **Step 5: commit** `feat: per-node listen_port conflict validation`

## Phase 5 — 橙云自动识别 + 协议护栏

**Files:**
- Create: `crates/db/src/cloudflare_ranges.rs`(bundle CF IPv4/IPv6 CIDR 常量 + `fn is_cloudflare_ip(ip) -> bool`,源见 spec §4)
- Modify: `crates/db/src/store/routing_access_entries.rs`(prepare:66-147 加 `detect_cdn_mode`)与 `crates/db/src/validation.rs`(加 `validate_protocol_cdn_combination`)
- Test: `crates/db/src/`(纯函数单测 + 入口集成测试)

- [ ] **Step 1: 失败测试(纯函数)**:`test_is_cloudflare_ip` —— `104.16.0.1`/`172.67.0.5`/`188.114.96.1` → true;`203.0.113.10`/`8.8.8.8` → false。
- [ ] **Step 2: 实现** cloudflare_ranges.rs:CIDR 解析 + 成员判断(IPv4 用 u32 掩码,IPv6 用 u128)。通过。
- [ ] **Step 3: 失败测试(护栏)**:`test_cf_entry_requires_vless_ws` —— cdn_enabled=true 且 protocol∈{trojan,hysteria,shadowsocks} 或 reality → `Err`("该协议不能过 CF,请用 VLESS-WS 或改用灰云/IP");vless+ws+tls+cdn → Ok。
- [ ] **Step 4: 实现** validate_protocol_cdn_combination,接入 prepare_access_entry。通过。
- [ ] **Step 5: 识别接入(控制面 dig)**:入口保存时,若 listen_host/cdn_hostname 是域名,控制面解析其 A 记录并 `is_cloudflare_ip` 判定→命中则置 cdn_enabled/cdn_provider=cloudflare(识别即预填,管理员可覆盖;dig 失败不阻断,仅不自动开)。说明:dig 在 api 层做(db 层不联网),把判定结果作为入参传入 prepare。新增 api helper `resolve_is_cloudflare(host)`。测试:mock 解析结果注入,验证置位逻辑(不在单测真连网)。
- [ ] **Step 6: commit** `feat: cloudflare orange-cloud auto-detect + cf protocol guardrail`

## Phase 6 — CF 入口证书复用 + 订阅 CF 线生成

**Files:**
- Modify: `crates/db/src/store/routing_access_entries.rs` / `routing_entry_reality.rs`(server_name_for_entry:22-40 与 cert 路径:CF 入口 cert 用节点 cert_domain,而非 cdn_hostname)
- Modify: `crates/db/src/validation.rs`(normalize_access_inbound_config:302-340:CF 入口证书路径锚定 cert_domain)
- Modify: `crates/core/src/subscription/proxy.rs`(68-149/185-232/320-345:CF 入口订阅 server=cf_domain、sni=cf_domain、ws;直连 server=直连地址、sni=cert_domain)
- Test: `crates/db/`、`crates/core/src/subscription/`(订阅单测)

- [ ] **Step 1: 失败测试**:`test_cf_entry_cert_anchored_to_node_cert_domain` —— CF VLESS-WS 入口编译后 inbound 证书路径 = `/etc/letsencrypt/live/{cert_domain}/fullchain.pem`(不是 cf_domain)。
- [ ] **Step 2: 失败测试(订阅)**:`test_subscription_cf_line_uses_cf_domain_server` —— CF 入口订阅条目 server=cf_domain、sni=cf_domain、network=ws;`test_subscription_direct_line_uses_cert_domain_sni` —— 直连 Trojan/HY2/VLESS-WS 订阅 sni=cert_domain。
- [ ] **Step 3: 确认失败**。
- [ ] **Step 4: 实现**:证书路径来源在 CF 模式锚定节点 cert_domain;订阅 server/sni 分直连/CF 取值。Reality 仍借用 dest SNI。
- [ ] **Step 5: 通过 + 负向**:非 CF 入口订阅不变(回归既有断言)。
- [ ] **Step 6: commit** `feat: cf entry reuse direct cert + cf subscription line`

## Phase 7 — 前端:节点 CF 字段 + 入口橙云识别提示 + CDN 子组件拆分

**Files:**
- Modify: `frontend/src/views/access-lines/CreateAccessNodeDialog.vue`(加 cert_domain/启用CF/cf_domain;启用CF 时 cert_domain 必填提示)
- Modify: `frontend/src/views/access-lines/OneClickAgentInstallDialog.vue`(ACME 区块旁加 CF 字段)
- Modify: `frontend/src/views/access-entries/AccessEntryDialog.vue`(选节点联动:节点 cf_enabled 时提示;地址橙云识别后标注"已识别为 CF 模式")
- Create: `frontend/src/views/access-entries/EntryCdnFields.vue`(把 AccessEntriesPage/Dialog 的 CDN 区块拆出,给 `AccessEntriesPage.vue`(501 行)降负到 ≤500)
- Modify: `frontend/src/services/api/types/routing.ts`、`normalizers/routing.ts`、`clients/routing.ts`(节点/入口新字段映射)
- Test: 前端 `npm run build` + 既有 e2e smoke 不回归

- [ ] **Step 1**: 类型与 normalizer 加节点 cfEnabled/cfDomain/certDomain、payload 映射(types/normalizers/clients)。
- [ ] **Step 2**: 节点表单加字段 + 校验(启用CF→cert_domain 必填、cf_domain 必填)。
- [ ] **Step 3**: 拆出 EntryCdnFields.vue,AccessEntriesPage.vue 引用,确认行数回落 ≤500(`scripts/check-source-file-length.sh`)。
- [ ] **Step 4**: 入口 Dialog 橙云识别提示(后端返回识别结果回显)。
- [ ] **Step 5**: `npm run build` 通过 + e2e smoke 调整(节点/入口表单步骤)。
- [ ] **Step 6: commit** `feat: frontend node cf fields + entry cf detect + split cdn fields`

## Phase 8 — 部署侧/文档:CF Full 约束 + env

**Files:**
- Modify: `deploy/access-agent/access-agent.env.example`(注释 CF 模式说明;cert_domain 进 XRAYC_TLS_CERT_DOMAINS)
- Modify: `开发方案.md`、`AGENTS.md`、`CLAUDE.md`、README、当前方案总口径:登记"节点直连+CF多模式、CF 回源复用灰云证书 + CF SSL=Full、Origin CA/DNS-01 为后续"。
- 门禁:`make check-docs-no-stale` 等效通过。
- [ ] commit `docs: node multimode/cf operating guidance`

## 最终集成门禁(主控)

- [ ] `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] 全 workspace 测试(真实 PG 集成)0 failed
- [ ] 前端 `npm run build` + `check-source-file-length`
- [ ] `make rebuild` 起栈 + `check-running-current`(仅 postgres/api/worker/caddy)

## Phase 9 — 真机闭环测试(no-mock,全真)

> 详见 spec §8。在节点A(…132,灰云根域 G)真机部署。

- [ ] 重读最新私有清单;一键安装 access-agent 到 …132,填 cert_domain=G 灰云域名 + ACME 邮箱 → certbot HTTP-01 签直连证书成功。
- [ ] 建 5 个直连入口:Reality(443)、Trojan、HY2、Shadowsocks、VLESS-WS(各自端口,端口冲突校验生效)。
- [ ] 建 1 个 CF 入口:VLESS-WS,地址填 G 下一个橙云子域(回源到 …132,CF SSL=Full);系统自动识别为 CF 模式;cert 复用 G 灰云证书。
- [ ] 真实客户端(mihomo,支持全 5 协议)导入订阅,逐协议真实出站:出口地址符合预期、流量上报按快照差值扣费、`/metrics`+`/sessions`+`/probes` 真实、运营中心展示。
- [ ] CF 线经 Cloudflare 真实代理可用(客户端连橙云子域→CF→…132)。
- [ ] 清理临时用户/入口/配置;脱敏记日报(覆盖 N 协议、CF 真实链路、计费数值、失败原因)。
- [ ] **外部依赖**:CF 控制台需配 橙云子域 + 回源 …132 + SSL=Full(我无 CF API,执行前与用户确认/索取 Token 或由用户在 CF 后台完成)。
