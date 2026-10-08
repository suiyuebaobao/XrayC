# XrayC

XrayC V2 是基于 Rust 与 Xray 单内核的中转节点控制系统，接入节点固定运行 `xray-core` 加 `access-agent`（单内核 Xray），所有入口统一由 Xray 处理。产品主路径保持简单：出口管理只维护出口线路；外部供应商线路、普通自建线路和中转节点本机出口服务登记后的线路都在同一个出口清单里维护，不按来源拆列表；入口管理在中转节点上维护用户可连接入口；入口出口绑定节点把一个入口和一条出口线路组合成一个可分组、可订阅节点；分组只有一层，分组可以是 AI 分组、游戏分组、GPT 分组、视频分组，每个分组直接选择绑定节点；套餐直接授权分组并设置分组倍率，不再设置节点数量限制；订阅设置不再自定义分组。Clash/mihomo 订阅先按用户套餐授权分组过滤绑定节点，再输出这些分组内已启用、已同步、可连接订阅节点；同一入口绑定多个出口时会输出多个节点，非限速运行态可以共用同一个入口端口，但每个节点都有绑定节点级凭据和路由。用户订阅只暴露可连接入口参数和节点名，不暴露真实出口地址、出口凭据或内部出口资源。V2 唯一主线是中心控制面下发配置到 `access-agent` 管理的中转节点，不保留旧 `node-agent` 直连、客户端直连出口或中心侧直连出口作为发布主线。后端、access-agent、worker 和配置编译器必须保持 Rust-only。

## 技术栈

- 后端：Rust + Tokio + Axum
- 数据库：PostgreSQL 16+，迁移文件位于 `migrations/`
- 前端：Vue 3 + Vite + TypeScript + Element Plus
- 节点侧：`xray-core` + `access-agent`（单内核 Xray），所有入口统一由 Xray 处理
- 部署：Docker Compose + Caddy

当前产品、部署和运营闭环总口径见 `文档/架构/当前方案与运营闭环.md`；任何公开文档与该口径冲突时，先同步 `开发方案.md`，再同步 README、规则文件和相关文档。

## 硬性规则摘要

