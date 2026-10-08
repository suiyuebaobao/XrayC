# XrayC 客户端自研方案(xray-core · 全平台 · 桌面优先)

> 状态:设计已确认(2026-07-03)。核心选型 xray-core、全平台一把梭、桌面优先、完全自研。
> 客户端代码在独立项目里开发(不在 XrayC 服务器仓库)。**现阶段客户端直接解析 XrayC 现有订阅(Clash/通用),后端零改动**;§5 的「专用客户端配置接口」是**后期可选优化**,现在不做、只保留设计。

---

## 1. 目标与范围

- 自研一款**跨全平台**代理客户端:Windows / macOS / Linux / Android / iOS。
- 内核统一用 **xray-core**,与 XrayC 服务端 100% 同源,所有协议(VLESS Reality/TLS/WS/gRPC/**XHTTP**、Trojan、Hysteria2、Shadowsocks 2022)零损耗兼容。
- **桌面优先落地**(Windows → macOS → Linux),移动端(Android → iOS)二期。
- 最大化复用 XrayC 现有资产:Rust 栈、Vue 前端栈、`crates/xray-config` 配置编译器、订阅系统。

**非目标**:不自研代理内核(复用 xray-core);不做多内核(不引入 sing-box)。

---

## 2. 技术选型总览

| 维度 | 选型 | 理由 |
|---|---|---|
| 代理内核 | **xray-core** | 与服务端同源,全协议兼容(含 XHTTP) |
| 桌面 UI | **Tauri 2 + Vue** | 吃 Rust 栈、复用 Vue 前端资产、体积远小于 Electron |
| 移动 UI | **Flutter** | 一套 UI 覆盖 Android + iOS,生态成熟 |
| 跨端共享逻辑 | **Rust 核心 crate** | 桌面 Tauri 直接调、移动经 `flutter_rust_bridge` 调 |
| 桌面内核集成 | **打包 xray 二进制 + 子进程** | 简单、升级只换二进制、崩溃隔离 |
| 移动内核集成 | **libXray(.aar/.framework)** | iOS 禁止子进程,必须嵌库 |
| 配置生成 | **扩展 `xray-config`(客户端 outbound)** | 服务端/客户端共用一套协议口径 |

---

## 3. 总体架构

```
┌────────────── UI 层 ────────────────────────────┐
│  桌面: Vue(Tauri WebView)        移动: Flutter    │
│         │  Tauri command                │ FRB      │
├────────────── 核心控制层(Rust · 跨端共享)────────┤
│  ● 订阅管理     拉取/解析/更新/多订阅              │
│  ● 配置组装     节点 outbound + 本地 inbound + 路由 │
│  ● 内核控制     XrayController 抽象(start/stop/…)  │
│  ● 系统代理/TUN 各平台适配                          │
│  ● 分流 + DNS   出厂默认策略                        │
│  ● 测速/统计    延迟探测 + Xray Stats               │
├────────────── 内核层 ───────────────────────────┤
│  桌面: xray 二进制(子进程 + 本地 API 控制)         │
│  移动: libXray(FFI 调用)                          │
└─────────────────────────────────────────────────┘
```

**关键原则**:UI 只管展示与交互,一切代理逻辑下沉到 Rust 核心层;核心层对内核只依赖 `XrayController` 抽象,换平台不改上层。

---

## 4. 内核集成:xray-core

### 4.1 统一抽象
```rust
/// 客户端内核控制抽象:桌面(子进程)与移动(libXray)各自实现,上层只依赖它。
pub trait XrayController: Send + Sync {
    /// 用给定 xray 配置启动内核(全量配置,含本地 inbound + 节点 outbound + 路由)。
    async fn start(&self, config: XrayConfig) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    /// 热切换节点/规则:优先走 config reload,不支持则 stop+start。
    async fn reload(&self, config: XrayConfig) -> Result<()>;
    /// 取内核运行态统计(上下行字节、连接数),用于面板与测速。
    async fn stats(&self) -> Result<XrayStats>;
    fn is_running(&self) -> bool;
}
```

### 4.2 桌面实现 —— 子进程
- 随包分发对应平台的 `xray` 可执行文件(`xray.exe` / `xray`)。
- Rust 层写出 `config.json` 到临时目录,`std::process::Command` 拉起 `xray -config`。
- 通过 xray 的 **Stats API(gRPC,本地回环端口)** 取上下行/连接统计。
- 切节点:重写 config + 重启子进程(或用 xray 的 API reload)。
- 崩溃看护:子进程退出即上报 UI + 可配置自动重连。
- 内核升级:替换二进制即可,不涉及重编。

### 4.3 移动实现 —— libXray
- xray-core 编译为 Android `.aar` / iOS `.framework`(参考 XTLS/libXray 或 Hiddify-Xray-core 的构建脚本)。
- 通过 FFI 传入 config JSON、启停内核。
- TUN 由系统 VPN 框架提供(见 §8),内核以 tun inbound 接管流量。
- iOS 需严格控制内存(NetworkExtension 约 ~50MB 上限):精简 geo 数据、按需加载。

---

## 5. 配置来源

> **现阶段(已定):客户端直接解析 XrayC 现有订阅(Clash/通用),后端不改**。§5.2 的「专用客户端配置接口」是**后期可选优化**,先保留设计、现在不实现。

### 5.1 现阶段做法 —— 解析现有订阅(现在就这么用)
- 客户端填**现有订阅 URL** → 拉取 → 解析 XrayC 输出的 **Clash/通用订阅** → 提取各节点连接参数 → 用 `xray-config` 组装 xray 配置(见 §5.3)。
- 优点:**后端零改动**、复用成熟订阅解析生态。客户端与服务端是**镜像视角**(服务端 inbound=用户入口,客户端 outbound=连这些入口)。
- 代价:按 Clash/通用格式解析稍绕,但格式成熟、有现成解析库。

### 5.2(后期可选)订阅新增结构化「客户端节点」输出
后端订阅系统新增一种输出格式(如 `GET /sub/{token}?format=xrayc-client`,管理员 JWT 或订阅 token 鉴权),返回**结构化节点列表**(JSON,而非 Clash YAML):

```jsonc
{
  "version": 1,
  "updated_at": 1720000000,
  "default_rules_version": "2026.07",   // 内置分流规则版本,供客户端判断是否更新
  "nodes": [
    {
      "id": "line-uuid",
      "name": "美国 01",
      "region": "US",
      "server": "cf.example.com",        // 客户端连接地址(域名/IP)
      "port": 443,
      "protocol": "vless",               // vless / trojan / hysteria2 / shadowsocks
      "security": "tls",                 // tls / reality / none
      "network": "ws",                   // tcp / ws / grpc / xhttp
      "uuid": "…",                       // 或 password(trojan/hy2/ss)
      "flow": "",
      "sni": "cf.example.com",
      "host": "cf.example.com",          // ws/xhttp host
      "path": "/xrayc",
      "reality": null,                   // reality 时含 public_key/short_id/dest
      "alpn": ["h2","http/1.1"],
      "extra": {}                        // 协议专属字段透传
    }
  ]
}
```

- **只暴露客户端连接所需参数**,严格不含出口真实 IP/账号/内部集合(沿用 XrayC 订阅防泄露红线)。
- 字段口径直接复用服务端已有的节点/订阅读模型,后端工作量小。

### 5.3 客户端侧:用 `xray-config` 组装完整 config
- 扩展 `crates/xray-config`,新增**客户端 outbound 生成器**:把上面每个 `node` → 一个 xray `outbound`(vless/trojan/hysteria2/ss + streamSettings)。
- 客户端 Rust 层再拼装:
  - **本地 inbound**:socks/http(系统代理模式)或 tun(全局模式);
  - 选中节点的 **outbound** + `direct`/`block` outbound;
  - **routing**(分流规则,见 §9);
  - **dns**(防污染,见 §9)。
- 服务端与客户端**共用同一个 `xray-config` crate**,协议编解码口径永远一致 —— 这是自研相对 fork 的最大红利。

---

## 6. 客户端核心层(Rust)模块划分

```
client-core/                     # 跨端共享 Rust crate(桌面 Tauri / 移动 FRB 都依赖它)
├── subscription/                # 订阅拉取、解析(§5 格式)、多订阅、增量更新
├── config/                      # 组装 xray config(依赖扩展后的 xray-config)
├── controller/                  # XrayController trait + 桌面子进程实现骨架
├── proxy/                       # 系统代理设置(平台适配 trait)
├── tun/                         # TUN 全局(平台适配 trait)
├── rules/                       # 内置分流 + DNS 默认策略,geo 数据管理
├── probe/                       # 节点延迟测速、可用性
└── state/                       # 连接状态机、当前节点、内核统计
```

- 平台相关(子进程 vs libXray、系统代理、TUN)用 trait + 条件编译分离;可测逻辑(订阅解析、配置组装、规则)做成纯函数,单测覆盖。

---

## 7. 桌面端(阶段 1 优先)—— Tauri 2 + Vue

### 7.1 结构
```
Vue(Element Plus,可复用 XrayC 前端风格)
   ⇅ Tauri command / event
Rust(client-core + Tauri 壳)
   ⇅ 子进程 / Stats API
xray 二进制
```

### 7.2 Tauri 命令接口(示例)
```
import_subscription(url) -> Subscription
list_nodes() -> Node[]
connect(node_id, mode: "proxy" | "tun") -> ConnState
disconnect()
current_state() -> ConnState        // + event 推送实时上下行/延迟
test_latency(node_ids) -> {id: ms}
set_routing_mode(mode)              // 全局 / 分流 / 直连
get_logs() / set_settings(...)
```

### 7.3 系统代理模式
- Windows:设置 WinINet/系统代理(注册表 + `InternetSetOption`);
- macOS:`networksetup -setwebproxy/-setsocksfirewallproxy`;
- Linux:GNOME `gsettings` / 环境变量,视桌面环境降级。

### 7.4 TUN 全局模式(阶段 2)
- Windows:**wintun** 虚拟网卡 + tun2socks(或 xray tun inbound);需管理员权限装驱动。
- macOS:`utun` + tun2socks;需授权。
- Linux:`tun` 设备 + tun2socks。
- 统一:TUN 起来后,xray 以本地 socks 承接,路由按 §9 分流。

### 7.5 平台顺序
Windows(先)→ macOS → Linux。一套 Tauri 代码,差异只在 §7.3/§7.4 的平台适配层。

---

## 8. 移动端(阶段 3-4)—— Flutter + libXray

- **UI**:Flutter 一套覆盖 Android + iOS;通过 `flutter_rust_bridge` 调 `client-core`(订阅/配置/规则逻辑复用),内核控制走 libXray FFI。
- **Android**:`VpnService` 提供 TUN;libXray `.aar`;后台保活、开机自启。
- **iOS**:`NetworkExtension`(Packet Tunnel Provider);libXray `.framework`;**严控内存**(~50MB)、精简 geo。
- **上架**:iOS 审核对代理/VPN 类偏严,预留企业号/独立开发者号与合规文案;Android 可直接分发 APK + 上架。

---

## 9. 分流 + DNS 默认策略(出厂内置)

把之前实战踩过的坑(Telegram 打不开、DNS 污染)在客户端**默认配好**:

- **分流**:`geosite:cn`/`geoip:cn` 直连,其余走代理;局域网/回环直连;广告可选 block。
- **DNS**:`fake-ip` 增强模式;国内域名走国内 DNS、国外走代理侧 DNS;`fallback` 按 geoip 校验,避免污染;本地/连通性探测域名不套 fake-ip。
- geo 数据(geosite/geoip)随包或首启下载,`default_rules_version`(§5.2)驱动更新提示。
- 提供「全局 / 分流 / 直连」三档,用户可覆盖。

---

## 10. 独立客户端项目结构建议(供你在别处开发)

```
xrayc-client/
├── client-core/            # Rust 共享核心(§6)
├── xray-config/            # 从 XrayC 复制/子模块引入,扩展客户端 outbound 生成
├── desktop/                # Tauri 2 应用
│   ├── src-tauri/          #   Rust 壳(依赖 client-core)
│   └── src/                #   Vue UI
├── mobile/                 # Flutter 应用(二期)
│   ├── lib/                #   Flutter UI
│   └── rust/               #   FRB 桥接到 client-core
├── core-bin/               # 各平台 xray 二进制 / libXray 产物
└── assets/geo/             # geosite/geoip 数据
```

- `xray-config` 建议以 git submodule 或按需 vendor 方式与 XrayC 同源,协议口径保持一致。

---

## 11. 里程碑

| 阶段 | 内容 | 交付 |
|---|---|---|
| **1(桌面 MVP,Win)** | Tauri+Vue+xray 子进程;**解析现有订阅**/连接/系统代理/切节点/测速 | 可用 Windows 客户端(后端零改动) |
| **2** | 桌面 TUN 全局 + 内置分流DNS + macOS/Linux | 桌面三平台完整 |
| **3** | Android(Flutter + libXray + VpnService) | 安卓客户端 |
| **4** | iOS(NetworkExtension)+ 打磨上架 | 全平台 |

---

## 12. 技术风险与对策

| 风险 | 对策 |
|---|---|
| `xray-config` 客户端视角改造工作量 | 只需新增 outbound 生成,复用现有协议编解码;先覆盖主用协议 |
| libXray 维护(xray 升级重编) | 只影响移动端;跟随 XTLS/libXray 构建脚本,固定内核版本节奏 |
| iOS 审核 + 内存上限 | 提前准备合规文案/账号;精简 geo、按需加载、内存压测 |
| 桌面 TUN 提权/驱动 | 首启引导安装 wintun;失败降级到系统代理模式 |
| 桌面(Tauri/Rust)与移动(Flutter)共享 | 逻辑全下沉 `client-core`,`flutter_rust_bridge` 桥接,UI 各写各的 |

---

## 13. 复用 XrayC 资产清单

| 资产 | 复用点 |
|---|---|
| `crates/xray-config` | 扩展客户端 outbound 生成,服务端/客户端协议口径统一 |
| 订阅系统 | 新增客户端配置输出格式(§5),节点参数来源 |
| Vue 前端栈 | Tauri 桌面 UI 复用组件与风格 |
| Rust 工程经验 | `client-core`、Tauri 壳、跨端共享 |
| 实战踩坑(DNS/分流) | 转化为客户端出厂默认策略(§9) |

---

## 附:关键决策速查
- 内核:**xray-core 统一**(桌面子进程 / 移动 libXray)。
- UI:**桌面 Tauri+Vue,移动 Flutter**,共享 **Rust `client-core`**。
- 配置:**后端订阅结构化输出** → 客户端 **`xray-config` 组装**。
- 落地:**桌面优先**(Win→Mac→Linux)→ 移动(Android→iOS)。
