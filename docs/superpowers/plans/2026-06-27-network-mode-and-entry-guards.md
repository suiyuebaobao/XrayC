# 入口网络模式重构 + 连接方式护栏 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把可合并的 TCP/UDP/XUDP 承载合并成一条线路(SS 选 TCP+UDP 出 1 条而非 2 条),并修两个连接方式护栏(域名直连不该有 Reality、IP 直连地址别填域名)。

**Architecture:** 网络模式拆成「传输(互斥→多条)」+「TCP/UDP/XUDP 承载(可合并→一条)」两维;前端选择器与扇出规则按此改,后端接受合并值 `tcp,udp` 并补 Reality+域名护栏。

**Tech Stack:** Rust(Axum/SQLx,PG 集成测试 `crates/db/src/tests/partNN.rs`)+ Vue3/TS(无 vitest,验证用 `npm run build` + Playwright `frontend/e2e/`)。

**测试设施现实**:后端有 PG 集成测试 → 真 TDD;前端无单测 → 验证 = `npm run build` 类型校验 + Playwright e2e + 纯函数逻辑核对。

---

## 文件结构(谁负责什么)

**后端(块2)**
- `crates/db/src/store/routing_local_exits.rs` — `validate_local_exit_network_mode` 接受合并值 `tcp,udp`。
- `crates/db/src/store/routing_access_entries.rs` — Reality+direct 域名护栏(拒 422)。
- `crates/xray-config/src/inbound.rs` — 确认 SS `network` 接 `tcp,udp` 正确编译(已支持,补测)。
- `crates/db/src/tests/part68.rs`(新)— 本批后端回归测试。

**前端网络模式(块3)**
- `frontend/src/views/access-lines/format.ts` — `networkModeOptions` 重构为「传输 + 承载」两维,新增按协议×security 的承载选项函数 + 合并值产出。
- `frontend/src/views/access-lines/localExitLines.ts` — 本机出口承载选项同口径。
- `frontend/src/views/access-lines/LocalExitLinesDialog.vue` + `frontend/src/views/access-entries/entryPayload.ts` — 扇出规则:同传输内承载合并 1 条,仅不同传输才多条。

**前端护栏+地址(块4)**
- `frontend/src/views/access-entries/AccessEntryDialog.vue` — securityOptions domain 分支去 Reality。
- `frontend/src/views/access-entries/entryConnectionMode.ts` — 提示文案去 Reality。
- `frontend/src/views/access-entries/useEntryAddressAutofill.ts` — 地址按连接方式分流。

**文档(块1,先行)**
- `开发方案.md` §2.2.1/§2.2.2。

---

## 块1(先行):同步开发方案

### Task 1: 更新 开发方案.md 网络模式 + 护栏口径

**Files:** Modify `开发方案.md`(§2.2.1 入口网络模式、§2.2.2 本机出口、连接方式↔协议护栏处)

- [ ] **Step 1: 改三处口径**
  1. 网络模式:明确「传输互斥(选多个→多条线路);TCP/UDP/XUDP 承载在同一传输内合并成一条(SS=`tcp,udp` 一条;不再逐 mode 拆)」。
  2. 连接方式↔协议:域名直连 VLESS **只 TLS**(删去 Reality/普通 none);Reality 仅 IP 直连。
  3. 地址来源:IP 直连入口监听地址默认取节点 `ip_direct_address`,域名直连取选中直连域名,CF 取 CF 域名。
- [ ] **Step 2: 校验文档门禁**

Run: `bash scripts/check-docs-no-stale.sh`
Expected: 通过(无 stale 口径残留)

- [ ] **Step 3: Commit**

```bash
git add 开发方案.md
git commit -m "docs: network-mode merge rule + reality/address connection-mode guards"
```

> 块2/3/4 依赖块1 的口径,块1 合并后并行启动。

---

## 块2(后端,可并行):合并值 + Reality 护栏

### Task 2: validate_local_exit_network_mode 接受 `tcp,udp`

**Files:**
- Modify: `crates/db/src/store/routing_local_exits.rs:255-289`(`validate_local_exit_network_mode`)
- Test: `crates/db/src/tests/part68.rs`(新)

- [ ] **Step 1: 写失败测试**(part68.rs,模仿 part67 头部 10 行中文注释 + DATABASE_URL 跳过守卫)

