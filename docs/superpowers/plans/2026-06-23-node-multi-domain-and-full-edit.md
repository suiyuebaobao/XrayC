# 节点多域名 + 全对象可编辑 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: 用 superpowers:subagent-driven-development(推荐)或 superpowers:executing-plans 逐任务实现。步骤用 `- [ ]` 勾选。
> spec:`docs/superpowers/specs/2026-06-23-node-multi-domain-and-full-edit-design.md`。

**Goal:** 中转节点支持多个直连/CF 域名(各自签证书),入口/本机出口下拉选用哪个域名,节点/入口/出口/本机出口全部可编辑(补本机出口就地编辑/删除)。

**Architecture:** 域名从 `access_nodes` 单字段升级为 `node_domains` 一对多表,旧单域名迁移搬入(is_primary)。入口 `access_entries` 与本机出口 `exit_endpoints` 加 `node_domain_id` 引用。护栏/订阅/证书签发改成按"选中域名 / 遍历清单"工作。单域名节点行为零回归。

**Tech Stack:** Rust(Tokio/Axum/SQLx)+ PostgreSQL16 + Vue3/Element Plus/Pinia;真实 PG 集成测试;真机 agent 闭环。

**测试命令(真实 PG):**
```
docker run --rm --network pgdevnet -v /root/suiyue/XrayC:/work -w /work \
  -e CARGO_TARGET_DIR=/ct -e CARGO_HOME=/ch -v xrayc-cargo-reg2:/ch -v xrayc-cargo-tgt2:/ct \
  -e DATABASE_URL=postgres://xrayc:xrayc@pgdev:5432/xrayc_test \
  rust:1-bookworm bash -c "export PATH=/usr/local/cargo/bin:\$PATH; cargo test -p <crate> <test> -- --test-threads=1"
```
前端:`docker run --rm -v /root/suiyue/XrayC/frontend:/app -w /app node:20-alpine sh -c "npm install --no-audit --no-fund >/dev/null 2>&1 && npm run build"`
门禁:fmt + clippy(-D warnings)+ 全量 db 测试 + 前端 build + check-source-file-length(≤550)。

---

## 文件结构

| 文件 | 职责 | 动作 |
|---|---|---|
| `migrations/202606230001_node_domains.sql` | node_domains 表 + 回填 + 入口/出口加 node_domain_id | 新建 |
| `crates/db/src/store/routing_node_domains.rs` | node_domains 增删查 + 派生 cf_enabled | 新建(≤550)|
| `crates/db/src/store/routing_access_nodes_write.rs` | 节点写入时同步 node_domains | 改 |
| `crates/db/src/store/routing_local_exits.rs` | 本机出口护栏按选中域名;就地更新/删除 | 改 |
| `crates/db/src/store/routing_access_entry_updates.rs` | 入口选/改域名 | 改 |
| `crates/db/src/store/routing_entry_cert.rs` | 证书锚定按选中 node_domain | 改 |
| `crates/db/src/protocol_guardrails.rs` | 护栏判定依据=选中域名 kind | 改 |
| `crates/core/src/{model,read_models,subscription_read_models,admin_read_models,heartbeat_read_model}.rs` | 读模型补 node_domains + selected_domain | 改 |
| `crates/access-agent/src/runtime/{tls,config}.rs` | 证书签发遍历 node_domains | 改 |
| `crates/api/src/{dto,admin_nodes,admin_entries}.rs` | DTO + handler:域名列表 + node_domain_id + 本机出口编辑/删除 | 改 |
| `frontend/src/views/access-lines/CreateAccessNodeDialog.vue` | 节点域名列表表单 | 改 |
| `frontend/src/views/access-entries/AccessEntryDialog.vue` | 入口选域名下拉 | 改 |
| `frontend/src/views/access-lines/LocalExitLinesDialog.vue` + 新列表组件 | 本机出口选域名 + 就地编辑/删除 | 改 |

---

## Phase 1:node_domains 表 + 迁移回填

### Task 1.1:迁移建表 + 回填旧单域名 + 入口/出口加列