- `开发方案.md` 是最高依据；方案变化必须先更新方案，再同步 README、真实测试用例、当日日报、相关公开文档、代码和测试，禁止文档滞后于方案。
- 后端、worker、access-agent、Xray 配置编译器和业务核心保持 Rust-only，禁止引入 Go、Gin、GORM、Go 版节点侧控制服务或旧 `node-agent` 直连链路。
- 接入节点部署口径是 `xray-core` + `access-agent`（单内核 Xray），所有入口统一由 Xray 处理。Xray 路径保留完整 Xray Stats API 计费和会话观测闭环。
- Rust 注释必须使用中文；每个 `.rs` 文件前 10 行内必须有中文模块级职责说明。
- 开发、测试、部署、审查和文档更新默认使用 subagent 协作；除极小单文件修复外，不允许主线程单打独斗。
- 涉及 Xray、mihomo/Clash、Caddy、PostgreSQL、Docker、Cloudflare 或系统网络行为时，遇到不确定必须先查官方文档或上游主仓库文档，再更新代码、测试和方案，不得凭记忆扩大协议能力；官方依据改变当前判断时，必须同步清理规则、方案和公开文档里的旧口径。
- 官方文档中的协议清单不等于当前产品菜单可以直接开放；入口管理只展示已完成 runtime 编译、订阅输出、用户识别、计费/限速和真实客户端 E2E 的协议组合，VMess、TUIC、ShadowTLS、Naive、HTTP/SOCKS 用户入站等仍是后续扩展项。
- 当前采用新的用户级平滑限速层：套餐提供 `rate_limit_bps` 默认值，用户可设置最高优先级覆盖值；access-agent 在中转节点通过 Xray `sockopt.mark`、tc `connmark` action、IFB 和 `tc HTB` 执行每用户独立 TCP+UDP 共享限速，同一个 `rate_limit_bps` 同时约束该用户 TCP 与 UDP 流量，不新增 `udp_rate_limit_bps`、不解析应用协议，也不恢复旧限速档案或多级分组设计。为保证共享逻辑线路下的上传也严格按用户限速，每条线路只有一个入站、守真实端口（CF=443），全用户共用、靠凭据区分；管理员仍只维护一条逻辑中转入口，用户订阅仍只看到一条节点、端口恒为线路真实端口。每个限速用户对应一个确定性 `mark` + 一条带 `sockopt.mark` 的出站 + 一条 `user` 路由规则，`source_line_id` 保留原始线路 ID 用于统计、探测、会话和账本。access-agent 在出口侧按 fwmark 整形：上行（节点→出口 egress）按 mark 进 HTB class，下行（出口→节点 ingress）用 `act_connmark` 还原 mark 重定向到 IFB 用户 class，避免只限制下载或中转到出口方向。用户 `rate_limit_bps` 变化只改 agent 侧 mark/出站/路由与 limiter 规则、不改订阅端口，真实客户端按常规重新拉取订阅即可。正限速配置必须先成功应用 limiter，limiter disabled、dry-run、命令失败或 mark 冲突无法分配都不得 reload Xray 新入口；该方案只使用官方 Xray 配置能力，不修改 Xray 源码。
- 公开产品口径只保留扁平 V2 入口绑定模型：出口管理 -> 入口管理 -> 入口出口绑定节点 -> 分组 -> 规则库 -> 套餐授权。出口管理维护所有来源的出口线路；入口管理维护中转节点上的用户入口，创建入口时网络模式可多选并按选择顺序展开为多条单模式入口，监听端口从起始端口递增；绑定节点把入口和出口线路组合成订阅节点；分组只有一层，直接选择绑定节点；规则设置集中维护通用规则和可复用规则库，分组只绑定规则库，不直接保存规则内容；套餐直接授权分组并设置分组倍率，不再设置节点数量限制；订阅设置只维护 YAML 基础项，不再自定义分组。本机出口服务主路径是在中转节点详情批量添加该服务器自己创建的出口线路，默认展示 VLESS、HY2、Shadowsocks、Trojan、SOCKS5、HTTP 六类主流自建协议（其中 SOCKS5/HTTP 免证书，后端自动生成 `username`+`password` 凭据，凭据只进 agent 必需配置、不入订阅或公开日志），管理员按协议行选择网络模式、地址、端口、选用的节点域名和少量必要字段，系统自动生成连接配置；保存后只进入出口管理并显示所属服务器，不自动创建入口、绑定节点，也不会加入分组。本机出口支持就地编辑/删除：可改 name/host/port/启用状态/选用的节点域名，以及 `outbound_config`/`stream_config` 协议配置，改配置走护栏重新校验、不绕过（要证书协议在无证书域名节点上仍被拦），改完置所属节点 dirty 触发下一轮 agent 同步收敛。中转节点、入口、出口、本机出口服务全部可编辑。
- 套餐周期按当前单一订阅模型处理：基础套餐不可删除，到期后由 Worker 按基础套餐 `duration_days` 重置新周期，默认基础套餐是 30 天月度口径；付费套餐到期后自动切回基础套餐；付费套餐重复购买、订单支付成功或兑换码续费都从 `max(existing_expires_at, now)` 延长有效期、叠加套餐流量，并保留已用流量。
- 数据库自动备份属于后台运营策略：`database_backup` 默认启用，1 天备份一次，保留 30 天；Worker 使用 `pg_dump --format=custom` 写入 `DATABASE_BACKUP_DIR` 指向的目录，管理平台可以调整开关、间隔和保留天数。
- 真实服务器测试凭据只从本机私有位置读取，不得写入 Git、公开文档、日志或截图；禁止私自结束 SSH 进程或重启 SSH 服务。
- 涉及用户路径、运行态或订阅的改动必须跑 `scripts/real-smoke.sh` 和 Playwright；`frontend/e2e/smoke.spec.ts` 用于浏览器 UI 回归，`frontend/e2e/runtime-no-mock.spec.ts` 才是访问真实后端响应的 no-mock 验收；真实第三方出口验收使用 `scripts/real-access-*.sh`，不得用模拟数据冒充公网出站。
- 发布门禁分两层：`make check-release` 是不依赖私有真实资产的本地完整门禁；`make check-real-release` 是需要私有 env、真实订阅、真实 access-agent 上报、真实服务器本地安装验证、本机出口服务真实 E2E 和三方出口协议矩阵的真实发布门禁。
- 完整真实发布只接受 `make check-real-release` 最终输出 `real-release-gate: status=0` 作为脚本级完成证据；中途打印某个 `gate:` 或单项压测通过不能写成整体发布通过。
- V2 是中转节点控制面架构：订阅只暴露套餐授权分组内绑定节点对应的已启用、已同步且可连接入口名称和入口参数，不暴露内部运行集合、出口管理、真实出口地址或出口凭据；订阅节点必须按入口入站协议和客户端网络模式输出为 VLESS/TCP、VLESS/XHTTP、VLESS/XUDP、VLESS/WS、VLESS/gRPC、Trojan、HY2 或 Shadowsocks 2022，而不是按后台出口协议输出。VLESS Reality 按官方 Xray transport/security 口径只开放 TCP、XHTTP 和 gRPC，不开放 UDP、XUDP 或 WS；非 Reality 的 VLESS 入口才按协议能力开放 UDP、XUDP、WS 等网络模式。
- 用户订阅页统一使用“可用节点”口径；`GET /api/user/subscription` 的可用节点条目返回 `line_group_name`，前端归一为 `lineGroupName`，用于显示所属分组名称。订阅 YAML 不输出 `icon` 字段，节点名和分组名不带图标前缀。公开文档不得把内部 `line_groups` 写成用户侧“节点分组”。
- Trojan 订阅只能使用 `password` 字段表达用户凭据，不输出 `uuid`、`flow` 或 Reality 字段；VLESS 订阅必须保留后台选择的 TCP、XHTTP、XUDP、WS 或 gRPC 客户端网络模式，但 Reality 安全层下只允许 TCP、XHTTP 或 gRPC；Shadowsocks 用户入站必须使用 2022 系列方法并以 `server:user` 形态输出，agent 下发 Xray 入站时使用 `settings.clients` 承载用户密码和统计 email，旧 `aes-*-gcm` 只允许作为内部或出口协议，不进入用户订阅、用户级路由或计费主链路。
- Trojan 用户入站必须使用 TLS/SNI 和有效证书；明文 Trojan、无证书 Trojan 或仅完成配置生成不能作为发布通过证据。
- Agent 安装提供可选“一键安装 Agent”和手动安装说明两条路径。一键安装由平台在本次请求内临时使用 SSH 自动连接服务器、自动上传并执行安装脚本，无需管理员手动上传，SSH 凭据不保存；手动安装说明继续作为受限环境备用路径保留。中转节点保存“客户连接地址”和“SSH IP”两个独立字段，订阅和入口默认值只使用客户连接地址，SSH IP 只用于后台运维记录和一键安装重试预填。一键安装的 SSH 目标必须是 IP（域名尤其橙云 CF 代理无法直连 SSH），后端校验拒绝域名 `ssh_host`；SSH IP 与客户连接地址（域名直连/CF直连/IP直连）无关、不联动，前端 SSH Host 自动填值来源是 `ip_direct_address` 而非客户连接地址。安装时填写证书域名和 ACME 邮箱后脚本会自动申请 SSL 证书，域名会写入 agent 环境用于后续证书状态上报。中转节点卡片展示 access-agent 心跳上报的 SSL 状态，续期 SSL 只对单个节点下发任务，不做跨节点续期。
- 单台中转节点固定只跑一个 Xray 内核，支持多域名模型：节点可有多个直连域名 + 多个 CF 域名，统一登记在 `node_domains` 表（按 `kind∈{direct,cf}` 分两组），旧单 `cert_domain`（灰云直连，HTTP-01 签证书）/`cf_domain`（橙云 CF）字段作为各组主域名向后兼容；IP 直连地址（`ip_direct_address`，给 Reality/SS，免证书）保持独立。3 行接入地址各自可空、至少填一个。`cf_enabled` 由“存在任一 `kind='cf'` 域名”派生，无独立开关。计费仍按 Xray Stats 快照差值、与协议无关。
- 入口（`access_entries.node_domain_id`）和本机出口（`exit_endpoints.node_domain_id`，免证书出口留空）可下拉选用节点某域名，订阅按入口选中域名输出 SNI/地址。支持防封轮换：节点囤多个已签好证书的备用域名，某域名被墙时把入口下拉切到另一个域名即切流量，订阅内容随之更新、客户端刷新订阅迁过去。
- 协议护栏按“选中域名 kind”判定：CF 域名只放 (VLESS|Trojan) + (WS/gRPC/XHTTP) + TLS（含 Trojan-WS，不再限死 VLESS-WS），Reality/HY2/SS/裸 TCP 无法过 CF；直连域名再加 Trojan、HY2、VLESS-TLS；不选域名（留空、免证书）放 Reality / Shadowsocks 2022，本机出口额外允许 SOCKS5 / HTTP。要证书协议在无证书域名节点上须在协议字段校验之前被拦（提示“请先配域名直连地址”）。同节点 enabled 入口端口唯一，CF 入口落 443/8443/2053/2083/2087/2096。
- agent 从心跳下发的 `node_domains` 列表遍历逐个签证书：`kind=direct`→HTTP-01、`kind=cf`→DNS-01（`certbot-dns-cloudflare` + CF API Token，不落控制面库），各域名 `x509 -checkend` 仍有效即跳过。删除仍被入口/出口引用的 `node_domain` 行必须拒绝。
- CF 域名证书走 DNS-01（`cf_cert_mode=dns01`，默认）：用 `certbot-dns-cloudflare` + CF API Token 给橙云域名签**它自己的**真 LE 证书，CF 入口证书锚定 `/etc/letsencrypt/live/{cf_domain}/`，`certbot renew` 自动续期；无 token 时退回 `reuse_direct` 复用灰云证书并要求 CF 控制台 SSL=Full（非 strict）。CF API Token 仿 SSH 凭据，仅安装/续期期用、写节点本机 `/etc/letsencrypt/cloudflare.ini`（0600），**不落控制面库、不入日志/审计/心跳**。失败/卡死的部署任务可在前端删除/取消，`waiting_for_server` 等非终态超时自动标记 failed。详见 `开发方案.md` §2.2.1。
- HY2 出口配置支持 `password` 或 `auth` 两种认证字段；二者只允许进入 access-agent 必需配置，不得出现在订阅、前端明文回显或公开日志中。

