# 节点3地址 + 协议按地址过滤 + DNS-01 CF证书 + 部署任务可清理 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: 用 superpowers:subagent-driven-development 逐任务执行,两阶段复核。步骤用 `- [ ]`。
> 设计依据:`docs/superpowers/specs/2026-06-22-node-3addr-protocol-filter-dns01-design.md`。

**Goal:** 节点对外身份摆成 3 行明确地址(IP直连/域名直连/CF),入口协议按地址类型过滤,CF 放开 WS/gRPC/XHTTP 全族,CF 域名用 DNS-01 签自己的证书,部署任务可删/可取消、卡死自愈;前后端对齐 + 真机闭环。

**Architecture:** access_nodes 加 ip_direct_address + cf_cert_mode(dns01);cf_enabled 派生(cf_domain 非空)。CF Token 仿 SSH 凭据只安装期用、写节点本机 ini、不落库。agent 按 cf_cert_mode 走 DNS-01(certbot-dns-cloudflare)或 reuse_direct。校验护栏放开 CF 到 (vless|trojan)+(ws|grpc|xhttp)+tls。部署任务加 DELETE/cancel + 卡死超时标 failed。

**Tech Stack:** Rust(Axum/SQLx/access-agent)+ PostgreSQL 迁移 + Vue3 + certbot(HTTP-01/DNS-01-cloudflare)。

**门禁:** 每阶段 `cargo test -p <crate>` 绿;终态 fmt+clippy(-D warnings)+全 workspace 测试(真实 PG)+前端 build+真机闭环。容器命令同前(`rust:1-bookworm bash -c "export PATH=/usr/local/cargo/bin:\$PATH; ..."`,DATABASE_URL=pgdev,共享库串行)。

---

## Phase 1 — DB 迁移:节点 3 地址 + cf_cert_mode dns01

**Files:** Create `migrations/202606220003_node_three_address_dns01.sql`;Test `crates/db/src/tests/`。

- [ ] **Step 1 迁移**:
```sql
ALTER TABLE access_nodes ADD COLUMN IF NOT EXISTS ip_direct_address TEXT;
ALTER TABLE access_nodes DROP CONSTRAINT IF EXISTS access_nodes_cf_cert_mode_check;
ALTER TABLE access_nodes ADD CONSTRAINT access_nodes_cf_cert_mode_check
  CHECK (cf_cert_mode IN ('reuse_direct','dns01'));
-- 历史:cf_domain 非空但 cf_cert_mode 仍 reuse_direct 的保留(无 token 兜底);新节点默认 dns01 由应用层决定。
```
- [ ] **Step 2 集成测试**:断言 ip_direct_address 列存在、cf_cert_mode CHECK 含 dns01(查 information_schema + 插一条 dns01 成功、插非法值失败)。
- [ ] **Step 3 commit** `feat: migration node ip_direct_address + cf_cert_mode dns01`

## Phase 2 — DB 类型/store/读模型:3 地址 + cf_enabled 派生

**Files:** Modify `crates/db/src/types.rs`(AdminAccessNodeInput/Update 加 ip_direct_address、cf_cert_mode)、`store/routing_access_nodes_write.rs`、`routing_nodes.rs`、`store/rows/topology.rs`(AccessNodeRow+ip_direct_address)、`store/load.rs`(SELECT+构造)、`read_models.rs`(JSON)、`crates/core/src/model.rs`(AccessNode+ip_direct_address);Test `crates/db/src/tests/`。

- [ ] **Step 1 失败测试** `test_node_persists_ip_direct_and_cf_cert_mode_and_derives_cf_enabled`:建节点带 ip_direct_address + cf_domain(不显式传 cf_enabled)→ 读回 ip_direct_address 落库、**cf_enabled 自动 true**(cf_domain 非空派生)、cf_cert_mode 默认 dns01;再建只填 ip(无 cf_domain)→ cf_enabled=false。
- [ ] **Step 2 确认失败**。
- [ ] **Step 3 实现**:input/row/model/SELECT/JSON 加 ip_direct_address;写入时 `cf_enabled = !cf_domain.trim().is_empty()`(store 派生,忽略入参 cf_enabled 或以派生为准);cf_cert_mode 默认值:有 cf_domain 时 dns01,否则保持。read_models 节点 JSON 加 ip_direct_address + cf_cert_mode。
- [ ] **Step 4 通过 + 负向**。
- [ ] **Step 5 commit** `feat: node 3-address model + derived cf_enabled`

## Phase 3 — API DTO/Handler + 安装 DTO 加 cf_api_token(不落库)

