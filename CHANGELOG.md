# Changelog

## 2026-07-01

- feat: add database backup management (admin panel + `site_settings.backup_config`) covering full `pg_dump`, off-site `rsync` sync, WAL/PITR (plain archive + `pg_basebackup`) and email delivery; each capability is independently toggleable and composable with custom interval + cron schedules. Full backup can `--exclude-table-data` for 7 observability tables while always keeping billing tables. New admin endpoints `GET/PUT /api/admin/backup/config`, `POST /api/admin/backup/test-and-provision`, `POST /api/admin/backup/run-now?mode=`, `GET /api/admin/backup/state` (all admin-JWT, sanitized responses). Off-site SSH password and email attachment passphrase are stored plaintext in `site_settings` (field-level encryption retired) with write-preserve, `_set` masking and never logged; enabling WAL requires a one-time postgres restart via a controlled, rollback-safe rollout and never blocks the local full backup or postgres.

## 2026-06-07

- 补齐全量公开文档总口径：新增 `文档/架构/当前方案与运营闭环.md`，集中说明唯一订阅、统一出口管理、一层分组、入口管理绑定出口、Agent 安装说明、Caddy only、用户级 TCP/UDP 共用限速和套餐周期规则。
- 同步 README、AGENTS、开发方案、架构、接口、部署、前端、测试和 Superpowers 文档，统一使用“绑定线路”口径，避免废弃的分组入口、客户/设备旧口径、后台服务器安装和旧 Web 双组件口径回流。

## 2026-06-05

- 补齐本机出口服务、用户级 TCP/UDP 共享限速和多用户真实流量 UAT 文档：本机出口服务批量创建自建出口线路到统一出口管理，不自动创建入口或加入分组；限速用户与不限速用户共用同一入站、靠每用户 Xray mark 区分，订阅端口不因限速变化。
- 明确套餐周期规则：基础套餐按 `duration_days` 周期重置，付费套餐到期回基础，付费重复购买、支付成功和兑换码续费均延长有效期、叠加流量并保留已用流量。

## 2026-06-03

- 将出口管理、入口管理和分组主路径最终收敛为“统一出口管理 -> 入口管理绑定出口 -> 分组归类绑定节点 -> 套餐授权分组”：分组只用于绑定节点归类、套餐授权和倍率，中转入口只在入口管理中创建并绑定具体出口线路。
- 修复数据库模式下用户可用入口同步为空时订阅生成器错误回退到同分组全部入口的问题，清空分组、禁用入口或出口不可用后，旧客户端不会继续获得失效入口授权。
- 删除后台服务器自动安装入口，后台只生成 Agent 安装说明；本机出口服务只批量创建统一出口管理中的 self-hosted 出口线路，不自动创建入口或加入分组。

## 2026-06-01

- 将出口管理和分组主路径收敛为“统一出口管理 -> 入口管理绑定出口 -> 分组 -> 套餐授权”：第三方、普通自建和中转节点本机出口服务登记后的出口线路在同一列表维护；订阅只使用唯一 `/sub/{token}`。
- 中转节点绑定线路时新增客户端网络模式选择，非 Reality VLESS 支持 TCP、UDP、XHTTP、XUDP；VLESS Reality 固定 TCP；每条绑定独立选择入口协议和网络模式，并在 VLESS 订阅和 Xray 入站中保留可用网络参数。
- 修复重置订阅链接后旧客户端仍可继续使用的问题：订阅 Token 重置时同步轮换用户节点入站凭据和统计归属 key，清理旧会话/快照并标记中转节点配置待同步；订阅 YAML 与 access-agent 下发配置不再使用稳定用户 ID 作为入站凭据。
- 修复套餐授权节点数量显示错误：后台套餐页区分分组内出口线路数量和客户端可见入口数量；新增 Playwright smoke 覆盖“出口线路数 / 实际订阅数”类问题。
- 补齐管理员邀请码管理闭环：新增 `/admin/invite-codes` 后台页面和菜单，支持管理员批量生成、复制、查看使用状态和删除未使用邀请码；后端新增 `GET/POST/DELETE /api/admin/invite-codes`，管理员生成不受普通用户自助开关和额度限制，已使用邀请码保留追责记录。
- 修正中转入口探测延迟展示：TCP 本机探测小于 1ms 时前端统一显示 `< 1 ms`，避免显示 `0 ms` 被误解为未探测；HY2/UDP 探测继续显示真实等待窗口延迟。