## 本地验证

```bash
make check-plan
make check-affected
make check-fast
make check-smoke
make check-runtime
# 使用临时 PostgreSQL 跑 Rust PostgreSQL 集成测试
make check-postgres-integration
# 复用本地镜像跑隔离 Worker 维护任务 smoke
make check-worker-smoke
# 本地完整发布门禁
make check-release
# 真实发布 env 完整性检查，不触发目标服务器 SSH 或三方流量
make check-real-release-env
# 按类别输出真实发布 env 缺口，只显示变量名和状态
make real-release-gap-report
# 从本机私有来源生成未跟踪的真实发布 env 草稿，不打印敏感值
make bootstrap-real-release-env
# 按当前私有测试服务器清单生成协议矩阵资产草稿并检查 SSH/Docker/Compose
XRAYC_SERVER_ACCOUNT_FILE=<private-server-account-file> bash scripts/prepare-real-protocol-matrix-assets.sh
# 用真实登录接口写入 USER_ACCESS_TOKEN/SUB_TOKEN，可选写入 ADMIN_ACCESS_TOKEN
make prepare-real-e2e-auth-env
# 真实发布门禁，只能在私有环境值齐全时运行
make check-real-release
# 真实发布默认包含 10 分钟长稳、backlog、隔离 DB large、HTTP/API strict large、认证 HA 和误操作恢复
# 禁止用跳过开关作为发布通过证据
# 追加 24 小时长稳 UAT；要求多用户、多中转节点、多入口/出口副本变量
RUN_REAL_STABILITY_UAT_24H=1 make check-real-release
# 运行态/账本/审计查询压测；真实发布默认要求隔离测试 PostgreSQL large 档
DATABASE_URL=<test-postgres-url> bash scripts/runtime-loadtest.sh
# 强门禁隔离 HTTP/API strict 压测；临时启动 Postgres + API 后自动清理
bash scripts/runtime-http-loadtest-compose.sh
# 只校验长稳、压测、认证 HA、误操作恢复门禁 profile，不连接真实服务器、不跑长稳、不灌数据
make check-real-release-gate-profiles
# 认证多副本长稳门禁，默认 10m；真实发布默认执行
make real-auth-ha-stability-uat
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
DATABASE_URL=<postgres-url> cargo test -p xrayc-db -- --nocapture
cd frontend && npm run build
cd frontend && E2E_BASE_URL=http://127.0.0.1:8080 E2E_USER_ACCOUNT=demo@example.test E2E_USER_PASSWORD=demo123456 E2E_ADMIN_ACCOUNT=admin@example.test E2E_ADMIN_PASSWORD=admin123456 npx playwright test e2e/smoke.spec.ts e2e/runtime-no-mock.spec.ts
docker compose config --services
BASE_URL=http://127.0.0.1:8080 SUB_TOKEN=demo-token bash scripts/real-smoke.sh
# 需要强制验证 agent traffic 2xx 时，必须提供私有 agent 变量：
STRICT_AGENT_TRAFFIC=1 BASE_URL=http://127.0.0.1:8080 AGENT_TOKEN=<private> ACCESS_NODE_ID=<private> ACCESS_LINE_ID=<private> bash scripts/real-smoke.sh
bash scripts/verify-no-secrets.sh
```

