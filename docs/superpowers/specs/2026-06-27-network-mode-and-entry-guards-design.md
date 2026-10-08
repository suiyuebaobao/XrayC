# 设计:入口网络模式重构 + 连接方式护栏修复

> 日期:2026-06-27 · 状态:待用户复核 · 来源:brainstorming
> 业务最高依据 `开发方案.md`;本设计落地前须先同步 `开发方案.md`(CLAUDE.md §0)。

## 一、背景与要解决的问题

用户审查入口/本机出口创建时发现三个问题:

1. **网络模式「多选扇出多条」把能合并的也拆了**:Shadowsocks 选 TCP+UDP 竟生成 2 条线路,而 SS 一条 `network:"tcp,udp"` 就能同时收 TCP/UDP。根因是把「传输」和「TCP/UDP/XUDP 承载」混在同一个多选框里、一律按选中项扇出。
2. **Reality 错误出现在「域名直连」连接方式下**:按 `开发方案.md:45`,域名直连协议放全族 **TLS**,Reality 属 IP 直连(用 decoy SNI、不碰证书)。前端 `AccessEntryDialog.vue` securityOptions 的 domain 分支误含 Reality,提示文案 `entryConnectionMode.ts:18` 也误列。
3. **IP 直连入口监听地址被自动填成域名**:`useEntryAddressAutofill.ts` 的 `nodeDirectAddress` 一律取 `certDomain || publicHost`,完全不分连接方式;有域名的节点选 IP 直连时,地址被填成域名而非节点 IP(`ipDirectAddress`)。

## 二、范围

- **含**:① 网络模式选择器重构(入口表单 + 本机出口表单)② Reality 连接方式护栏(前端 + 后端)③ IP 直连地址自动填充修复。
- **不含**(明确排除):VMess 暴露(保持原状不动)、XTLS-Vision flow 接入 UI(单独功能,本批不做)。

## 三、A 部分 —— 网络模式重构

### A.1 核心原则:分离两个维度

| 维度 | 能否在一条 inbound 内合并 | UI 处理 |
|---|---|---|
| **传输** `streamSettings.network`:RAW(tcp)/WS/gRPC/XHTTP | ❌ 互斥,一条 inbound 只能一个 | 选多个传输 = 生成多条线路(**保留,本来就对**)|
| **TCP/UDP/XUDP 承载** | ✅ 可合并到一条 | 做成下拉里的**合并选项 = 一条线路**(本次新增)|

> 本次只暴露 RAW/WS/gRPC/XHTTP 四种传输(与现状一致);Xray 另支持 HTTPUpgrade/mKCP,本批不加。

### A.2 各协议「承载」下拉(单选,每项 = 一条线路的 network 承载)

**Shadowsocks**(无传输层,只有协议层 `settings.network`):

| 下拉项 | 落库 network | 线路数 |
|---|---|---|
| TCP | `tcp` | 1 |
| UDP | `udp` | 1 |
| TCP + UDP | `tcp,udp` | **1**(不再拆 2)|

**VLESS — 非 Reality**(先选传输 RAW/WS/gRPC/XHTTP,每条再选承载):

| 承载下拉 | 含义 | packetEncoding |
|---|---|---|
| TCP | 关 UDP | — |
| TCP + UDP | UDP 标准 | none/packet |
| TCP + UDP + XUDP | UDP 全锥优化 | xudp |

**VLESS — Reality**(先选传输 RAW/gRPC/XHTTP,**不含 WS**):

| 承载 | 说明 |
|---|---|
| **仅 TCP** | Reality 常配 Vision flow(仅 TCP),一律不开 UDP/XUDP(用户决定) |

**Trojan**(先选传输 RAW/WS/gRPC/XHTTP):

| 承载下拉 | 落库 |
|---|---|
| TCP | tcp |
| TCP + UDP | tcp + udp relay(Trojan 无 XUDP) |

**HY2**:固定 传输=hysteria、承载=**UDP only**,无下拉。

**VMess**:不动。

### A.3 CF(橙云)连接方式叠加约束(跨协议)