## 2026-05-31

- 默认订阅规则新增海外 AI 强制代理白名单：ChatGPT/OpenAI、Claude/Anthropic、Grok/xAI、Gemini/Google AI、Perplexity、Poe、Cursor、Codeium 和 Windsurf 等域名会优先匹配代理分组，再进入中国地区直连规则，避免规则模式下 AI 被误直连。
- 后台订阅设置新增可视化规则编辑器：管理员可选择规则类型、输入域名或 IP/CIDR、选择代理/直连/拒绝动作并调整顺序；高级文本模式保留批量粘贴能力，底层仍保存为 `default_rules` 字符串数组。

## 2026-05-26

- 补齐出口管理完整增删改查口径：后端新增 `DELETE /api/admin/exit-endpoints/:exit_endpoint_id`；当前删除 endpoint 会同步清理内部运行集合成员引用、分组成员、绑定该 endpoint 的中转入口、`user_access_line_assignments` 和 `user_exit_assignments`，并标记接入节点 dirty；修改 endpoint、出口资源或内部运行成员状态会同步清理不可用分配，self-hosted 归属迁移会同时标记旧/新所属节点 dirty。前端出口管理页展示真实 endpoint，并支持搜索、编辑、探测和删除；管理员后台可见真实配置，公开输出不得泄露真实地址或凭据。
- 修复 HY2/Hysteria2 健康探测口径：access-agent 对 HY2/Hysteria2 中转入口和出口 endpoint 使用 UDP 可达性探测，不再按 TCP 或 `unknown` 处理；手动/定时出口探测任务会携带脱敏 target，本机 self-hosted endpoint 即使没有活跃用户分配也能由所属中转节点领取并探测；本机 UDP 端口明确拒绝时会记为 `unhealthy`，避免未监听 HY2 端口误报健康。
- 默认订阅规则改为“中国地区直连、其他流量走代理”：本机/私有网段、`.cn`、常见国内域名、`GEOSITE,CN` 和 `GEOIP,CN` 默认 `DIRECT`，兜底 `MATCH,PROXY` 继续在生成订阅时改写为实际代理分组。
- VLESS Reality 用户入口固定 TCP，不开放 XHTTP/XUDP；非 Reality 的 VLESS 入口按当前协议能力过滤可选网络模式。历史 VLESS Reality XHTTP 入口会切回 TCP 并标记中转节点重新同步，修复 FlClash 移动端 VLESS 无信号问题。
- 按当日私有清单接入测试中转节点并按地区命名线路；为可用节点生成 VLESS/TCP、Trojan/TCP、HY2/Hysteria 和 Shadowsocks 2022 入口，正式订阅按“测试服务器”线路分组输出。
- 修复 HY2 入站真实不可用问题：Xray Hysteria2 入站用户认证字段改为 `auth`，TLS 入站显式声明 `alpn: ["h3"]`，订阅端 Hysteria2 节点同步下发 `alpn: ["h3"]`。
- 部署包 Xray core 固定升级到 `26.5.9`，避免继续使用 `latest` 指向的旧 26.3.27；当日测试节点已通过 Agent 安装脚本重新部署并确认配置 hash 应用。
- 真实客户端验证发现香港测试机外部 UDP 入站不可达，本轮按“香港不交付 HY2”收口；正式用户订阅按当时协议矩阵输出多条中转入口，真实客户端全量出站和账本入账均已覆盖。
- 本机出口服务和中转节点绑定弹窗的“客户端连接协议”开放 Trojan 与 HY2；HY2 自动绑定 `hysteria` 传输和 TLS/SNI 入站配置，批量重绑已有入口时会同步重建入站配置，避免页面显示协议但后端仍保留旧 VLESS 配置。
- 修复三方 VLESS Reality 单行配置导入：粘贴 Clash/mihomo 风格的逗号分隔配置时，前端会清理字段尾部逗号/分号，避免 `flow`、`server_name`、`fingerprint`、`public_key` 等字段被带入非法分隔符导致 access-agent 渲染 Xray 配置失败。
- 已清理运行库中受影响的日本 Reality 线路字段，重新触发中转节点配置同步；用真实 Xray 客户端验证上游直连、正式订阅入口出站、账本入账和前端导入 smoke，公开记录保持脱敏。
- 文档补充订阅健康过滤口径：线路探测故障默认仍继续显示在订阅中；管理员可在后台订阅设置开启 `block_unhealthy_lines` 后，传统订阅才会屏蔽探测故障线路。

