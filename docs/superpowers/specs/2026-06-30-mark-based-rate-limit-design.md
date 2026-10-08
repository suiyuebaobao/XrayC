# 限速整体改「每用户 mark」设计 spec

- 日期:2026-06-30
- 状态:已获用户批准(用户授权全程自主执行,不再逐步询问)
- 背景前提:XrayC 未正式上线,可直接替换、不留兼容层(用户 2026-06-30 确认)

## 1. 动机
现状限速给「限速用户」分配运行时端口(20000-60999),tc 按端口整形。CF 橙云只代理
443/8443/2053/2083/2087/2096,运行时端口把 CF 入站顶出 443 → CF 线 timeout(节点
66bd0c56 真实故障根因)。运行时端口还带来:单机约 4 万限速用户上限、与本机出口端口撞车、
订阅每用户端口各异等复杂度。

## 2. 已验证结论(真机 spike,测试机 56292bfa,靶机 2a4bde17)
「每用户靠 Xray mark 区分、不靠端口」真机可行:
- Xray 路由 `match user → 各自带 sockopt.mark 的出站`,共享一个入站也能给每用户打不同内核 mark。
- tc 按 fwmark 整形;下行用 `act_connmark` 还原 mark + IFB。
- 矩阵全过(A 限 2mbit / B 限 8mbit,精度 ~95%):
  - 方向×传输:TCP 下 1.92/上 1.83、UDP 下 1.95/上 1.93、HTTP 1.91。
  - 协议:VLESS / VMess / Shadowsocks2022 / Trojan(TLS) / SOCKS5 / HTTP 全部 2/8;HY2 限速 1.90。
- HY2 说明:XrayC 用 xray-core 26.5.9,HY2 协议名是 `hysteria`+`hysteriaSettings.version:2`(非 `hysteria2`),`-test` Configuration OK、真机隧道+限速均通过。

## 3. 新架构 / 数据流
- 每线路 = 单入站,守真实端口(IP 直连=配置端口 / 域名直连=域名端口 / CF=443)。
- 同线路所有用户共用入站,凭据区分(VLESS uuid / Trojan 密码 / SS PSK / VMess id / SOCKS·HTTP 账号)。
- 每个限速用户 → 确定性 `mark` + 一条「带 `sockopt.mark` 的出站(到其指定出口)」+ 一条
  `routing {user:[email] → 该出站}`。不限速用户走共享出口出站(无 mark)。
- agent tc 在出口侧按 mark 整形:
  - 上行(用户上传 = 节点→出口 egress)按 fwmark → HTB class。
  - 下行(用户下载 = 出口→节点 ingress)用 `act_connmark` 还原 mark → mirred 到 IFB → HTB class。
  - iptables `CONNMARK --save-mark`(出站 SO_MARK 存进 conntrack 供回程还原)。

## 4. mark 分配
- 专用区间(候选 `0x10000–0xFFFFF`,约 100 万,远超单机用户上限)。
- `hash(user_id)` 落区间 + 按节点去冲突(沿用现 `runtime_listen_ports_for_node` 的确定性去冲突思路)。
- tc fw filter 用掩码隔离限速位,不与其它 fwmark 用途冲突(掩码值实现期与 agent 现有 mark 使用对齐)。
- `XrayUserRateLimit` 已有 `mark`/`class_id` 字段,改为以 mark 为主键驱动 class。

## 5. 改动组件(探查已定位 file:line)
| 文件 | 改动 |
|---|---|
| `crates/core/src/runtime_ports.rs` | 端口分配→mark 分配;`runtime_listen_port_for_user` 恒返回 `line.listen_port`;`user_needs_runtime_port`→`user_needs_rate_limit`;退役 `RUNTIME_PORT_START/END`(20-21)+ 避让本机出口端口逻辑(101-115) |
| `crates/db/src/xray_render.rs` | 限速用户不再拆单用户 runtime 入站(128-178)→并进共享入站;为限速用户生成带 mark 出站 + 路由规则;`rate_limit_entry_for_user`(409-440)填 `mark` |
| `crates/access-agent/src/runtime/limiter.rs` | 删「ingress 按 dport 打 mark」(260-293);下行从按 sport(301-331)改按 fwmark;上行改「节点→出口 egress 按 mark」;保 connmark/IFB/`act_connmark` 降级(39-46,87-93,156-171) |
| `crates/core/src/subscription.rs` | 用户入口端口恒用 `line.listen_port`(169-172),删 runtime 端口覆盖 |
| `crates/xray-config/src/types.rs` | `UserRateLimit`(223-238)按需调整(mark 为主) |

