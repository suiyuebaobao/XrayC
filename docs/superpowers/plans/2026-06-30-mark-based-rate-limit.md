# 限速改「每用户 mark」实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 XrayC 限速从「每用户运行时端口」整体换成「每用户 Xray sockopt.mark」,让 CF 入口稳守 443 同时保每用户限速,并退役整套运行时端口系统。

**Architecture:** 每线路单入站守真实端口、全用户共用、凭据区分;每限速用户=确定性 mark + 带 mark 出站 + `user`路由;agent 在出口侧按 fwmark/connmark 整形(上行 egress、下行 ingress→IFB)。

**Tech Stack:** Rust(Tokio/Axum/SQLx)、Xray-core 26.5.9、Linux tc/iptables/IFB/act_connmark、Vue3+Playwright、Docker Compose、PostgreSQL16。

参考 spec:`docs/superpowers/specs/2026-06-30-mark-based-rate-limit-design.md`

---

## Phase 0:文档/方案先行(CLAUDE.md §0 强制)

### Task 0.1:更新 开发方案.md 限速章节
**Files:** Modify `开发方案.md`(§7.7.1、§2.2.2、行 5/45/424-428/1192/1459-1469)
- [ ] 把「正限速用户拆成单用户 runtime inbound + runtime 端口」口径改为「全用户共用单入站守真实端口 + 每用户带 mark 出站 + user 路由;agent 出口侧按 fwmark/connmark 整形」。
- [ ] §7.7.1 保留 `act_connmark` 自检/优雅降级红线(机制不变,仍用 connmark+IFB)。
- [ ] 删「运行时端口 20000-60999」「订阅端口=用户 runtime 端口」「逻辑端口 readiness 不可用」等过时描述。
- [ ] Commit:`docs: switch rate-limit design to per-user mark in 开发方案`

### Task 0.2:同步 AGENTS.md / CLAUDE.md + README + 文档
**Files:** Modify `AGENTS.md`(行68)、`CLAUDE.md`、`README.md`(行24)、`文档/说明.md`、`文档/架构/当前方案与运营闭环.md`(128-134)、`文档/架构/架构说明.md`、`文档/部署/部署说明.md`(46/129/165)、`文档/接口/*`、相关 migration 注释
- [ ] 按 spec §8 清单逐一把「按端口/runtime 端口」措辞改为「按每用户 mark」。AGENTS.md 与 CLAUDE.md 双向同步(CLAUDE.md §0 强制)。
- [ ] `scripts/check-docs-no-stale.sh` 增加旧措辞拦截(如 `runtime 端口`、`按端口限速`)。
- [ ] 跑 `bash scripts/check-docs-no-stale.sh` 通过。
- [ ] Commit:`docs: sync rate-limit wording to per-user mark across docs/rules`

---

## Phase 1:实现重构(TDD,Rust 在 docker 内跑)

> 通用:Rust 测试用 `docker compose run --rm ...` 或既有 `cargo test -p <crate>` 在 rust:1-bookworm 容器内跑(CLAUDE.md §6)。每个 Task 先写失败测试→看红→最小实现→看绿→commit。

### Task 1:core — 每用户 mark 分配
**Files:** Modify `crates/core/src/runtime_ports.rs`;Test 同文件 `#[cfg(test)]`
- [ ] **Step1 写失败测试**:`test_rate_limit_mark_for_user_is_deterministic_and_in_range` —— 同一 user_id 两次调用得同一 mark;mark 落 `[0x10000, 0xFFFFF]`;两个不同 user 在同一 node 上 mark 不相等(去冲突)。
- [ ] **Step2 看红**:`cargo test -p xrayc-core rate_limit_mark` 失败(函数不存在)。
- [ ] **Step3 实现**:新增 `pub fn rate_limit_mark_for_user(data,&line,user_id)->Option<u32>` 与 `rate_limit_marks_for_node(...)`(镜像现 `runtime_listen_ports_for_node` 的 hash+按节点去冲突,落 mark 区间);`runtime_listen_port_for_user` 改为恒返回 `line.listen_port`;`user_needs_runtime_port` 重命名 `user_needs_rate_limit`(语义不变:任一方向有效限速>0)。保留 `effective_user_rate_limit_up/down_bps`。
- [ ] **Step4 看绿**:测试通过。
- [ ] **Step5 退役**:删 `RUNTIME_PORT_START/END`、`next_available_runtime_port`、避让本机出口端口的端口逻辑(去冲突逻辑迁到 mark)。改 `cargo test -p xrayc-core` 全绿。
- [ ] **Step6 Commit**:`feat(core): allocate per-user fwmark, retire runtime ports`