## 2026-05-25

- 加固用户删除和管理员保护：删除单用户、批量删除和管理员状态变更现在使用 PostgreSQL advisory lock 串行化最后管理员校验；删除用户时同步清理用户会话表，避免残留在线会话。
- 加固探针上报幂等：新增探针幂等唯一索引迁移，agent 上报入口/出口探针使用数据库唯一约束防并发重复；runtime loadtest 造数同步改为每条探针独立 `probed_at`，large 压测不再与幂等索引冲突。
- 加固发布脚本安全：runtime loadtest 目标强制使用独立 loadtest 数据库并修正 Makefile forbidden DB 传参；HTTP 压测和真实恢复脚本把 Bearer Token 写入临时 curl config，避免敏感 Header 出现在进程参数里。
- 补齐真实 no-mock 覆盖：管理员 no-mock Playwright 增加健康检查页、用户 CRUD、大小写邮箱搜索和删除清理；测试侧 runtime 归一化补齐出口探针计数字段，避免真实健康页断言读成 NaN。
- 修复订阅读模型和 YAML 口径差异：同一个内部运行集合关联不同线路时，用户订阅读模型不再全局折叠，保持与 YAML 分组输出一致。
- 本轮最终验证重新通过：`env -u DATABASE_URL make check-release` 退出 0；`make check-real-release` 退出 0，并输出 `real-release-gate: status=0`，覆盖真实矩阵、10 分钟稳定性、large 数据库压测、large HTTP/API 压测、认证 HA 和误操作恢复。
- 补齐管理员用户管理 CRUD：后台用户页新增“新增用户”和“删除”操作；后端新增 `POST /api/admin/users` 与 `DELETE /api/admin/users/{id}`，创建用户时生成订阅 Token 和默认/指定套餐订阅，删除用户时清理用户侧订阅、令牌、分配、认证会话、订单、账本和运行快照，并保护当前登录管理员和最后一个启用管理员。
- 调整订阅自动测速策略：客户端 `url-test` 自动选择分组默认关闭，后台仍可手动开启；默认订阅只输出普通 `select` 分组和节点，当前主路径由中转入口绑定的具体线路决定出口，不提供产品层负载、粘性或自动选路面板。
- 新增订阅设置升级迁移：旧库中已存在的 `subscription_config.auto_test_enabled=true` 会自动收口为 false，避免升级后继续默认生成客户端 `url-test` 分组。
- 修复订阅默认规则误指向自动测速分组的问题：`GEOIP/MATCH` 现在默认指向首个普通 `select` 专区分组，自动测速开启时也不再覆盖管理员在普通分组里选择的节点；已用真实 Xray 客户端验证美国线路入口出站命中对应上游。
- 修复销售首页刷新时短暂闪现另一套文案的问题：前端不再先渲染本地兜底销售配置，接口加载期间改为显示骨架屏，接口失败时才使用兜底内容。
- 收口出口管理新增流程：出口管理统一维护三方、普通自建和本机出口服务登记后的线路，支持粘贴 Clash/mihomo YAML、代理 URI、Xray outbound JSON 和半结构化文本后自动识别并预填；中转节点本机出口服务只登记出口管理线路，不创建入口、不加入分组。
- 修复分组管理中“已在出口管理创建但未加入分组”的可见性问题：分组弹窗直接展示入口出口绑定节点；入口只在入口管理中创建并绑定具体出口线路。
- 修复线路分组保存时报 `Unprocessable Entity` 的问题：节点线路下拉现在提交 `access_lines.uuid`，不再把表格序号 `id` 当成 UUID 传给后端。
## 2026-05-24