**Files:**
- Create: `migrations/202606230001_node_domains.sql`
- Test: `crates/db/src/tests/part56.rs`(新建)+ `crates/db/src/tests/mod.rs`(`include!`)

- [ ] **Step 1: 写失败测试** — 迁移后,已有 cert_domain/cf_domain 的节点应在 node_domains 各有一行 is_primary。

`crates/db/src/tests/part56.rs`(前 10 行中文头):
```rust
// 数据库测试片段五十六:node_domains 多域名表 + 旧单域名迁移回填。
// 规则:迁移把 access_nodes.cert_domain→direct 行、cf_domain→cf 行,均 is_primary=true。
// 派生 cf_enabled:存在 kind='cf' 行即启用 CF。真实 PG,缺 DATABASE_URL 跳过。
// 只用 example.test 占位。断言以 store 返回为准。父 tests 模块 include! 引入。
// 单文件低于 550 行。本头满足前十行中文注释。
    #[tokio::test]
    async fn test_node_domains_backfill_from_single_domain_columns() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else { return; };
        let _guard = pg_test_guard().await;
        let store = PgStore::connect(&database_url).await.unwrap();
        store.migrate("../../migrations").await.unwrap();
        store.seed_demo_data().await.unwrap();
        let node_id = store.create_admin_access_node(AdminAccessNodeInput {
            name: "md-backfill".into(),
            public_host: "md.example.test".into(),
            public_port: 443,
            agent_token: "md-backfill-token".into(),
            cert_domain: Some("direct.example.test".into()),
            cf_domain: Some("cf.example.test".into()),
            ..Default::default()
        }).await.unwrap();
        let domains = store.list_node_domains(node_id).await.unwrap();
        assert!(domains.iter().any(|d| d.domain == "direct.example.test" && d.kind == "direct" && d.is_primary));
        assert!(domains.iter().any(|d| d.domain == "cf.example.test" && d.kind == "cf" && d.is_primary));
    }
```

- [ ] **Step 2: 运行,确认失败**(`list_node_domains` / 表不存在)。

- [ ] **Step 3: 写迁移**

`migrations/202606230001_node_domains.sql`:
```sql
-- node_domains:中转节点的多域名清单(直连/CF 各成列表)。
CREATE TABLE IF NOT EXISTS node_domains (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    domain         TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('direct','cf')),
    cf_cert_mode   TEXT,
    acme_email     TEXT,
    is_primary     BOOLEAN NOT NULL DEFAULT FALSE,
    cert_status    TEXT NOT NULL DEFAULT 'unknown',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (access_node_id, domain)
);
CREATE INDEX IF NOT EXISTS idx_node_domains_node ON node_domains(access_node_id);

-- 回填:旧单域名搬入,标 is_primary。
INSERT INTO node_domains (access_node_id, domain, kind, acme_email, is_primary)
SELECT id, cert_domain, 'direct', acme_email, TRUE
FROM access_nodes WHERE cert_domain IS NOT NULL AND cert_domain <> ''
ON CONFLICT (access_node_id, domain) DO NOTHING;

INSERT INTO node_domains (access_node_id, domain, kind, cf_cert_mode, acme_email, is_primary)
SELECT id, cf_domain, 'cf', cf_cert_mode, acme_email, TRUE
FROM access_nodes WHERE cf_domain IS NOT NULL AND cf_domain <> ''
ON CONFLICT (access_node_id, domain) DO NOTHING;

-- 入口 / 本机出口引用选中的域名(免证书留空)。
ALTER TABLE access_entries ADD COLUMN IF NOT EXISTS node_domain_id UUID
    REFERENCES node_domains(id) ON DELETE SET NULL;
ALTER TABLE exit_endpoints ADD COLUMN IF NOT EXISTS node_domain_id UUID
    REFERENCES node_domains(id) ON DELETE SET NULL;
```

- [ ] **Step 4: 实现 store + 类型**,见 Task 1.2。先让本测试编译通过、运行通过。

- [ ] **Step 5: Commit** `feat: node_domains table + backfill migration`

### Task 1.2:node_domains store CRUD