```rust
#[tokio::test]
async fn test_local_exit_accepts_merged_tcp_udp_network() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping merged network mode test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    // SS 本机出口 network_mode="tcp,udp" 应被接受并落库为一条,不报错。
    let node = create_demo_direct_node(&store).await; // 复用 part 内既有 helper 或现造
    let resp = store.create_local_exit_lines(node, vec![local_exit_input("shadowsocks", "tcp,udp")]).await.unwrap();
    assert_eq!(resp.created_lines.len(), 1, "tcp,udp 应只建 1 条");
    let net: String = sqlx::query_scalar("SELECT stream_config->>'network' FROM exit_endpoints WHERE id=$1")
        .bind(resp.created_lines[0].exit_endpoint_id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(net, "tcp,udp");
}
```

- [ ] **Step 2: 跑测试看它失败**

Run(容器化,见 Task 9 命令):`cargo test -p xrayc-db test_local_exit_accepts_merged_tcp_udp_network`
Expected: FAIL —— 当前 `validate_local_exit_network_mode` 把 `tcp,udp` 当非法值拒掉。

- [ ] **Step 3: 实现**:在 `validate_local_exit_network_mode` 的 `match selected` 前先处理合并值:把逗号分隔的每段单独校验(每段 ∈ {tcp,udp}),全合法则原样返回合并值(规范化为 `tcp,udp` 顺序)。非 SS/SOCKS 协议不允许合并(仍单值)。

```rust
// 合并承载:tcp,udp 这类多值,逐段校验后原样保留(仅 SS/SOCKS 允许)。
if normalized.contains(',') {
    let parts: Vec<&str> = normalized.split(',').map(str::trim).collect();
    let all_l4 = parts.iter().all(|p| matches!(*p, "tcp" | "udp"));
    if all_l4 && matches!(outbound_type, "shadowsocks" | "socks") {
        return Ok(if parts.contains(&"udp") && parts.contains(&"tcp") { "tcp,udp" } else { selected_static(parts[0]) });
    }
    return Err(DbError::InvalidAgentPayload(format!("该协议不支持合并网络模式: {normalized}")));
}
```
(`selected_static` 把 &str 映射到现有 'static 返回值;若签名返回 `&'static str` 不便返回动态,改函数返回 `String` 或 `Cow`,并同步调用点。)

- [ ] **Step 4: 跑测试看它通过**

Run: `cargo test -p xrayc-db test_local_exit_accepts_merged_tcp_udp_network`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/db/src/store/routing_local_exits.rs crates/db/src/tests/part68.rs crates/db/src/tests.rs
git commit -m "feat: accept merged tcp,udp network mode for ss/socks local exit"
```

### Task 3: Reality + 域名直连入口护栏(拒 422)

**Files:**
- Modify: `crates/db/src/store/routing_access_entries.rs`(入口校验路径,`access_entry_network_mode`/创建入口处)
- Test: `crates/db/src/tests/part68.rs`(追加)

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn test_reality_entry_rejected_on_direct_domain() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else { eprintln!("skip"); return; };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();
    store.seed_demo_data().await.unwrap();
    // 节点配直连域名;建 VLESS+reality 入口且 node_domain_id 指向该 direct 域名 → 必须被拒。
    let (node, direct_domain_id) = create_node_with_direct_domain(&store).await;
    let mut input = vless_reality_entry_input(node);
    input.node_domain_id = Some(direct_domain_id);
    let err = store.create_admin_access_entry(input).await.unwrap_err();
    assert!(matches!(err, DbError::InvalidAgentPayload(_)), "reality+direct 域名应被拒");
}
```

- [ ] **Step 2: 跑测试看它失败**

Run: `cargo test -p xrayc-db test_reality_entry_rejected_on_direct_domain`
Expected: FAIL —— 当前无此护栏,入口被创建。

- [ ] **Step 3: 实现**:在入口创建校验里加:当 `node_domain_id` 解析到 kind=`direct` 域名,且 inbound security=`reality` → 返回 `DbError::InvalidAgentPayload("域名直连不支持 Reality,请用 TLS 或改 IP 直连")`。放在协议字段护栏附近(与 `validate_vless_reality_entry_network_mode` 同区)。

- [ ] **Step 4: 跑测试看它通过**

Run: `cargo test -p xrayc-db test_reality_entry_rejected_on_direct_domain`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/db/src/store/routing_access_entries.rs crates/db/src/tests/part68.rs
git commit -m "feat: reject reality entry bound to direct-domain (defense in depth)"
```

---

## 块3(前端网络模式,可并行):两维选择器 + 合并扇出

### Task 4: format.ts 承载选项两维化

**Files:** Modify `frontend/src/views/access-lines/format.ts`(`networkModeOptions` 区 + 新增承载选项函数)