- 修复 access-agent 流量采集混入上游出口认证用户统计的问题：Xray Stats 解析层现在只接受当前线路前缀或历史兼容线路前缀的用户计数，无线路归属的上游用户计数不再进入用户账本、运行指标或 dropped 快照日志；当日真实测试节点已重新部署验证。
- 清理过期真实测试客户端配置：旧本地测试客户端仍连接已废弃的 VLESS 443 入口，已改用当前正式订阅生成临时客户端复测真实出站，并验证账本扣费和订阅用量同步增长。
- 新增管理员后台“使用教程”页：侧边栏新增入口，页面用静态内容说明出口管理、入口管理绑定出口、分组、套餐授权、用户订阅和日常运维的标准路径，并补充 Playwright smoke 覆盖。
- 修复 Clash/mihomo 订阅客户端分组错误：订阅 YAML 现在按套餐授权分组输出普通 `select proxy-groups`，线路只作为 `proxies` 节点挂在分组下；一条线路即使背后有多个内部出口，客户端也只显示 1 个节点。
- 新增管理员后台用户级流量日志：后台侧边栏新增“流量日志”页，支持先选用户再查看访问 IP、入口、出口端点、真实流量和扣费流量；用户管理页保留单用户快捷入口。access-agent 从中转节点 Xray access log 上报 `client_ip`，中心只在管理员用户级日志中展示，普通用户接口和订阅不回显。
- 加固订阅配置校验：自动测速分组名、套餐授权分组名和线路代理名都会避开同名冲突；节点显示名仍保持线路名称。
- 收口 V2 订阅展示：订阅 YAML 节点名使用线路名称，普通 `select proxy-groups` 使用套餐授权分组名称，自动测速分组只聚合全部节点；当前订阅不携带线路或分组图标字段。
- 用户订阅接口补齐节点归属信息：`GET /api/user/subscription` 的可用节点条目返回 `line_group_name`，前端归一为 `lineGroupName` 后在订阅页展示线路所属分组名称；旧图标字段已废弃。
- 用户页面口径从“节点分组/入口分组”收敛为“可用节点”，页面说明只表达套餐授权下可连接的节点，不把内部 `line_groups` 或出口资源写成用户理解模型。
- 清理运行库历史残留：移除旧 UAT 用户与账本、离线 `real-*` 节点、禁用且未分组的旧入口、无引用旧内部集合和旧 endpoint；当前订阅数据拆为 8 条线路，避免客户端重复显示单一“全部线路”。
- 最终验证记录已补齐：DB/API/core/workspace lib 测试、前端构建、Playwright smoke、Playwright runtime no-mock、真实链路 smoke 和真实流量测试均已执行；真实流量复测看到订阅用量与账本扣费同步增长。公开记录只保留脱敏命令和结果摘要，不写真实 token、密码、公网 IP、真实域名或代理 URL。

## 2026-05-23

- 修复后台修改套餐流量额度后，已绑定用户订阅仍保留旧 `limit_bytes` 的问题；现在会同步现有订阅总量、保留已用流量，并触发线路分配重算和接入节点配置刷新。
- 修复套餐 `PATCH` 的部分更新风险：未传 `traffic_limit_bytes` 时不再被默认值覆盖为 0，避免外部客户端只改名称或状态时误清空套餐和订阅额度。

## 2026-05-22