**Files:**
- Create: `crates/db/src/store/routing_node_domains.rs`
- Modify: `crates/db/src/store/mod.rs`(挂模块)、`crates/db/src/types.rs`(`NodeDomain` 结构)

- [ ] **Step 1:** `NodeDomain` 类型 + `AddNodeDomainInput`(domain/kind/cf_cert_mode/acme_email/is_primary)。
- [ ] **Step 2:** 实现:
  - `list_node_domains(&self, node_id) -> Result<Vec<NodeDomain>>`(按 created_at 排序)。
  - `add_node_domain(&self, node_id, AddNodeDomainInput) -> Result<Uuid>`(校验 domain 非空、kind 合法;同节点 domain 唯一;cf 派生 cf_cert_mode)。
  - `delete_node_domain(&self, domain_id)`:**若被 access_entries/exit_endpoints 引用则 Err**(先查引用计数)。
  - `set_primary_node_domain(&self, node_id, domain_id, kind)`:同 kind 下唯一 is_primary。
  - `node_has_domain_kind(&self, node_id, kind) -> bool`(护栏/cf_enabled 派生用)。
- [ ] **Step 3:** 运行 Task 1.1 测试 → 通过。
- [ ] **Step 4:** 补测试:同节点重复 domain 报错;delete 被引用域名报错;cf 域名 cf_cert_mode 派生 dns01(给了 acme/token 时)。
- [ ] **Step 5: Commit** `feat: node_domains store crud`

### Task 1.3:节点写入同步 node_domains + cf_enabled 派生

**Files:** Modify `crates/db/src/store/routing_access_nodes_write.rs`、`routing_nodes.rs`(读)

- [ ] **Step 1:** 失败测试:`create_admin_access_node` 带 cert_domain+cf_domain → node_domains 两行(Task 1.1 已覆盖创建路径,这里覆盖**更新**:`update_access_node` 改 cert_domain 应同步 node_domains 主行)。
- [ ] **Step 2-4:** 节点创建/更新时,把 cert_domain/cf_domain 写入的同时 upsert 对应 is_primary 的 node_domains 行;cf_enabled 读取改为 `node_has_domain_kind(node,'cf')`。
- [ ] **Step 5: Commit** `feat: sync primary node_domains on node write`

---

## Phase 2:护栏按选中域名判定

### Task 2.1:出口侧护栏用选中 node_domain 的 kind

**Files:** Modify `crates/db/src/store/routing_local_exits.rs`、`crates/db/src/protocol_guardrails.rs`

- [ ] **Step 1: 失败测试**(`part56.rs`):本机出口选一个 kind='direct' 的 node_domain → Trojan 放行;选一个不存在/留空 → 无证书 → Trojan 被拦(沿用现有 `validate_local_exit_cert_domain` 消息)。CF 域名 → 只放 ws/grpc/xhttp。
```rust
    #[tokio::test]
    async fn test_local_exit_trojan_allowed_when_selected_domain_is_direct() {
        // 节点有一个 direct 域名;本机出口选它 → Trojan 放行。
        // 节点同时有 cf 域名;本机出口选 cf 域名 → Trojan 被拦(CF 只放 ws/grpc/xhttp)。
    }
```
- [ ] **Step 2-4:** `create_admin_local_exit_lines` 接受 `node_domain_id`;护栏判定从 `node_has_cert_domain`(节点级)改为"选中 node_domain 是否 direct/cf";没选域名=无证书=只放免证书协议。`validate_local_exit_cert_domain` 增加按选中域名 kind 的分支。保持「提前判断」位置(在协议字段校验之前)。
- [ ] **Step 5: Commit** `feat: local exit guard keys off selected node_domain`

### Task 2.2:入口侧护栏用选中 node_domain

**Files:** Modify `crates/db/src/store/routing_access_entries.rs`、`routing_access_entry_updates.rs`、`protocol_guardrails.rs`

- [ ] **Step 1: 失败测试:** 入口选 cf 域名 → 只放 VLESS/Trojan + ws/grpc/xhttp + tls;选 direct 域名 → 放 Trojan/HY2/VLESS-TLS;留空 → Reality/SS。
- [ ] **Step 2-4:** 入口创建/更新接受 `node_domain_id`;`validate_protocol_cdn_combination` / `validate_tls_protocol_needs_domain` 的判定依据从节点 cf_domain/cert_domain 改为选中 node_domain 的 kind 与 domain 值。
- [ ] **Step 5: Commit** `feat: entry guard keys off selected node_domain`