- 仅 VLESS/Trojan;传输仅 **WS/gRPC/XHTTP**(无 RAW);强制 TLS;监听端口限 CF 支持的 HTTPS 端口(443/8443/2053/2083/2087/2096)。
- 承载:VLESS/Trojan 的 UDP 封装进 TCP 流、**可隧道穿过 CF**(承载选项同直连);**HY2 不能走 CF**(原生 QUIC)。

### A.4 线路生成规则(核心改动)

> **同一个传输内的 TCP/UDP/XUDP 合并为 1 条线路;只有选了不同「传输」才生成多条。**
> 例:SS 选「TCP+UDP」→ 1 条;VLESS 选「RAW + WS」两个传输 → 2 条(各自再带承载)。

## 四、B 部分 —— Reality 连接方式护栏

| 层 | 改动 |
|---|---|
| 前端 `AccessEntryDialog.vue` securityOptions | domain(域名直连)分支:`[Reality, TLS, 普通]` → **只 `[TLS]`** |
| 前端 `entryConnectionMode.ts:18` 提示文案 | 域名直连提示去掉「Reality」字样 |
| 前端 `AccessEntryDialog.vue:95` 注释 | 「域名直连三选」口径更正为「域名直连只 TLS」 |
| 后端(`routing_access_entries.rs` 校验) | 防御纵深:`node_domain_id` 指向 kind=direct 时,拒绝 `security=reality`(返回 422) |

## 五、C 部分 —— IP 直连地址自动填充

`useEntryAddressAutofill.ts`:`applyNodeAddressDefaults` 改为**按连接方式分流**取默认监听地址:

| 连接方式 | listenHost 默认值来源 |
|---|---|
| ip(IP 直连) | `node.ipDirectAddress`(节点 IP),空则回退 `publicHost` |
| domain(域名直连) | 选中的 direct 域名 |
| cf(CF 直连) | cf 域名(cdnHostname 另走 CF 分支) |

> 切换连接方式(`handleConnectionModeChange`)时若用户未手改地址,按新连接方式重取默认。

## 六、受影响文件

**前端**:`views/access-entries/AccessEntryDialog.vue`、`entryConnectionMode.ts`、`useEntryAddressAutofill.ts`、`views/access-lines/format.ts`(入口网络模式选项+扇出)、`views/access-lines/localExitLines.ts` + `LocalExitLinesDialog.vue`(本机出口网络模式选项+扇出)。

**后端**:`crates/db/src/store/routing_access_entries.rs`(Reality+direct 护栏)、`routing_local_exits.rs`(`validate_local_exit_network_mode` 接受合并值 `tcp,udp`)、本机出口/入口线路生成处(同传输内不再逐 mode 拆)。

**文档**:`开发方案.md` §2.2.1/§2.2.2(网络模式合并规则 + 连接方式↔协议护栏 + 地址来源),先于代码改。

## 七、测试策略

- **前端单测**(Vitest 若有,否则纯函数单测):① SS 选 TCP+UDP 生成 1 条且 network=`tcp,udp` ② 不同传输才多条 ③ securityOptions domain 分支无 reality ④ IP 模式地址=节点 IP、域名模式=域名。
- **后端 PG 集成测试**:① 入口/本机出口 network=`tcp,udp` 正确落库与编译 ② Reality + direct 域名入口被拒(422)③ IP 直连入口 listen_host=节点 IP。
- **门禁**:`cargo fmt`+`clippy -D warnings`、相关包测试、`frontend npm run build`;涉及用户路径补 Playwright/no-mock(按 CLAUDE.md §9)。

## 八、并行实现拆分(供 writing-plans / 多 agent)

| 块 | 范围 | 依赖 |
|---|---|---|
| **块1 文档** | 先改 `开发方案.md` 三处口径 | 无(最先)|
| **块2 后端** | network 合并值校验/落库/编译 + Reality-direct 护栏 + PG 测试 | 块1 口径 |
| **块3 前端网络模式** | 两个表单的传输/承载选择器 + 扇出规则 + 单测 | 块1 口径 |
| **块4 前端护栏+地址** | securityOptions domain 去 Reality + 地址按连接方式分流 + 单测 | 块1 口径 |
| **集成** | 主 Agent 统一跑 fmt/clippy/全包测试/前端 build,复核各块产物 | 块2/3/4 |

块2/3/4 写入范围基本不重叠,可并行。