- [ ] **Step 1: 拆两维**。保留传输概念(tcp/ws/grpc/xhttp),新增「承载」选项产出函数 `carriageOptionsFor(protocol, security)`,返回离散合并项:
  - shadowsocks → `[{label:'TCP',value:'tcp'},{label:'UDP',value:'udp'},{label:'TCP+UDP',value:'tcp,udp'}]`
  - vless 非 reality → `[{label:'TCP',value:'tcp'},{label:'TCP+UDP',value:'tcp,udp'},{label:'TCP+UDP+XUDP',value:'tcp,udp,xudp'}]`(xudp 段映射 packetEncoding)
  - vless reality → `[{label:'仅 TCP',value:'tcp'}]`
  - trojan → `[{label:'TCP',value:'tcp'},{label:'TCP+UDP',value:'tcp,udp'}]`
  - hysteria → `[{label:'UDP',value:'udp'}]`
- [ ] **Step 2: 扇出口径函数**:新增 `expandLinesByTransport(transports, carriage)` —— 对每个选中传输生成一条线路、承载值取 `carriage`(合并字符串);承载内的 tcp/udp/xudp **不再各拆一条**。
- [ ] **Step 3: 类型校验**

Run: `cd frontend && npm run build`
Expected: 构建通过(TS 无类型错)。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/access-lines/format.ts
git commit -m "feat: split network mode into transport + combinable carriage"
```

### Task 5: 本机出口 + 入口表单接新承载选项 + 合并扇出

**Files:** Modify `frontend/src/views/access-lines/localExitLines.ts`、`frontend/src/views/access-lines/LocalExitLinesDialog.vue`、`frontend/src/views/access-entries/entryPayload.ts`(或入口扇出处)

- [ ] **Step 1**:两个表单的「网络模式」多选 → 改为「传输多选(可多条)+ 承载单选(合并)」;提交载荷时按 Task 4 的 `expandLinesByTransport` 生成 `lines[]`:SS 选 TCP+UDP 只产 1 条 `network_mode:"tcp,udp"`。
- [ ] **Step 2: 构建**

Run: `cd frontend && npm run build`
Expected: 通过。

- [ ] **Step 3: Playwright 验收**(若入口/本机出口创建在 e2e 覆盖内)

Run: `cd frontend && npx playwright test e2e/runtime-no-mock.spec.ts -g "local-exit|network"`(按实际用例名)
Expected: 通过;否则在 `runtime-no-mock` 补一条断言「SS 选 TCP+UDP → 列表只多 1 条、network=tcp,udp」。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/access-lines/localExitLines.ts frontend/src/views/access-lines/LocalExitLinesDialog.vue frontend/src/views/access-entries/entryPayload.ts
git commit -m "feat: forms emit merged carriage, fan out only across transports"
```

---

## 块4(前端护栏+地址,可并行):Reality 去除 + 地址分流

### Task 6: 域名直连 securityOptions 去 Reality

**Files:** Modify `frontend/src/views/access-entries/AccessEntryDialog.vue:93-115`、`entryConnectionMode.ts:18`

- [ ] **Step 1**:`securityOptions` 的 domain 分支(`addressType.value` 非 ip 非 cf)从 `[Reality, TLS, 普通]` 改为只 `[{ label: 'TLS', value: 'tls' }]`:

```ts
const securityOptions = computed(() => {
  if (props.entryForm.protocol === 'vless') {
    if (addressType.value === 'ip') {
      return [{ label: 'Reality', value: 'reality' }, { label: '普通 VLESS', value: '' }];
    }
    if (isCfAddress.value) {
      return [{ label: 'TLS', value: 'tls' }];
    }
    // 域名直连:只 TLS(用证书),Reality 属 IP 直连、普通 none 不用证书,均不在此。
    return [{ label: 'TLS', value: 'tls' }];
  }
  if (props.entryForm.protocol === 'trojan' || props.entryForm.protocol === 'hysteria') {
    return [{ label: 'TLS', value: 'tls' }];
  }
  return [{ label: 'None', value: '' }];
});
```

- [ ] **Step 2**:`entryConnectionMode.ts:18` 域名直连提示删「Reality」:
  改为 `domain: '域名直连（灰云）：VLESS/Trojan 直连 TLS、HY2、Shadowsocks。'`
- [ ] **Step 3**:`AccessEntryDialog.vue:95` 注释「域名直连三选」→「域名直连只 TLS」。
- [ ] **Step 4: 构建**