---

## Phase 3:证书锚定 + 签发遍历清单

### Task 3.1:证书锚定按选中 node_domain

**Files:** Modify `crates/db/src/store/routing_entry_cert.rs`

- [ ] **Step 1: 失败测试:** 入口选 cf 域名(dns01)→ 证书路径锚定该 cf 域名;选 direct 域名 → 锚定该 direct 域名(非节点主域名)。
- [ ] **Step 2-4:** `anchor_tls_certificate_paths` 入参增加选中 node_domain(domain + kind + cf_cert_mode),路径用选中域名而非节点单字段。
- [ ] **Step 5: Commit** `feat: anchor entry cert to selected node_domain`

### Task 3.2:agent 证书签发遍历 node_domains

**Files:** Modify `crates/access-agent/src/runtime/tls.rs`、`config.rs`;心跳读模型 `crates/core/src/heartbeat_read_model.rs`

- [ ] **Step 1: 失败测试**(`crates/access-agent` 单测):给 agent 下发两个 direct 域名 → 生成两条 HTTP-01 签发计划;一个 cf 域名 → 一条 DNS-01 计划。
- [ ] **Step 2-4:** 心跳/期望配置把节点 node_domains 列表下发给 agent;agent 遍历清单,direct→`build_certbot_http01_args`、cf→`build_certbot_dns01_args`(已存在),各域名各签;`x509 -checkend` 有效即跳过(沿用)。CF 多域名各自 token 从 env 注入(同账号复用)。
- [ ] **Step 5: Commit** `feat: agent signs cert per node_domain`

---

## Phase 4:本机出口就地编辑/删除

### Task 4.1:本机出口更新/删除 store + 端点

**Files:** Modify `crates/db/src/store/routing_local_exits.rs`、`crates/api/src/lib.rs`、`crates/api/src/admin_nodes.rs`、`crates/api/src/dto.rs`

- [ ] **Step 1: 失败测试:** 建一条本机出口 → `update_local_exit_line`(改端口/域名/启用)生效;`delete_local_exit_line` 删除 + 节点 config_dirty。
- [ ] **Step 2-4:**
  - store:`update_local_exit_line(endpoint_id, fields)`、`delete_local_exit_line(endpoint_id)`(限 ownership=self_hosted),改完置节点 config_dirty。
  - 路由:`PUT /api/admin/access-nodes/:id/local-exit-lines/:endpoint_id`、`DELETE .../:endpoint_id`、`GET .../local-exit-lines`(列该节点 self_hosted 出口)。复用已存在的 exit_endpoints PUT/DELETE 逻辑或薄封装。
- [ ] **Step 5: Commit** `feat: local exit in-place update/delete endpoints`

---

## Phase 5:读模型对齐 + 订阅按选中域名

### Task 5.1:读模型补 node_domains + selected_domain

**Files:** Modify `crates/core/src/model.rs`、`read_models.rs`、`admin_read_models.rs`

- [ ] **Step 1: 失败测试:** 控制面读节点 JSON 含 `node_domains[]`;读入口含 `node_domain_id` + 解析出的 domain/kind。
- [ ] **Step 2-4:** 读模型 load 补查 node_domains;节点视图加 `domains` 数组;入口/出口视图加 `node_domain`(id+domain+kind)。遵守"写侧有读侧无"红线。
- [ ] **Step 5: Commit** `feat: read models carry node_domains and selected domain`

### Task 5.2:订阅输出按入口选中域名

**Files:** Modify `crates/core/src/subscription_read_models.rs`

- [ ] **Step 1: 失败测试:** 入口选 domainB → 订阅该入口输出 SNI/地址=domainB;未选 → 用节点主域名/IP(行为同今天)。
- [ ] **Step 2-4:** 订阅生成把入口的连接地址/SNI 替换成选中 node_domain.domain;免证书入口(无 node_domain)保持现状。**防泄露不变**:只出中转入口参数,不出口真实出口。
- [ ] **Step 5: Commit** `feat: subscription presents entry selected domain`