运行态数据库压测必须使用隔离 PostgreSQL。先运行 `scripts/prepare-runtime-loadtest-seed.sh` 准备基础数据，或在 `RUN_RUNTIME_LOADTEST=1 make check-real-release` 中由发布脚本自动 seed；`standard` 默认覆盖 3 个用户、2 个中转节点、3 个中转入口和 3 个出口 endpoint，`large` 默认覆盖 100 个用户、3 个中转节点、6 个中转入口和 6 个出口 endpoint，可用 `RUNTIME_LOADTEST_MIN_USERS`、`RUNTIME_LOADTEST_MIN_NODES`、`RUNTIME_LOADTEST_MIN_LINES`、`RUNTIME_LOADTEST_MIN_EXITS` 调整。脚本沿 `users -> access_lines -> access_nodes -> exit_pool_members -> exit_endpoints` 关系写入临时压测数据，并对账本排行/总计、审计浅/深分页、运行态汇总/路由附加、快照/会话新鲜度和探测状态查询执行 `EXPLAIN ANALYZE` 延迟检查；默认按本轮 marker 清理，不输出数据库 URL。HTTP/API strict 压测由 `scripts/runtime-http-loadtest-compose.sh` 在隔离 PostgreSQL + API Compose 中执行，自动使用 demo seed 准备用户、管理员和 agent 阶段变量，并调用 `scripts/runtime-http-loadtest.sh`；large profile 必须设置 `RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1`，覆盖健康、套餐、用户订阅/用量、管理员运行态读取和 agent heartbeat/config-result/metrics/sessions/probes/traffic 阶段，任一受保护阶段缺少 token 或 ID 都算失败；管理员读取矩阵只包含 GET 读接口，不把出口探测触发写接口当作 GET 压测。输出仍只包含阶段、计数和延迟摘要，不打印真实 URL、token、响应体或 IP。