Run: `cd frontend && npm run build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add frontend/src/views/access-entries/AccessEntryDialog.vue frontend/src/views/access-entries/entryConnectionMode.ts
git commit -m "fix: drop reality from domain-direct vless security options"
```

### Task 7: IP 直连地址按连接方式分流

**Files:** Modify `frontend/src/views/access-entries/useEntryAddressAutofill.ts`(`nodeDirectAddress` + `applyNodeAddressDefaults`),并把 connectionMode 传入

- [ ] **Step 1**:新增按连接方式取默认监听地址的函数,IP 用 `ipDirectAddress`:

```ts
// IP 直连默认取节点 IP(ip_direct_address),空则回退 publicHost;域名直连取证书/选中域名;CF 取 CF 域名。
export function nodeListenHostForMode(
  node: AccessNodeSummary | null | undefined,
  mode: 'ip' | 'domain' | 'cf',
  selectedDomain = '',
): string {
  if (!node) return '';
  if (mode === 'ip') return (node.ipDirectAddress || node.publicHost || '').trim();
  if (mode === 'cf') return (node.cfDomain || '').trim();
  return (selectedDomain || node.certDomain || node.publicHost || '').trim();
}
```

- [ ] **Step 2**:`applyNodeAddressDefaults` 增加 `mode`(+ 选中域名)入参,`form.listenHost = nodeListenHostForMode(node, mode, selectedDomain)`;调用点(AccessEntryDialog 的 watcher / `handleConnectionModeChange`)传入 `props.entryForm.connectionMode`。确认 `AccessNodeSummary` 含 `ipDirectAddress`(RelayNodeCard 已用 `node.ipDirectAddress`,services/api 类型应有;无则补)。
- [ ] **Step 3: 构建**

Run: `cd frontend && npm run build`
Expected: 通过。

- [ ] **Step 4: Playwright 验收**:`runtime-no-mock` 补/跑「选 IP 直连 → listenHost 默认 = 节点 IP,不是域名」。
- [ ] **Step 5: Commit**

```bash
git add frontend/src/views/access-entries/useEntryAddressAutofill.ts frontend/src/views/access-entries/AccessEntryDialog.vue
git commit -m "fix: ip-direct entry autofills node ip, not cert domain"
```

---

## 集成(主 Agent,块2/3/4 全部完成后)

### Task 8: 统一门禁 + 真机 smoke

- [ ] **Step 1: 后端全量**

Run(容器化):`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test -p xrayc-db -p xrayc-api -p xrayc-access-agent -p xrayc-xray-config`
Expected: FMT_OK、clippy exit 0、全 PASS(含新 part68)。

- [ ] **Step 2: 前端构建**

Run: `cd frontend && npm run build`
Expected: 通过。

- [ ] **Step 3: 真机回归**(可选,按 CLAUDE.md §9/§11):在隔离环境建 SS 本机出口选 TCP+UDP → 确认只 1 条 `tcp,udp`;建域名直连入口 → 协议无 Reality;建 IP 直连入口 → 地址=节点 IP。

- [ ] **Step 4: 不单独提交**(各 Task 已提交);主 Agent 复核各块产物一致性。

### Task 9: 容器化测试命令参考

```bash
docker run --rm -v "$PWD":/app -w /app \
  -e CARGO_HOME=/app/.cargo-docker -e CARGO_TARGET_DIR=/app/target \
  -e DATABASE_URL=postgres://xrayc:change-me@127.0.0.1:5497/xrayc_test \
  -e XRAYC_FIELD_ENCRYPTION_KEYS="new:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ,old:AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM,test:unit-test-field-key" \
  --network host rust:1-bookworm bash -c "rustup component add rustfmt clippy >/dev/null 2>&1; <cmd>"
```
隔离 PG:`docker run -d --name xrayc-test-pg -p 5497:5432 -e POSTGRES_USER=xrayc -e POSTGRES_PASSWORD=change-me -e POSTGRES_DB=xrayc_test postgres:16-alpine`

---

## 并行编排

```
块1(Task1 文档)──→ 合并后并行:
  ├─ 块2 后端(Task2,3)   [agent A]
  ├─ 块3 网络模式(Task4,5) [agent B]
  └─ 块4 护栏+地址(Task6,7) [agent C]
              └──→ 集成(Task8)主 Agent 统一门禁
```
块2/3/4 写入文件不重叠(块3 动 format.ts/localExitLines/LocalExitLinesDialog/entryPayload;块4 动 AccessEntryDialog/entryConnectionMode/useEntryAddressAutofill;块2 动 Rust),可三 agent 同步。