---

## Phase 6:API DTO + handler 接线

### Task 6.1:节点 DTO 域名列表 + 入口/出口 node_domain_id

**Files:** Modify `crates/api/src/dto.rs`、`admin_nodes.rs`、`admin_entries.rs`

- [ ] **Step 1: 失败测试**(api 层):创建节点带 `domains:[{domain,kind}]` → 落库;创建入口带 `node_domain_id` → 落库。
- [ ] **Step 2-4:** `CreateAccessNodeRequest`/`UpdateAccessNodeRequest` 加 `domains: Vec<NodeDomainInput>`(向后兼容:仍接受旧 cert_domain/cf_domain,内部并入);入口/本机出口 DTO 加 `node_domain_id: Option<Uuid>`。
- [ ] **Step 5: Commit** `feat: api dto for node domains and selected domain`

---

## Phase 7:前端

### Task 7.1:节点域名列表表单

**Files:** Modify `frontend/src/views/access-lines/CreateAccessNodeDialog.vue`(超 550 先拆子组件 `NodeDomainsField.vue`)

- [ ] **Step 1-3:** "域名直连地址"/"CF直连地址"从单 input 改为**可增删多行**列表(每行 input + 删除按钮 + "设为主域名");CF 行带 token。绑定 `form.domains`。
- [ ] **Step 4:** `npm run build` 通过。
- [ ] **Step 5: Commit** `feat(ui): node multi-domain list form`

### Task 7.2:入口/本机出口选域名下拉 + 本机出口编辑删除

**Files:** Modify `AccessEntryDialog.vue`、`LocalExitLinesDialog.vue` + 新 `LocalExitLinesList.vue`

- [ ] **Step 1-3:** 入口/本机出口表单加"选域名"下拉(列该节点 domains,按 kind 过滤;留空=免证书);护栏前端过滤按选中域名 kind(复用 `entryProtocolMatrix` / `localExitProtocolOptionsFor`,入参从节点域名换成选中域名)。本机出口页加已建线路列表 + 编辑/删除按钮(接 Task 4.1 端点)。
- [ ] **Step 4:** `npm run build` 通过。
- [ ] **Step 5: Commit** `feat(ui): entry/exit domain dropdown + local exit edit/delete`

### Task 7.3:节点卡片显示域名/IP 数量

**Files:** Modify `frontend/src/views/access-lines/RelayNodeCard.vue`、读模型(`admin_read_models.rs` 已带 domains)

- [ ] **Step 1-3:** 节点卡片/信息区显示:**直连域名 N 个、CF 域名 N 个、直连 IP**(有 ip_direct_address 显示 1 个/该 IP 脱敏或后台可见)。数据取节点视图的 `domains[]`(按 kind 计数)+ `ip_direct_address`。鼠标悬停或展开可看具体域名清单。
- [ ] **Step 4:** `npm run build` 通过。
- [ ] **Step 5: Commit** `feat(ui): node card shows domain/ip counts`

---

## Phase 9:一键安装仅限 IP 部署(正确性)

> **关键认知:SSH IP 与客户连接地址是两套完全无关的东西。** 一键部署只用专门的 **SSH IP** 字段
> SSH 进服务器装 agent;客户连接地址(域名直连 cert_domain / CF直连 cf_domain / IP直连
> ip_direct_address)是客户端连的,**和 SSH 毫无关系,绝不能混**。
> 因此:SSH Host 是它自己**独立**的 IP 字段,**不从任何客户连接地址自动带出**;域名(尤其橙云)
> 无法 SSH,SSH 目标只能 IP。public_host 等客户地址仍可 IP 可域名,但与 SSH 无关。

### Task 9.1:后端校验 ssh_host 必须是 IP

**Files:** Modify `crates/api/src/deploy_install.rs`(`resolve_one_click_agent_install_request`,约 L51 ssh_host 校验处)