## Docker 启动

```bash
cp .env.example .env
make up
curl http://127.0.0.1:8080/health
```

只有依赖、Dockerfile、Compose、迁移、部署脚本变化或最终交付验收时，才运行 `make rebuild` 重建镜像；日常开发保持已有容器运行，避免重复构建。`make up`、`make restart`、`make rebuild`、`make rebuild-backend`、`make rebuild-frontend`、`make check-smoke` 和 `make check-runtime` 都会执行 `make check-running-current`，要求运行容器 image id 与当前本地镜像 tag 一致，不能让旧版本容器继续运行。
后端 Dockerfile 使用 BuildKit cache mount 缓存 cargo registry 和 release target，依赖不变时后续重建会明显减少重复编译时间。

## 快速部署 / install.sh

运维侧一键部署脚本 `install.sh` 在**目标机本地**运行，菜单或 `--role` 参数从 5 个角色里选一个，把对应组件装起来：

| 角色 | `--role` | 装哪些 | 带 Caddy |
|---|---|---|---|
| ① 一体机 | `aio` | postgres（本地库时）+ api + worker + caddy（含前端） | ✅ |
| ② 仅前端 | `frontend` | caddy（含前端，反代到后端公网地址） | ✅ |
| ③ 仅后端 | `backend` | api + worker（对外暴露端口） | ❌ |
| ④ 仅数据库 | `database` | postgres | ❌ |
| ⑤ 镜像仓库 | `registry` | registry + caddy（仅给 registry 上 HTTPS） | ✅ |

`install.sh` 只装管理平台，**不含 node 节点角色**；node 节点仍走管理面板现有的一键 SSH 安装（`deploy-access-agent`），不变。

用法示例（占位域名，按需替换）：

```bash
# 交互菜单（目标机本地运行）
./install.sh

# 一条命令装（脚本由自建中心机 Caddy 用 HTTPS 提供）
curl -fsSL https://registry.example.com/install.sh | bash

# 非交互装“仅前端”，同源反代到后端公网地址
./install.sh --role frontend --domain panel.example.com --api-upstream http://backend.example.com:8000

# 非交互装“一体机”（本地库）
./install.sh --role aio --domain panel.example.com --acme-email ops@example.com
```

镜像统一从**自建 registry** 公开拉取（HTTPS、无需 `docker login`），第三方基础镜像（`postgres`、`registry:2`、caddy 基础镜像）也搬进自家仓库 re-host，**装机完全不依赖 Docker Hub/ghcr**，规避国内拉取被墙。前后端分离体现在部署：前端 caddy 同源反代到后端公网地址，鉴权（JWT `Authorization` 头 + HttpOnly+Secure Refresh Cookie，host-only + `SameSite=Lax`）在公网拆分下无缝穿透——所以浏览器只连前端域名、前端同源反代到后端是鉴权决定的唯一正确走法。一体机/仅后端安装时脚本自动建**初始真实管理员**（账号 `admin`，随机密码装完仅打印一次，区别于 demo 种子），并置 `XRAYC_ENV=production`、`SEED_DEMO_DATA=false`、`XRAYC_REFRESH_COOKIE_SECURE=true`。脚本只管本机、幂等可重跑（不动数据卷），端口被陌生服务占用时立即中止报告、绝不停别人的服务。已在真机试验通过。完整设计与角色输入/密钥/健康检查口径见 `文档/部署/2026-06-24-一键部署脚本设计.md`。

开发中先运行 `make check-plan` 查看最小验证清单，再用 `make check-affected` 执行当前改动影响范围的最小门禁；小改也可用 `make check-fast`，只需要脚本和 API 运行态 smoke 时用 `make check-smoke`，需要浏览器路径时再用 `make check-runtime`，交付前才用 `make check-release`。只改前端时优先 `make rebuild-frontend`，只改后端运行态时优先 `make rebuild-backend`，不要默认全量 `make rebuild`；涉及运行态验证时必须先通过 `make check-running-current`。