## 6. 本机出口(回环)——唯一带风险设计点
self-hosted 本机出口走 `127.0.0.1` 回环、不经 eth0。方案:lo 段也按 mark/connmark 整形
(sockopt.mark 已确认能落回环上行,真机验证 mark100 +5708 字节)。**实现期第一个真机 E2E 在
有本机出口的 CF 节点 66bd0c56 上验「回环按 mark 整形能否限住」**;若回环整形不灵,退到
「主 Xray↔本机出口走 veth」备选方案。

## 7. 错误处理(保留现有红线)
- limiter 关闭/dry-run/命令失败/mark 冲突 → fail-closed,不 reload 新入站。
- `act_connmark` 不可用 → 优雅降级(跳下行 connmark、保上行、绝不 bail),自动装内核不重启、
  心跳上报「内核待升级·需重启」。

## 8. 文档/规则同步(探查已出清单)
`开发方案.md`(§7.7.1 / §2.2.2 / 行 5、45、424-428、1192、1459-1469)→ `AGENTS.md`(行 68)/
`CLAUDE.md` → `README.md`(行 24)/`文档/说明.md`/`文档/架构/当前方案与运营闭环.md`(128-134)/
`文档/架构/架构说明.md`/`文档/部署/部署说明.md`(46、129、165)/`文档/接口/*` → migration 注释。
术语「按端口限速→按每用户 mark 限速」;`scripts/check-docs-no-stale.sh` 加旧措辞拦截。

## 9. 测试方案(全 UI + 真客户端,零 API —— 用户 2026-06-30 定)
- Playwright 驱动真实后台面板:建 2 套餐、10 用户、配 CF 橙云 + 灰云直连线路(用
  `新的服务器账号密码.txt` 多台真机当节点)。绝不用后台 API/curl/DB 造场景或造数。
- 全协议(VLESS/VMess/SS2022/Trojan/SOCKS5/HTTP/HY2)× 连接方式(CF/IP 直连/域名直连)×
  方向(上/下)× TCP/UDP,真实 xray 客户端发真实流量,真实观测限速生效。
- 多账号并发模拟真实用户操作。
- 网络事实:香港测试机外部 UDP 入站不通,涉 UDP/HY2 按此限制判读。

## 10. 压测方案(3 轮 × 20 分钟)
- 10 账号、套餐 A(限速)/套餐 B(不限速)各 5 用户,全 UI 建。
- 真实客户端持续真实流量;实时监控(面板运行数据 + 节点 tc 计数 + 客户端速率)。
- 每轮 20 分钟 × 3 轮;发现问题 → 修复 → 重测该轮。

## 11. 交付门禁(验收标准)
- `make check-release` 通过 + 镜像新鲜度核对。
- 真机 E2E:CF 线通(66bd0c56,先排 401)+ IP/域名线正常 + 限速 2/8 按 mark 生效 + 本机出口回环限速验证。
- 全协议真机测试通过(脱敏报告记「已覆盖 N 台/N 协议」)。
- 3 轮压测通过,限速套餐严格受限、不限速套餐不受限。
- 文档/规则口径一致,无「按端口限速」残留。

## 12. 非目标(YAGNI)
- 不引入 sing-box/第三方核(HY2 由现有 xray-core 26.5.9 承载)。
- 不做多中转全局累计限速、不解析应用层协议、不新增 UDP 独立速率字段(TCP/UDP 共享 `rate_limit_bps`)。
- 不留运行时端口兼容层(未上线)。
</content>