- [ ] **Step 1: 失败测试**(`crates/api/src/deploy_install_tests.rs`):ssh_host 传域名(如 `relay.example.test`)→ resolve 返回 Err,消息含"SSH 地址必须是 IP";传 `192.0.2.10` → Ok。
```rust
    #[tokio::test]
    async fn test_one_click_ssh_host_rejects_domain() {
        std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
        let mut body = /* 复用现有测试构造,ssh_host 改成域名 */;
        body.ssh_host = "relay.example.test".to_string();
        let err = resolve_one_click_agent_install_request(&HeaderMap::new(), body).await.unwrap_err();
        assert!(format!("{err:?}").contains("SSH 地址必须是 IP"));
    }
```
- [ ] **Step 2: 运行确认失败**(当前不校验 IP)。
- [ ] **Step 3: 实现:** ssh_host 校验后加 `ssh_host.parse::<std::net::IpAddr>().is_err()` → 返回 InvalidInput("一键安装的 SSH 地址必须是 IP,不能是域名(域名尤其橙云无法直连 SSH)")。
- [ ] **Step 4: 运行确认通过** + 现有一键安装测试不回归。
- [ ] **Step 5: Commit** `fix: one-click install requires IP ssh_host (reject domain)`

### Task 9.2:前端 SSH 目标从 IP 直连地址带出 + IP 校验

**Files:** Modify `frontend/src/views/access-lines/OneClickAgentInstallDialog.vue`

- [ ] **Step 1-3:** SSH Host 字段:label 改"SSH Host(IP)",placeholder"服务器公网 IP,例如 203.0.113.10";**彻底移除从 publicHost(或任何客户连接地址)自动带出的 watcher**——SSH Host 是独立字段,管理员单独填它自己的 SSH IP,不跟域名直连/CF直连/IP直连 联动。表单提交前前端校验 sshHost 是 IP(非 IP 直接提示,不发请求)。public_host 及各客户连接地址字段保持原样(它们是客户端连的,与 SSH 无关)。
- [ ] **Step 4:** `npm run build` 通过。
- [ ] **Step 5: Commit** `fix(ui): one-click ssh host is IP, autofilled from ip_direct_address`

---

## Phase 8:门禁 + 真机闭环

### Task 8.1:全量门禁

- [ ] fmt + clippy(-D warnings)+ **全量 db 测试**(`cargo test -p xrayc-db --lib -- --test-threads=1`)+ api/core/agent 包测试 + 前端 build + check-source-file-length。
- [ ] Commit 修复(若有)。

### Task 8.2:真机多域名闭环(节点A,灰云示例域名 example.com)

- [ ] 节点A 配**两个直连域名**(example.com + 一个子域),agent 各签 HTTP-01 证书(`certbot certificates` 显示两张,各 89 天)。
- [ ] 建两个入口分别选两个域名(或一个入口切换域名),真实客户端经各域名出站,出口符合预期,计费各自独立。
- [ ] **防封轮换验证**:把入口的域名下拉从 A 切到 B → 订阅更新 → 客户端刷新 → 流量迁到 B(B 证书早已就绪,切换即时)。
- [ ] 本机出口就地编辑(改端口)/删除真 API + 前端 no-mock。
- [ ] 防泄露自检 + 清理干净;日报记"已覆盖 N 台 + 多域名/轮换闭环"。

---

## Self-Review(写完自检)

- **Spec 覆盖:** 多域名表✓(P1)/迁移回填✓(P1)/cf_enabled派生✓(P1.3)/入口出口选域名✓(P2,6)/护栏按选中域名✓(P2)/证书遍历清单✓(P3)/本机出口就地编辑删除✓(P4)/读模型对齐✓(P5)/订阅✓(P5.2)/前端✓(P7)/真机闭环✓(P8)。
- **占位扫描:** 无 TBD;关键 SQL/测试均给具体代码;重复样式引用现有模式 + 明确文件路径。
- **类型一致:** `NodeDomain`/`node_domain_id`/`list_node_domains`/`validate_local_exit_cert_domain` 全程同名。
- **YAGNI:** 无自动轮换、无 SNI 多路复用、无泛域名(spec 非目标),计划未引入。