`make check-release` 是本地完整门禁，覆盖格式、Clippy、workspace 测试、前端构建、脚本语法、防泄露、临时 PostgreSQL 集成测试、隔离 Docker runtime、Worker smoke、真实 smoke、严格 API contract、Playwright runtime no-mock 和 Compose 配置，不要求私有三方出口凭据。`make check-real-release-env` 只做私有 env 完整性、占位符、文件存在性和粗粒度格式/安全策略检查，不触发目标服务器 SSH、真实安装或三方流量。`make check-real-release` 是真实发布门禁，必须在私有环境值齐全时运行，且必须使用真实订阅、测试服务器上的 mihomo 真实客户端代理出网、真实 access-agent 运行数据上报、真实服务器 Agent 安装、用户入站矩阵、access-agent backlog 恢复、本机出口服务真实 E2E，以及 SOCKS、HTTP、VLESS、Trojan、Shadowsocks、HY2 三方出口协议矩阵；完整发布矩阵必须使用 `REAL_RELEASE_MATRIX_MODE=relay`，其他专项脚本只能作为脱敏诊断证据，不能替代完整发布闭环。relay 矩阵在安装中转前会从中转服务器对 SOCKS/HTTP 上游做真实代理握手，对 TCP 类上游做连通预检；预检失败表示上游资产不可用，必须先刷新私有协议上游或供应商线路。真实 Playwright no-mock 门禁使用 `E2E_BASE_URL`、`E2E_USER_ACCOUNT/E2E_USER_PASSWORD` 和 `E2E_ADMIN_ACCOUNT/E2E_ADMIN_PASSWORD` 访问真实后端，禁止 `page.route` mock。脚本最终成功时会输出 `real-release-gate: status=0` 脱敏汇总行；没有该行不能写成完整真实发布通过。

Agent 安装提供可选自动 SSH 安装和手动安装说明两条路径。管理员使用“一键安装 Agent”时，平台只在本次请求内使用 SSH 主机、端口、用户、密码或私钥，自动连接目标服务器、自动上传安装脚本和依赖并执行，完成远端执行后不得保存这些凭据；一键安装的 SSH 目标必须是 IP（域名尤其橙云 CF 代理无法直连 SSH），后端校验拒绝域名 `ssh_host`，SSH IP 与客户连接地址互不联动；一键安装会先验证中心与下载制品，再以独立 Compose 实例切换运行服务；旧容器及日志、原配置、累计计数与待补送队列保留，失败恢复旧实例，不执行跨项目清理；安装成功后脚本输出节点鉴权码，平台解析鉴权码并自动创建中转节点，并把本次 SSH 主机保存到中转节点的 `ssh_host` 运维字段。中转节点的 `public_host` 是客户连接地址，进入订阅、入口默认值和 SSL 检查；`ssh_host` 是服务器 SSH 登录 IP（一键安装必须是 IP），只在后台编辑、重试安装和运维记录中使用，与客户连接地址无关、不联动，两者可以不同。手动安装说明仍可用于受限环境：管理员把模板复制到服务器本地并替换 `XRAYC_DEPLOY_ARTIFACT_TOKEN`，再执行 `scripts/deploy-access-agent.sh`，脚本同样输出节点鉴权码用于平台绑定。平台生成安装任务时会创建部署任务，脚本使用一次性上报 token 回传步骤、进度、完成状态和脱敏错误摘要；中转节点页按服务器地址聚合未完成或失败的部署任务，已成功完成的任务不再显示在卡片中。点击地址进入详情查看历史、安装对象、SSH 鉴权方式、安装目录、SSL 申请信息、连接服务器、远端执行安装、下载容器、启动服务、登记节点等阶段，失败或超时任务提供“重试安装”，只回填非敏感参数，管理员需重新填写 SSH 凭据。中转节点需要提供本机出口能力时，在节点详情打开“本机出口服务”，一次性添加该服务器自建的多条出口线路；默认协议行是 VLESS、HY2、Shadowsocks、Trojan、SOCKS5、HTTP，其中 SOCKS5/HTTP 免证书，由后端自动生成 `username`+`password` 凭据，凭据只进 agent 必需配置、不入订阅或公开日志；表单按协议展示 UUID、密码、SNI、证书路径或加密方法等必要字段，并可下拉选用节点某域名（护栏按选中域名 kind 判定），不再要求管理员填写连接 JSON；服务地址默认读取所属中转节点地址，管理员只在默认值不符合实际监听地址时覆盖。VLESS Reality 本机出口固定 TCP，只需要选择 Reality 并填写 UUID/SNI，Reality public key、private key 和 short id 由后端自动生成。本机出口支持就地编辑/删除（改 name/host/port/启用/选用域名 + `outbound_config`/`stream_config` 配置，走 `GET/PUT/DELETE /api/admin/access-nodes/{id}/local-exit-lines` 护栏重校验，改完置节点 dirty）。中转节点、入口、出口、本机出口服务全部可编辑。该服务只登记为出口管理中的可复用出口线路，与第三方和普通自建线路放在同一列表里维护，不自动创建入口、绑定节点，也不会加入分组。