### Task 2:render — 单入站合并 + 每用户带 mark 出站 + 路由
**Files:** Modify `crates/db/src/xray_render.rs`(128-178、409-440);`crates/xray-config/src/types.rs`(223-238 按需);Test 在 `crates/db/src/tests/`
- [ ] **Step1 写失败测试**:`test_render_limited_user_shares_inbound_and_gets_marked_outbound` —— 一条 CF 线(443)+2 用户(1 限速1 不限速):断言只渲染**一个** 443 入站、含两个 user;限速用户有一条带 `sockopt.mark` 的出站 + 一条 `routing rule {user:[email]→该出站}`;`XrayUserRateLimit.mark>0`;入站端口=443(不被改成 runtime 端口)。
- [ ] **Step2 看红**。
- [ ] **Step3 实现**:删「限速用户拆单用户 runtime 入站」分支(128-155),让限速/不限速用户都进 `shared_inbounds`;为限速用户新增「带 mark 出站(到其 exit)+ user 路由规则」;`rate_limit_entry_for_user` 用 `rate_limit_mark_for_user` 填 mark。
- [ ] **Step4 看绿**。
- [ ] **Step5 修连带测试**(原断言 runtime 端口的用例改为断言真实端口 + mark)。`cargo test -p xrayc-db` 全绿。
- [ ] **Step6 Commit**:`feat(render): shared inbound + per-user marked outbound + routing`

### Task 3:subscription — 用户端口用真实端口
**Files:** Modify `crates/core/src/subscription.rs`(169-172);Test 同 crate
- [ ] **Step1 写失败测试**:`test_subscription_uses_line_listen_port_for_limited_user` —— 限速用户订阅 proxy.port == `line.listen_port`(非 runtime 端口)。
- [ ] **Step2 看红**(若现行为是 runtime 端口则红)。
- [ ] **Step3 实现**:`runtime_listen_port_for_user` 已恒返回 `line.listen_port`(Task1),确认此处不再被改写;删端口覆盖那行(172)若多余。
- [ ] **Step4 看绿**。
- [ ] **Step5 Commit**:`fix(subscription): emit real listen port for limited users`

### Task 4:agent limiter — 按 mark 整形
**Files:** Modify `crates/access-agent/src/runtime/limiter.rs`;Test `crates/access-agent/src/runtime/tests/limiter.rs`
- [ ] **Step1 写失败测试**:`test_limiter_shapes_by_fwmark_not_port` —— 给 2 限速用户(各自 mark/up/down),断言生成的命令里:**上行**=节点出口方向(eth0 egress)按 `handle {mark} fw` 进 class 2:N(或物理网卡 egress class)、速率=up;**下行**=ingress `act_connmark` 还原→IFB,IFB 上 `handle {mark} fw`→class、速率=down;**不再**出现 `match ip dport {port} action skbedit`(按端口打 mark 的规则)。
- [ ] **Step2 看红**。
- [ ] **Step3 实现**:删 `ingress_access_port_mark_commands`(260-293)按 dport 的 u32+skbedit;`egress_download_to_client_commands`(301-331)从按 sport 改为按 fwmark(`handle {mark} fw`);上行改「节点→出口 egress 按 mark」;保留 `CONNMARK save/restore`、`matchall connmark redirect`、`act_connmark` 降级(39-46/87-93/156-171)不动。
- [ ] **Step4 看绿**;改既有 limiter 单测断言(去掉按端口断言、加按 mark 断言)。`cargo test -p xrayc-access-agent` 全绿。
- [ ] **Step5 Commit**:`feat(agent): shape per-user by fwmark/connmark, drop port-based rules`

### Task 5:全量门禁
- [ ] 跑 `make check-fast`(接口/DB/订阅/限速面广)→ 按影响补包级测试。
- [ ] 跑 `make check-release`(隔离 docker runtime);通过后 `make check-running-current` 核镜像新鲜度。
- [ ] Commit(如有修复):`test: green check-release for mark-based limiter`

---

## Phase 2:真机部署 + 风险点优先验证