- 统一 V2 分组模型：出口管理保存真实出口，入口管理把入口与出口组成绑定节点，分组直接包含绑定节点；套餐只授权分组，不维护分组结构；订阅设置不再自定义分组，用户订阅按套餐授权分组自动生成 `proxy-groups`。
- 新增线路国家/图标字段和完整国家/地区图标选择表，线路图标会输出到 Clash/mihomo `proxies[].icon`。
- 加固订阅规则输出：历史 `PROXY` 规则目标会自动改写为实际首个代理分组，同名线路会追加唯一后缀，避免客户端分组合并。
- 完整真实发布门禁已收口通过：`make check-real-release` 覆盖真实 smoke、Playwright no-mock、Worker smoke、接入节点安装清理契约、真实入口协议矩阵、三方出口矩阵、运行态恢复、10 分钟真实稳定性、流量 backlog 回放、隔离 DB large 压测、真实与隔离 Compose HTTP/API large 压测、认证 HA、10 分钟认证 HA 稳定性、误操作恢复和大规模误操作恢复，最终 `status=0`。
- 加固真实流量回放 UAT：从订阅 token 动态解析当前有效用户 key，账本校验绑定入口和用户 key；access-agent 写 backlog 失败时返回错误，避免持久化失败被静默忽略。
- 修复隔离压测和恢复脚本稳定性：Compose 构建使用 host 网络，认证 HA 隔离 Caddy 不再占用宿主 80 端口，误操作恢复订阅断言改为检查出口管理/分组展示名并保持出口凭证防泄露校验。
- 修复 V2 主线覆盖门禁：用户订阅 smoke 的静态断言同步到当前“扁平分组 + 线路”页面口径。
- 修复 HY2/Hysteria2 后台展示：出口管理和内部运行集合明细不再把 HY2 误显示为空账号，改为明确展示 `password/auth` 认证模型。
- 收口 V2 文档主路径：普通运营从出口管理维护第三方/普通自建出口，或在中转节点详情创建本机出口服务，再在入口管理绑定具体出口并由分组归类绑定节点；订阅只暴露中转入口，控制与计费在中转节点/access-agent 链路完成。
- 补充 2026-05-22 日报口径：本机出口服务可被其他中转复用，真实验收必须看到真实客户端流量、access-agent 快照和 `usage_ledgers.traffic_source=access_line` 账本增长。

## 2026-05-20