日常运营主路径必须先通过出口管理维护出口线路，再到入口管理维护中转节点入口并创建入口出口绑定节点，然后在分组里选择绑定节点，最后让套餐授权分组。出口管理使用 `/api/admin/exit-resources` 与 `/api/admin/exit-endpoints` 创建、查询、编辑和删除出口档案；删除出口线路时会同步清理引用该出口的绑定节点、分组成员、用户入口分配和用户出口分配，并标记相关中转节点待同步。停用出口线路或把内部兼容成员改为不可新分配时，会清理内部兼容映射，必要时禁用无可用出口的绑定节点。中转节点本机出口服务使用 `/api/admin/access-nodes/{id}/local-exit-lines` 批量创建自建出口线路，出口管理来源列显示“自己创建 / 所属中转服务器”。旧手动刷新类按钮已删除；系统内部在资源、入口、绑定节点、分组、套餐、用户状态和 agent 同步结果变化时自动收敛。

生产环境必须设置 `XRAYC_ENV=production`、`DATABASE_URL`、高强度 `JWT_SECRET`、`JWT_EXPIRES_IN=30m` 或更短、`JWT_REFRESH_EXPIRES_IN=7d` 或更短，并设置 `SEED_DEMO_DATA=false`。支付功能当前暂停，默认 `XRAYC_PAYMENT_ENABLED=false`；订单查询接口保留，订单创建和支付回调写接口返回暂停状态，不创建待支付订单、不处理回调。后续接入真实支付时再显式设置 `XRAYC_PAYMENT_ENABLED=true`，并提供高强度 `PAYMENT_CALLBACK_SECRET` 和非占位 `PAYMENT_RECEIVE_ADDRESS`。生产模式默认会给 Refresh Cookie 追加 `Secure` 属性，且不允许显式关闭。数据库自动备份默认每天一次写入 `DATABASE_BACKUP_DIR`，管理平台可调整自动备份开关、间隔和保留天数，环境变量和部署产物仍需单独备份。

## 运行数据接口

V2 运行数据必须由中转节点侧的 `access-agent` 主动上报到中心控制面。中心接收 `/api/agent/access/metrics`、`/api/agent/access/sessions` 和 `/api/agent/access/probes`，分别写入中转入口运行指标、在线会话摘要、本机入站入口探测和内部 `access_node -> exit_endpoint` 组合探测结果；组合探测通过连续失败/恢复阈值驱动当前中转节点的可用性状态和配置收敛，不把单个节点的失败写成全局出口故障。access-agent 的探测按协议执行，HY2/Hysteria2 的中转入口和出口 endpoint 使用 UDP 可达性探测，TCP 只用于 TCP 类协议；本机 UDP 端口明确拒绝时必须记为 `unhealthy`，不能把只完成 UDP send 写成健康。所有请求都必须使用 `Authorization: Bearer <agent_token>`，公开文档和日志不得记录真实 Token、服务器 IP、密码或代理凭据。

运营中心只展示真实观测数据：真实在线用户数、连接数、上下行速率和探测延迟均来自 access-agent 在中转节点本机采集的指标、会话和探测结果，不得由中心侧猜测或由内部出口 endpoint 反推。运营中心会基于真实心跳、SSL 证书状态和数据库备份状态派生生产告警，当前覆盖中转节点离线、证书缺失/失效/临期和数据库备份失败，并显示告警数量汇总。健康检查页的流量统计来自保留期内 `usage_ledgers.traffic_source=access_line` 明细，以及过期明细汇总出的 `usage_daily_rollups` 和 `usage_hourly_rollups`；今日、本周、本月和累计使用日汇总，总量排行使用日汇总，小时峰值和小时窗口使用小时汇总。用户级“流量日志”展示保留期内明细，以及迁移时标记保护的既有明细。用户管理里的“设备管理”只是管理员 IP 观察列表，合并 `/sub/{token}` 成功拉取来源和 access-agent 上报的真实节点使用来源，相同 IP 只显示一条，不改变系统唯一订阅方式，也不新增第二套用户凭据。中心侧定时主动探测只用于发现中转入口和 `access_node -> exit_endpoint` 组合探测任务是否持续产生、探测队列是否积压，以及驱动运营中心延迟/故障展示和当前接入节点出口 failover；它不能替代真实客户端流量、不能冒充 access-agent 上报、不能用中心机公网请求估算用户出口。queued 探测任务会带租约和投递计数，反复租约过期达到上限后由 Worker 转为 `timeout` 结果并推动组合状态机，避免任务永久积压。验证运行数据时必须使用真实 Docker 环境、Playwright smoke/no-mock，以及对真实接口的 curl 或脚本模板验证。