### Task 6:rebuild + 部署新 agent
- [ ] `make rebuild-backend`(api 跑迁移)+ rebuild xray/agent 镜像;`make package-docker-artifacts`。
- [ ] 用面板一键安装或 `scripts/deploy-access-agent.sh` 把新 agent 部署到测试节点(读 `新的服务器账号密码.txt` 最新清单)。核镜像 da3f→新 hash。

### Task 7:本机出口回环按 mark 整形真机验证(**风险点,先做**)
- [ ] 在有本机出口的 CF 节点(指纹 66bd0c56)上,真机验证「主 Xray↔本机出口(127.0.0.1 回环)按 mark/connmark 整形能否限住」:限速用户经本机出口下载/上传,实测被限到套餐值。
- [ ] **若回环整形不灵** → 切换备选:主 Xray 连本机出口走 veth(非 loopback),改 render/部署让本机出口 bind veth IP,重验。更新 spec/方案记录最终结论。

### Task 8:CF 线通 + 401 排查
- [ ] 排查并修复节点 66bd0c56 的 agent 401(token 不匹配:核对面板 agent_token_hash 与节点 env XRAYC_NODE_TOKEN,重装/重置)。
- [ ] 验证 CF 入站稳守 443(`ss -tlnp` 有 443、本机 `openssl s_client 127.0.0.1:443` 握手成功)、限速用户 mark 出站存在、fast.com/真实客户端 CF 线通且上传被限到套餐值。

---

## Phase 3:全协议真机测试(全 UI + 真客户端,零 API)

### Task 9:搭 Playwright 后台 UI harness
**Files:** Create `frontend/e2e/runtime/mark-limit-setup.spec.ts`、`frontend/e2e/runtime/lib/admin-ui.ts`
- [ ] 用 Playwright 驱动真实后台(admin/admin123456):UI 建 2 套餐(限速/不限速)、10 用户、配 CF 橙云 + 灰云直连线路(节点取自清单真机)。**绝不调后台 API/DB**。
- [ ] 断言 UI 上套餐/用户/线路建成、订阅可下载。

### Task 10:搭真实客户端流量 harness
**Files:** Create `scripts/real-multiclient-traffic.sh`、`scripts/lib/real-multiclient/*.sh`
- [ ] 从清单真机起多个真实 xray 客户端(各协议 VLESS/VMess/SS2022/Trojan/SOCKS5/HTTP/HY2),用 UI 建的真实订阅/账号连接,发真实流量(iperf3/curl 大文件),测真实到达速率。
- [ ] 覆盖矩阵:协议 × 连接方式(CF橙云/IP直连/灰云域名直连)× 方向(上/下)× TCP/UDP。香港机 UDP 入站不通按网络事实判读。

### Task 11:跑全协议矩阵 + 问题修复重测
- [ ] 跑矩阵,真实观测「限速套餐用户被限到套餐值、不限速套餐不受限」。脱敏记「已覆盖 N 台/N 协议」。
- [ ] 发现问题 → 回 Phase 1 定位修复(TDD 补测)→ 重测该项,直到全绿。

---

## Phase 4:压测(3 轮 × 20 分钟)

### Task 12:全 UI 建压测场景
- [ ] Playwright UI 建:10 账号、套餐 A(限速)/套餐 B(不限速)各 5 用户。零 API。

### Task 13:实时监控 harness
**Files:** Create `scripts/stress-monitor.sh`
- [ ] 实时采集(每 ~30s):面板运行数据(在线/速率/流量)、各节点 `tc -s class` 计数与 drop/overlimit、客户端实测速率。输出脱敏滚动报告。

### Task 14:跑 3 轮 × 20 分钟 + 问题修复重测
- [ ] 10 账号真实客户端持续真实流量,每轮 20 分钟 × 3 轮。
- [ ] 每轮实时监控;**发现问题 → 修复 → 重测该轮**(该轮重新计满 20 分钟)。
- [ ] 验收:限速套餐 5 用户全程严格受限、不限速套餐 5 用户不受限、节点稳定无 apply 卡死/无断流、计费/会话正常。

---

## 交付门禁(总验收)
- `make check-release` 通过 + 镜像新鲜度核对。
- 真机:CF 线通 + IP/灰云线正常 + 本机出口回环限速验证 + 限速按 mark 生效(套餐值)。
- 全协议矩阵真机通过。
- 3 轮压测通过。
- 文档/规则口径一致、无「按端口限速」残留。
- 当日 `文档/每日记录/2026-06-30-日报.md` 记完成/验证/遗留。
