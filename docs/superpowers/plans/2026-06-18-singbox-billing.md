# sing-box 计费闭环 实现计划

> **执行方式**:subagent-driven-development,逐任务 TDD,主控集成 + 真机 no-mock 验证。

**目标**:让 `runtime_core=sing_box` 的入口(尤其 AnyTLS)按用户真实流量计费,接入既有的快照差值计费管线,达到与 Xray 路径等价的"真实观测、可计费、可超额剔除"。

**架构**:复用 sing-box 实验性 V2Ray API 的按用户统计(`user>>>email>>>traffic>>>uplink/downlink`,与 Xray StatsService 同格式)。sing-box 入站用户名已是 `user.email`(见 `access_inbound.rs`),故映射层与解析器 `parse_stats_user_email` 直接复用;计费主管线(快照差值/配额/超额)内核无关,已建好,无需改。

**前置(部署)**:节点上的 sing-box 二进制/镜像必须带 `with_v2ray_api` + `with_grpc` build tag(官方默认不含)。

**硬门禁**:CLAUDE.md §4/§9——计费必须真机真流量验证(no-mock);未验证前不得在方案/口径里宣称"sing-box 计费已完成"。

---

## Task 1:xray-config 生成 v2ray_api 统计配置

**Files:**
- Modify: `crates/xray-config/src/sing_box.rs`(`compile_sing_box`,把 `experimental: Null` 改为按需填 `v2ray_api`)
- Test: `crates/xray-config/src/tests/config.rs` 或 `tests/inbound.rs`

- [ ] **Step 1: 失败测试**:给一个含 1 条 sing-box 入口(2 个用户 email=`u1@line`/`u2@line`)的 `AccessConfig`,断言 `compile_sing_box` 输出 `experimental.v2ray_api.listen` 非空、`experimental.v2ray_api.stats.enabled==true`、`stats.users` 含 `u1@line` 与 `u2@line`。
- [ ] **Step 2: 跑测试确认失败**(当前 `experimental` 恒为 Null)。
- [ ] **Step 3: 实现**:新增 `fn sing_box_experimental(config) -> Value`:收集所有入站用户 `name`(= email),无用户则返回 `Null`(不开 api);否则返回 `{"v2ray_api":{"listen": <api_listen 常量,如 "127.0.0.1:8081">, "stats":{"enabled":true,"users":[...去重邮箱],"inbounds":[...入站 tag],"outbounds":[...出站 tag]}}}`。listen 端口设为常量 + 注释说明仅本机。
- [ ] **Step 4: 跑测试通过**。
- [ ] **Step 5: 负向测试**:空 `access_lines` 时 `experimental` 仍为 `Null`(不开 api),通过。
- [ ] **Step 6: commit**。

## Task 2:access-agent 采集 sing-box v2ray-api 统计

**Files:**
- Modify: `crates/access-agent/src/config.rs`(`AgentSettings` 加 `sing_box_api_server: String`,env `XRAYC_SING_BOX_API_SERVER`,默认 `127.0.0.1:8081`,与 Task1 listen 对齐)
- Modify: `crates/access-agent/src/stats.rs`(`collect_user_traffic_snapshots` 的 sing-box 分支)
- Test: `crates/access-agent/src/stats.rs`(`parse_stats_output` 已有解析测试,复用;新增 server 选择测试)

- [ ] **Step 1: 失败测试**:断言当 `runtime_core==SingBox` 时,采集器选用 `sing_box_api_server`、`sing_box_binary`(或复用 xray 二进制作 gRPC 客户端)构造的 statsquery 命令(用一个可注入的命令构造函数做纯函数测试,不跑真进程)。
- [ ] **Step 2: 确认失败**(当前直接返回空)。
- [ ] **Step 3: 实现**:把 server/binary 选择抽成 `fn stats_query_target(settings) -> (binary, server)`:SingBox→(`xray_binary` 作为通用 v2ray StatsService gRPC 客户端, `sing_box_api_server`);Xray→(`xray_binary`, `xray_api_server`)。`collect_user_traffic_snapshots` 去掉早返回,统一用该 target 调 `api statsquery -server <server> -pattern user>>>`,复用 `parse_stats_output`。
- [ ] **Step 4: 跑测试通过**。
- [ ] **Step 5: 更新 `test_collect_user_traffic_snapshots_skips_sing_box_runtime`**:语义从"跳过"改为"用 sing-box server",或替换为命令构造断言。
- [ ] **Step 6: commit**。

## Task 3:部署侧确保 sing-box 带 v2ray_api

**Files:**
- Modify: `deploy/access-agent/access-agent.env.example`(注释说明 sing-box 需 v2ray_api;新增 `XRAYC_SING_BOX_API_SERVER`)
- Modify: sing-box 镜像来源说明 / Dockerfile(若项目自建 sing-box 镜像则加 `-tags with_v2ray_api,with_grpc`;若用第三方镜像则文档标注需带该 tag)

- [ ] 确认/切换到带 `with_v2ray_api`+`with_grpc` 的 sing-box 镜像;在部署文档与 env 示例标注;commit。

## Task 4:真机 no-mock 验证(硬门禁)

- [ ] 读取最新私有清单,选一台可用节点。
- [ ] 部署带 v2ray_api 的 sing-box + 一条 **AnyTLS** 入口(真实证书)。
- [ ] 真实客户端经 AnyTLS 出站、跑真实流量。
- [ ] 确认:agent `api statsquery` 从 sing-box 读到该用户 uplink/downlink 非零 → 上报 → DB 入账(快照差值)→ 配额扣减 → 超额后被剔除。
- [ ] 全部以真实数据为证据,脱敏记日报;失败原因如实记录。

## Task 5:方案/口径同步(仅验证通过后)

- [ ] `开发方案.md`、`CLAUDE.md §4`、README、当前方案总口径:把"sing-box 不得宣称计费闭环"更新为"sing-box 计费已完成(经真机 AnyTLS no-mock 验证)"。**Task4 未通过则本任务不做**,保持现有保守口径。