当前仓库处于 XrayC V2 阶段，核心路径优先验证统一出口管理维护真实出口、入口管理维护中转入口、入口出口绑定节点进入分组、套餐直接授权分组、订阅按授权分组内绑定节点自动生成、agent 心跳、SOCKS、HTTP、VLESS、Trojan、Shadowsocks、Hysteria/HY2 的 AccessConfig 下发、Xray 配置编译、Xray Stats API 配置生成、access-agent 真实累计流量采集和快照差值计费。VLESS 出口校验必须按 `security` 区分：三方和普通自建 VLESS Reality 出口连接别人的服务端，`reality` 才要求 public key、short id、fingerprint 等 Reality 字段且网络模式只允许 TCP，`tls` 只校验 TLS/SNI/ALPN 等 TLS 字段，不得互相套用；中转节点“本机出口服务”的 VLESS Reality 是自己创建的服务，保存时由后端自动生成 public key、private key 和 short id，不要求管理员手填 public key。Cloudflare 橙云入口只能走 HTTP(S) 代理路径，开放 (VLESS 或 Trojan) + (WS/gRPC/XHTTP) + TLS（含 Trojan-WS，不再限死 VLESS-WS），Reality/HY2/SS/裸 TCP 无法过 CF；CF 域名走 DNS-01 用 CF API Token 签它自己的真 LE 证书（`cf_cert_mode=dns01`），无 token 时退回复用灰云证书（`reuse_direct`，要求 CF SSL=Full 非 strict）。订阅生成必须按入口协议输出：VLESS 入口输出 VLESS 节点，Trojan 入口输出 Trojan 节点，Shadowsocks 入口仅允许 2022 系列方法输出 `ss` 节点；Trojan 节点只写 `password`，不得写 `uuid`、`flow` 或 Reality 参数。旧 `aes-*-gcm` Shadowsocks 只允许作为内部或出口协议，不得进入用户订阅、用户级路由或计费主链路。HY2 出口配置可使用 `password` 或 `auth`，仅用于中转节点 outbound 编译。外部供应商线路地址和凭据只允许出现在 agent 必需配置中，订阅、用户侧和公开日志不得泄露；真实供应商链路仍需按协议分别跑公网 E2E 后才能声明生产闭环。
Agent 安装说明仍保留服务器本地执行脚本作为受限环境备用；“一键安装 Agent”是可选自动 SSH 安装，由平台自动连接服务器、上传并执行脚本，无需管理员手动上传，不替代手动说明。部署脚本会提示填写证书域名和 ACME 邮箱；只有两者同时存在才自动申请 SSL 证书，并把域名持久化给 access-agent 做证书状态上报。强制重装会先清理本次域名和旧 env 域名对应的 LetsEncrypt 证书材料，再按本次证书域名重新申请；平台的“续期 SSL”按钮只下发单节点续期任务，agent 不执行任意后台命令，也不把证书敏感内容回传中心。
本轮文档收口记录的最终验证已覆盖 DB/API/core/workspace lib、前端构建、Playwright smoke、Playwright runtime no-mock 和真实流量测试；公开记录只能写脱敏命令、阶段名称和通过摘要，不能写真实 token、密码、公网 IP、真实域名、订阅链接或代理 URL。
API 启动时会读取 `DATABASE_URL` 并自动执行 `migrations/`；开发环境未配置数据库时才回退内存模式，生产环境会 fail-fast。

### 前端工作区整理（2026-10-05）

后台前端按日常管理、运维与系统组织导航，新增绑定线路总览及路径辅助视图；原有入口、出口、分组和管理路由保持兼容。规则与订阅设置按各自字段提交，避免整份回写无关设置。该轮首先在本机演示环境验证，不自动发布或更改正式日志。

本机 UI 演示可在 `frontend` 执行 `npm run preview:ui`。演示 API 仅监听回环地址，使用内存示例资料；不连接正式数据库，也不替代真实业务验收。详见 `frontend/preview/README.txt`。


### 系统优化实施（2026-10-05）

已获授权按总结开发、测试后迁移正式 XrayC。历史路由以独立快照保留身份，删除活动配置不删除账务；Worker 维护分开事务和调度，累计计数基准不按日志保留期清理。节点接入精灵与流量查询共用现有业务接口。已于 2026-10-06 完成隔离验收及既有正式站升级，版本 `2026.10.06-opt1`。完整证据、保留限制及外部未测项目见 `文档/部署/2026-10-06-優化發布驗收報告.txt`。原站 HTTP / development 配置仍保留，不能据此声称已完成 HTTPS 生产配置。