**Files:** Modify `crates/api/src/dto.rs`(CreateAccessNodeRequest/Update + ip_direct_address、cf_cert_mode;OneClickAgentInstallRequest/AgentInstallGuideRequest + cf_api_token)、`admin_nodes.rs`、`deploy_install.rs`(resolve 透传 cf_api_token→env,**不入 AdminAccessNodeInput、不落库**)、`deploy.rs`;Test `crates/api/`。

- [ ] **Step 1 失败测试** `test_node_api_accepts_ip_direct_and_cf_cert_mode`(handler 建/读回)+ `test_one_click_cf_api_token_goes_to_env_not_db`(resolve 后 cf_api_token 进 agent env `XRAYC_CLOUDFLARE_API_TOKEN`,DB 节点无 token 字段)。
- [ ] **Step 2 确认失败**。
- [ ] **Step 3 实现**:DTO 加字段;install resolve 把 cf_api_token 注入 `agent_install_envs` 的 `XRAYC_CLOUDFLARE_API_TOKEN`(脱敏,不落库/不入审计/不回显);节点 cf_cert_mode 默认 dns01(有 cf_domain)。
- [ ] **Step 4 通过**。
- [ ] **Step 5 commit** `feat: cf_api_token transient into agent env + node ip/cert_mode in api`

## Phase 4 — 校验护栏升级(协议×地址×CF)

**Files:** Modify `crates/db/src/validation.rs`(validate_protocol_cdn_combination + 新增直连TLS需域名);接入 prepare_access_entry。若 routing_access_entries.rs 逼近 550 抽子模块;Test `crates/db/src/tests/`。

- [ ] **Step 1 失败测试**:
  - `test_cf_allows_vless_and_trojan_ws_grpc_xhttp`:cdn + trojan + ws + tls → Ok;cdn + vless + grpc + tls → Ok;cdn + vless + tcp → Err;cdn + reality → Err;cdn + shadowsocks → Err。
  - `test_direct_tls_requires_domain_not_ip`:security=tls 的 trojan/hysteria/vless-tls,listen_host 是纯 IP 且节点无 cert_domain → Err;有域名直连地址 → Ok;reality/ss 纯 IP → Ok。
- [ ] **Step 2 确认失败**(当前护栏只放 vless-ws,且无 IP 限制)。
- [ ] **Step 3 实现**:CF 规则改 `protocol∈{vless,trojan} && transport∈{ws,grpc,xhttp} && security==tls`;新增 `validate_tls_protocol_needs_domain`(security=tls 且非 reality 时要求有域名锚点,纯 IP 拒)。
- [ ] **Step 4 通过 + 回归**(已有 vless-ws-cf 测试仍绿)。
- [ ] **Step 5 commit** `feat: cf guardrail open ws/grpc/xhttp + direct tls requires domain`

## Phase 5 — agent DNS-01 给 CF 域名签证书

**Files:** Modify `crates/access-agent/src/config.rs`(加 cloudflare_api_token、cf_cert_mode、cf_domain env)、`runtime/tls.rs`(DNS-01 分支);部署脚本 `scripts/lib/deploy-access-agent/tooling.sh`(install_tls_certificates 加 dns-cloudflare 分支 + 写 0600 ini);Test `crates/access-agent/src/`。

- [ ] **Step 1 失败测试(纯函数)** `test_certbot_dns01_args_for_cf_domain`:给 cf_cert_mode=dns01 + cf_domain + token,构造的 certbot 命令含 `--dns-cloudflare --dns-cloudflare-credentials <ini> -d <cf_domain>`;reuse_direct 时不构造 DNS-01。
- [ ] **Step 2 确认失败**。
- [ ] **Step 3 实现**:agent 证书目标按 cf_cert_mode 选签法;DNS-01 写 token 到 0600 ini(`/etc/letsencrypt/cloudflare.ini`,不日志/不回显),`certbot certonly --dns-cloudflare ... -d cf_domain`;续期 `certbot renew` 复用 ini;心跳上报 cf_domain 证书状态(脱敏)。脚本 tooling.sh 同步加 dns-cloudflare 分支。
- [ ] **Step 4 通过**。
- [ ] **Step 5 commit** `feat: agent dns-01 cloudflare cert for cf domain`

## Phase 6 — CF 入口证书锚定按 cf_cert_mode + 订阅

**Files:** Modify `crates/db/src/store/routing_entry_cert.rs`(anchor_tls_certificate_paths 接受 cf_cert_mode:dns01→锚 cf_domain、reuse_direct→锚 cert_domain);相关调用传入;Test `crates/db/`。