- 同步 V2 文档口径：用户订阅只输出中转入口，当前订阅主链路支持 VLESS、Trojan 和 Shadowsocks；HTTP、SOCKS、VMess 入站仍为 unsupported，不得回退成 VLESS。
- 补充真实发布门禁说明：`make check-real-release` 覆盖真实 Playwright no-mock、Worker smoke、真实 Agent 安装脚本、真实 access-agent 运行观测、自建出口线路、6 小时长稳、隔离 DB large 压测、隔离 HTTP/API strict large 压测，以及 `socks`、`http`、`vless`、`trojan`、`shadowsocks`、`hy2` 六类第三方出口矩阵；所有私有资产只读取私有 env。
- 明确真实发布结果分层：强门禁覆盖三方出口协议 E2E、真实服务器本地 Agent 安装验证、真实 access-agent 观测、真实订阅导入、账本扣费、六类第三方出口矩阵、6 小时长稳、隔离 DB large 压测和隔离 HTTP/API strict 压测；24h 长稳、多副本认证和误操作恢复属于追加 UAT，未启用时只能记录为未执行追加项。
- 真实发布强门禁前置与协议矩阵阶段已通过多轮：`make check-real-release` 覆盖 env、Docker artifact、real-smoke、Playwright no-mock、Worker smoke、清理契约和 relay 模式六类三方出口真实 E2E；完整强门禁仍必须等待 6 小时长稳、backlog 和压测阶段全部跑完后才能记录为通过。
- 新增隔离库压测 seed 脚本，并把 `RUN_RUNTIME_LOADTEST=1` 发布门禁接入自动 seed + large profile；百万账本、五十万审计和十万运行态数据压测已有隔离阶段通过记录，但不能单独代表完整真实发布强门禁通过。
- 优化运营中心账本排行查询，先聚合 `usage_ledgers` 再 join 元数据，large 压测中排行查询降至 1 秒以内。
- 认证 HA 双 API 副本短 UAT 和误操作恢复短 UAT 已真实执行通过；认证多副本 6h/24h 长稳和大数据误操作恢复演练仍需按对应脚本单独执行。
- 修复 `scripts/runtime-http-loadtest.sh` 的管理员运行态读取矩阵，不再把出口探测触发写接口当作 GET 读接口压测；large HTTP/API 压测新增 strict full 模式和延迟预算，缺少用户、管理员或 agent 阶段会直接失败；隔离 Compose + demo seed 的 large HTTP/API 压测已有阶段通过记录，覆盖健康、套餐、用户订阅/用量、管理员运行态读取和 agent heartbeat/config/metrics/sessions/probes/traffic 阶段。
- 新增 `scripts/runtime-http-loadtest-compose.sh`，真实发布强门禁会启动隔离 PostgreSQL + API Compose 自动跑 strict full large HTTP/API 压测，避免向真实发布库写入 synthetic agent 流量。
- 增加中转出口探测任务租约字段和领取逻辑，heartbeat 领取 queued 探测任务后在租约过期前不再重复下发；管理员重复触发同一出口探测时不再堆叠未完成 queued 任务。
- 增强 `scripts/real-v2-relay-pool-e2e.sh`，支持真实 4 服务器中转验收成功后保留中转、出口、客户端代理和本地隧道，并把长稳 UAT 需要的变量写入未跟踪私有 env。
- 修复 `scripts/real-runtime-stability-uat.sh` 与当前 V2 表结构的兼容问题：心跳/config 检查显式读取 `public.access_nodes`，节点维度账本检查通过 `access_lines` 关联回中转节点；长稳每轮追加真实下载流量和短暂 settle，避免短请求被 access-agent 会话采样漏掉。
- 新增 `scripts/restore-real-runtime-access-deploy.sh` 并接入真实发布门禁：第三方 relay 矩阵结束后会恢复长稳用中转部署、清理目标节点旧 queued 探测和目标服务器旧 backlog，再用真实客户端流量确认 heartbeat、metric、session 和 ledger 全部恢复。
- 真实 4 服务器中转链路已重新部署并通过：订阅只暴露中转入口、真实客户端出站命中配置出口、流量快照和 `usage_ledgers.traffic_source=access_line` 增长，禁用用户和额度耗尽均可剔除并恢复；6h 长稳 UAT 已启动并完成首轮通过，完成前不得写成 6h 已通过。
- 修复 relay 六协议矩阵与长稳并发时的 Xray Stats API 端口冲突：矩阵脚本默认使用独立 `XRAYC_REAL_MATRIX_RELAY_XRAY_API_PORT=11085`，避免读取到长稳中转的 Stats 计数。
- 增强长稳前置链路的公网出口探测：最终写入私有 runtime env 前会在多个公网 IP 服务间重试，区分“代理不可用”和“出口 IP 不匹配”两类失败。
- 修复隔离认证 HA UAT：临时 Compose 使用独立 JWT 密钥和 demo seed 时强制丢弃外部 `ADMIN_ACCESS_TOKEN`，重新登录临时管理员，避免真实发布 token 导致 401 误判。
- 补充本轮实测：隔离 PostgreSQL large 数据量压测、隔离 Compose large HTTP/API 压测、双副本认证短 UAT 和误操作恢复短 UAT 均有通过记录；完整真实发布强门禁仍以 6h 长稳、backlog UAT、隔离 DB large、隔离 HTTP/API strict large 全部完成且最终脚本退出 0 为准。
- 补充 Worker 维护任务与中心侧定时探测口径：Worker 只写入 queued 探测任务，access-agent 回报非 queued 结果后才参与组合状态阈值。
- 同步运维、接口、订阅、管理接口和真实发布资产准备清单，强调公开文档不得记录真实 IP、密码、Token、完整订阅链接或代理 URL。
- 明确当前版本只保留统一账本：发布验收覆盖出口管理绑定路由、订阅防泄露和 access-agent 上报；第三方出口对接和普通自建上游出口保持不变。
- 修复真实发布前置校验：`check-real-release-env` 现在会按默认强门禁校验长稳、隔离 DB large 压测、HTTP/API strict large 压测和当前私有服务器清单的协议矩阵变量；VLESS raw URL 不再误要求 host+port，Reality public key 只在对应安全模式下必需，URL/host 校验更严格。
- 补强 Worker smoke 与 API contract：Worker smoke 现在断言中心 Worker 会按策略写入 scheduled queued 出口探测任务；API contract 增加 `exit_probe_interval_seconds`、`probe_queue_batch_size`、`max_pending_probe_tasks` 字段和范围检查，并兼容顶层 `- name:` 订阅 YAML。
- 本轮真实资产复核按当前私有服务器清单执行：测试服务器均按中转节点能力检查 SSH、Docker 和 Compose；真实客户端只作为临时测试进程或容器运行，新的私有 inventory 已写入未跟踪私有文件并刷新到 active 私有 env。
- 隔离 DB large 压测阶段已重新执行通过：100 用户、3 中转节点、6 中转入口、6 出口 endpoint 种子完成，百万账本、五十万审计和十万运行态数据查询均低于 2 秒预算；6 小时长稳仍在持续运行，跑满前不得写成通过。
