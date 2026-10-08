# 彻底移除 sing-box 内核 实现方案

**决定**:sing-box 在本系统唯一用途是 AnyTLS;AnyTLS 无法按用户计费/限速(已穷尽验证,含 v1.13 稳定 + v1.14 alpha,v2ray-api/clash-api/路由全绕过)。其余协议(VLESS 含 Reality/XHTTP/XUDP、Trojan、HY2、Shadowsocks)Xray 全支持且可精确计费限速。故**彻底删除 sing-box,回归纯 Xray 单内核**,像它从未存在。

**前置事实**:实时库 0 条 AnyTLS 线路、0 个 sing_box 节点内核——**无数据迁移**,纯代码 + schema。DB 处理走**方案A:删列/删表**(`runtime_core`、`core_type`、`access_node_runtime_cores`)。

**门禁**:每阶段 `cargo check --workspace --all-targets` 绿;最终 `make check` + 前端 `npm run build` + 隔离 Docker 起栈无 sing-box 容器。

---

## 待删的纯 sing-box 文件(整删)
- `crates/xray-config/src/sing_box.rs`、`crates/xray-config/src/sing_box/{access_inbound,common,exit_outbound}.rs`
- `crates/access-agent/src/sing_box_stats.rs`
- 相关测试文件中 sing-box 专属用例

## Phase 1 — Rust 全栈移除(一个协调改动,必须整体编译通过)
**类型层(源头)**:
- `xray-config`:`AccessProtocol` 删 `AnyTls` 变体;删 `sing_box` 模块声明与文件;`compile_sing_box*` 全删;`RuntimeCore`(若在此)删 `SingBox`。
- `access-agent` `config.rs`:`RuntimeCore` 删 `SingBox`;删 `sing_box_binary/sing_box_config_path/sing_box_*_command/sing_box_api_server/sing_box_stats_enabled/grpcurl_binary` 等字段与 env 读取;删能力探测 `probe_sing_box_v2ray_api`。
**级联修复(编译器引导)**:
- `access-agent`:`runtime/apply*.rs`、`core_control.rs`、`stats.rs`(删 sing-box 采集分支与 `sing_box_stats` 调用,回到只读 Xray)、`limiter.rs`(`ingress_protocols_for_line` 删 AnyTls 分支)。
- `db`:`validation*.rs`(删 AnyTLS/`sing_box` 校验,`normalize_runtime_core` 删 sing_box)、所有引用 `runtime_core`/`core_type` 的 SQLx 查询与结构体;删/改节点内核相关 store。
- `core`:`subscription/proxy.rs`(删 `anytls_*`、`runtime_core` 判定)、相关模型。
- `api`:DTO/handler 删 `runtime_core`/AnyTLS 字段与节点内核控制接口。
- `worker`:如有引用一并清。
- **删 sing-box 计费 commit**:我之前 `aed6528`/`0231d2a`/`c230f91` 涉及的新增(v2ray_api 配置、能力门、grpcurl 采集)随本删除一并消失。
- 删所有 sing-box/AnyTLS/runtime_core 相关测试用例(保留并修正仍有效的非 sing-box 断言)。
**门禁**:`cargo check --workspace --all-targets` + `cargo test --workspace` 编译通过、相关单测绿。

## Phase 2 — DB 迁移(方案A:删列删表)
- 新迁移 `2026..._drop_sing_box_runtime.sql`:`DROP TABLE access_node_runtime_cores`;`ALTER TABLE access_lines DROP COLUMN runtime_core`(若存在);删 `access_nodes`/相关表的 `core_type` 列与 `CHECK (... 'sing_box')` 约束;清理只为 sing-box 存在的列/索引。
- 与 Phase 1 的 SQLx 查询改动保持一致(同批提交)。
- 门禁:隔离库 `make check` 迁移可正向应用;集成测试绿。

## Phase 3 — 前端(并行)
- 删入口/线路编辑里的「内核选择 runtime_core」控件、AnyTLS 协议选项;删节点页 sing-box 内核启停控制;删订阅/展示里 AnyTLS 相关分支;删对应 API client 字段/类型。
- 门禁:`npm run build` 通过。

## Phase 4 — 部署 + Makefile(并行)
- `Makefile`:删 `SING_BOX_UPSTREAM_IMAGE/SING_BOX_LOCAL_IMAGE/SING_BOX_IMAGE_PLATFORM`、sing-box 镜像构建 target、产物打包里的 sing-box-image。
- `deploy/`:`access-agent.env.example` 删所有 `XRAYC_SING_BOX_*`、`XRAYC_GRPCURL_BINARY`;compose/部署脚本删 sing-box 容器与挂载;`docker-singbox` 相关。
- 门禁:`docker compose config` 通过、无 sing-box 服务。

## Phase 5 — 文档 + 脚本(并行)
- `开发方案.md`、`AGENTS.md`、`CLAUDE.md`、README、`文档/`:删双内核口径,改为「纯 Xray 单内核」;删 AnyTLS/runtime_core/sing-box 段落,清理 `scripts/check-docs-no-stale.sh` 相关。
- `scripts/`(22):删 sing-box 镜像/容器/矩阵相关步骤与变量。
- 门禁:`make check-docs-no-stale`(或等效)通过、无 sing-box 残留引用。

## 最终集成门禁(主控)
- `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings`
- `make check`(含单文件长度、no-secrets、迁移、集成测试)
- 前端 `npm run build`
- `make rebuild` 起栈 + `check-running-current`:运行容器仅 postgres/api/worker/caddy,**无 sing-box**
- 全仓 `grep -riE 'sing.box|sing_box|anytls|runtime_core'` 仅剩历史日报等不可避免处,核心代码/接口/UI/部署 0 残留