- [ ] **Step 1 失败测试**:`test_cf_entry_cert_anchored_by_mode`:dns01 时 CF 入口证书路径 = `live/{cf_domain}/`;reuse_direct 时 = `live/{cert_domain}/`。订阅 SNI:CF 线仍 = cf_domain(回归不变)。
- [ ] **Step 2 确认失败**(当前恒锚 cert_domain)。
- [ ] **Step 3 实现**:把 cf_cert_mode + cf_domain 透到证书锚定;按模式选路径。
- [ ] **Step 4 通过 + 回归**。
- [ ] **Step 5 commit** `feat: anchor cf entry cert by cf_cert_mode (dns01->cf_domain)`

## Phase 7 — 部署任务删除/取消 + 卡死自愈

**Files:** Modify `crates/api/src/lib.rs`(加路由)、`crates/api/src/deploy.rs` 或新 handler(delete/cancel)、`crates/db/src/store/deployment_tasks.rs`(delete + 超时标 failed)、`crates/worker/`(扫描卡死任务);Test。

- [ ] **Step 1 失败测试**:`test_delete_deployment_task`(DELETE 后查不到)、`test_cancel_unfinished_task_marks_failed`、`test_stale_waiting_task_auto_failed`(waiting_for_server 超阈值被扫成 failed)。
- [ ] **Step 2 确认失败**(无删除路由/无超时)。
- [ ] **Step 3 实现**:`DELETE /api/admin/deployment-tasks/:id` + `POST .../:id/cancel`;store delete_deployment_task + mark_stale_tasks_failed(created_at/updated_at 超阈值且非终态→failed);worker 周期调用。前端部署任务列表加删除/取消按钮(Phase 9 一并)。
- [ ] **Step 4 通过**。
- [ ] **Step 5 commit** `feat: deployment task delete/cancel + stale auto-fail`

## Phase 8 — 文档对齐

**Files:** 开发方案.md、README、方案总口径、env 示例(XRAYC_CLOUDFLARE_API_TOKEN、cf_cert_mode、3 地址)。门禁 check-docs-no-stale。
- [ ] commit `docs: 3-address node + dns-01 cf cert + task cleanup operating guidance`

## Phase 9 — 前端:节点 3 行地址表单 + 入口协议过滤 + 部署任务删除按钮

**Files:** Modify `frontend/src/views/access-lines/CreateAccessNodeDialog.vue`(3 行地址 IP/域名/CF + 去 cf_enabled 开关 + CF token 输入可选)、`OneClickAgentInstallDialog.vue`(同 + cf_api_token)、`access-entries/AccessEntryDialog.vue`(协议下拉按地址类型过滤)、`DeploymentTasksCard.vue`(删除/取消按钮);types/normalizers/clients 同步;新增辅助 `entryProtocolMatrix.ts`(地址类型→协议集)。

- [ ] **Step 1** 类型/normalizer/client 加 ip_direct_address、cfCertMode、cfApiToken。
- [ ] **Step 2** 节点表单 3 行地址 + 去开关(CF 地址填了即启用)+ CF token 输入(填了 CF 地址才显示);校验:填域名/CF 要 ACME 邮箱。
- [ ] **Step 3** 新增 entryProtocolMatrix.ts:`protocolsForAddressType(type)`(ip→[reality,ss];domain→[reality,ss,trojan,hy2,vless-ws/grpc/xhttp];cf→vless/trojan 的 ws/grpc/xhttp);入口协议下拉据此过滤。
- [ ] **Step 4** 部署任务卡片加删除/取消按钮调新端点。
- [ ] **Step 5** `npm run build` + check-source-file-length(AccessEntriesPage ≤500,余 ≤550;超就拆)。
- [ ] **Step 6 commit** `feat: node 3-address form + entry protocol filter + task delete ui`

## 最终集成门禁(主控)
- [ ] fmt + clippy(-D warnings)+ 全 workspace 测试(真实 PG,串行)+ 前端 build + check-source-file-length。
- [ ] `make rebuild` 起栈 + check-running-current。

## Phase 10 — 真机闭环(no-mock,对齐验证)
- [ ] 重读私有清单;重建控制面到本分支;更新/重建演示节点(用 CF token DNS-01)。
- [ ] 验证:① 节点 3 行地址 UI;② 入口协议按地址过滤(IP 只 Reality/SS;CF 出现 Trojan-WS);③ **DNS-01 给 cf.example.com 签出自己的证书 valid**;④ **Trojan-WS 经 CF 真实出站**(补缺口)+ 直连同时在线 + 计费;⑤ 部署任务删除/取消可用、无卡死残留;⑥ 防泄露。
- [ ] token 只在节点本机 ini,验证后清理;脱敏记日报。
