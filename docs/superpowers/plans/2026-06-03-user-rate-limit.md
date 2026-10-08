# User Rate Limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Implement per-user smooth TCP+UDP shared bandwidth limiting on each transit node, with user overrides taking priority over plan rates and real-server verification.

**Architecture:** Store plan defaults and nullable user overrides in PostgreSQL. Render effective per-node user rate entries into `AccessConfig`, generate user-marked Xray outbounds with `sockopt.mark`, and reconcile protocol-agnostic `tc` HTB/token bucket rules from access-agent after Xray config applies. TCP and UDP share the same user class and the same `rate_limit_bps`.

**Current Product Boundaries:** The limiter keeps the single public subscription entry `/sub/{token}`. Runtime single-user ports are generated inside that same subscription for strict user limiting; they are not client-device subscriptions, dedicated subscriptions, or a second subscription method. Transit node installation is still server-local: the platform generates install instructions, the server-side script prints an auth code, and the platform adds the node with that auth code. Caddy remains the only Web entry for the control plane.

**Tech Stack:** Rust, sqlx/PostgreSQL migrations, xrayc-xray-config JSON compiler, access-agent, Linux `tc`/iptables, Vue/Element Plus admin UI, shell real E2E scripts.

---

### Task 1: Xray Config Mark Support

**Files:**
- Modify: `crates/xray-config/src/types.rs`
- Modify: `crates/xray-config/src/outbound.rs`
- Modify: `crates/xray-config/src/tests/config.rs`
- Modify: `crates/xray-config/src/tests/common.rs`

- [x] Add failing test `test_compile_outbound_applies_sockopt_mark` that sets `ExitEndpoint.sockopt_mark = Some(65537)` and expects `streamSettings.sockopt.mark == 65537`.
- [x] Run `cargo test -p xrayc-xray-config test_compile_outbound_applies_sockopt_mark -- --nocapture` and confirm it fails because the field is missing.
- [x] Add `sockopt_mark: Option<u32>` to `ExitEndpoint` and inject `streamSettings.sockopt.mark` into every outbound shape.
- [x] Run `cargo test -p xrayc-xray-config -- --nocapture`.

### Task 2: Control Plane Rate Model

**Files:**
- Create: `migrations/202606030002_user_rate_limits.sql`
- Modify: `crates/core/src/model.rs`
- Modify: `crates/core/src/store.rs`
- Modify: `crates/db/src/store/rows/*.rs`
- Modify: `crates/db/src/store/load.rs`
- Modify: `crates/db/src/store/plans.rs`
- Modify: `crates/db/src/store/users.rs`
- Modify: `crates/db/src/store/admin_user_queries.rs`
- Modify: `crates/db/src/admin_read_models.rs`
- Modify: `crates/api/src/dto.rs`
- Modify: `crates/api/src/admin_plans.rs`
- Modify: `crates/api/src/admin_users.rs`

- [x] Add failing db/core tests proving plan rate is serialized and user override beats plan rate.
- [x] Add migration columns `plans.rate_limit_bps BIGINT NOT NULL DEFAULT 0` and `users.rate_limit_bps BIGINT NULL`.
- [x] Add API fields `rate_limit_bps` to plan create/update and user create/update.
- [x] Ensure user rate changes call `mark_all_nodes_dirty_in_tx`.
- [x] Run targeted Rust tests for db and API helpers.

### Task 3: AccessConfig Effective Limits

**Files:**
- Modify: `crates/xray-config/src/types.rs`
- Modify: `crates/db/src/xray_render.rs`
- Modify: `crates/db/src/heartbeat_read_model.rs`
- Modify: `crates/db/src/tests/part01.rs`

- [x] Add failing test that a seeded user with plan `10_000_000` and user override `25_000_000` appears in heartbeat `config.rate_limits[0].rate_limit_bps == 25_000_000`.
- [x] Add `UserRateLimit` to `AccessConfig`.
- [x] Compute deterministic mark/class values for active users on the reported node.
- [x] Generate marked per-user outbound tags only for users with positive effective rates.
- [x] Run `cargo test -p xrayc-db heartbeat rate_limit -- --nocapture` or closest targeted db test.

### Task 4: Agent Limiter Module

**Files:**
- Create: `crates/access-agent/src/runtime/limiter.rs`
- Modify: `crates/access-agent/src/runtime.rs`
- Modify: `crates/access-agent/src/runtime/apply.rs`
- Modify: `crates/access-agent/src/config.rs`
- Modify: `crates/access-agent/src/runtime/tests/config.rs`

- [x] Add failing unit test for deterministic command plan with two users.
- [x] Add settings for limiter enable flag, interface, IFB name, dry-run, and command timeout.
- [x] Implement command plan generation for qdisc, class, fwmark filters, and cleanup.
- [x] Reconcile limiter before applying a positive-rate Xray config, so limiter disabled, dry-run, command failure, or runtime port exhaustion fails closed instead of releasing a new inbound.
- [x] Run `cargo test -p xrayc-access-agent -- --nocapture`.

### Task 5: Frontend Admin Controls

**Files:**
- Modify: `frontend/src/services/api/types/billing.ts`
- Modify: `frontend/src/services/api/normalizers/billing.ts`
- Modify: `frontend/src/views/plans/*`
- Modify: `frontend/src/views/users/UserFormDialog.vue`
- Modify: `frontend/src/views/UsersPage.vue`

- [x] Add failing frontend type/build coverage by wiring new fields into existing form models.
- [x] Add plan default speed input in Mbps.
- [x] Add user override speed input in Mbps with inherited/unlimited states.
- [x] Display effective speed in the users table.
- [x] Run `npm run build`.

### Task 6: Deployment and Docs

**Files:**
- Modify: `scripts/deploy-access-agent.sh`
- Modify: `deploy/access-agent/access-agent.env.example`
- Modify: `docker-compose.yml`
- Modify: `README.md`
- Modify: `开发方案.md`
- Modify: `文档/接口/接口草案.md`
- Modify: `文档/部署/部署说明.md`
- Modify: `文档/测试/真实测试用例.md`
- Modify: `scripts/check-no-legacy.sh`
- Modify: `scripts/check-plan.sh`

- [x] Add Docker capabilities and limiter env variables.
- [x] Replace old "用户级限速下线" statements with the new design.
- [x] Keep legacy guard blocking only deleted old structures, not the new limiter.
- [x] Run `bash scripts/check-no-legacy.sh` and `bash scripts/check-plan.sh`.

### Task 7: Real Private-Inventory Verification

**Files:**
- Modify/Create real E2E script under `scripts/`

- [x] Deploy updated services to the current private server inventory, requiring enough real servers for client, transit, and exit roles; this verifies server-local Agent installation, not platform-side SSH installation.
- [x] Create real plan/user/node/group/line/exit binding.
- [x] Set plan rate lower than user override and verify heartbeat effective rate uses the user override.
- [x] Import the single public `/sub/{token}` subscription on the test client server; runtime ports may differ per limited user but remain part of the same subscription model.
- [x] Run real TCP download and assert measured rate is capped near the configured user rate.
- [x] Send real UDP traffic through the same user path and assert the same limiter class records UDP bytes under the same `rate_limit_bps`.
- [x] Reproduce the fast.com-style upload direction gap and verify a real client upload is capped through the single-user access-port IFB ingress rule.
- [ ] Optional follow-up: change the user rate a second time and verify the cap changes after heartbeat.
- [x] Disable/delete user or line and verify old traffic stops.

### Task 8: Strict Runtime Ingress Isolation

**Files:**
- Create: `crates/core/src/runtime_ports.rs`
- Modify: `crates/core/src/lib.rs`
- Modify: `crates/core/src/subscription.rs`
- Modify: `crates/core/src/subscription/tests.rs`
- Modify: `crates/db/src/subscription_read_models.rs`
- Modify: `crates/db/src/xray_render.rs`
- Modify: `crates/db/src/xray_protocol.rs`
- Modify: `crates/db/src/tests/part01.rs`
- Modify: `crates/xray-config/src/types.rs`
- Modify: `crates/access-agent/src/runtime/metrics.rs`
- Modify: `crates/access-agent/src/runtime/sessions.rs`
- Modify: `crates/access-agent/src/runtime/probes.rs`
- Modify: `crates/access-agent/src/runtime/tests/metrics_sessions.rs`
- Modify: `crates/access-agent/src/runtime/tests/probes.rs`

- [x] Add failing subscription test proving two limited users on the same logical access line get one node each, with distinct non-base runtime ports.
- [x] Add failing heartbeat test proving two limited users on the same logical access line render as two single-user runtime inbounds with the same `source_line_id`.
- [x] Add failing access-agent session and probe tests proving runtime inbound IDs are not reported as database line IDs.
- [x] Add failing runtime-port exhaustion test proving limited users are omitted instead of falling back to an occupied port.
- [x] Add failing limiter application tests proving disabled, dry-run, and tc command failure do not replace/reload Xray with uncapped inbounds.
- [x] Add failing marked-exit probe and access-log session tests proving reports use the source endpoint/line IDs and exact Xray bracket email matching.
- [x] Implement deterministic runtime port assignment in core and reuse it from subscription YAML, subscription JSON, and heartbeat rendering.
- [x] Render positive-rate users as single-user runtime inbounds, while retaining the original logical line ID in `source_line_id`.
- [x] Keep Xray source unchanged; use official Xray user routing, outbound `sockopt.mark`, and Linux TC only.
- [x] Harden the real multi-user traffic UAT script so every temporary user has a positive rate limit, each subscription contains exactly one runtime-port node, all runtime ports are unique/non-base, and the target-node Xray config plus limiter plan are checked before the 20-minute traffic phase.
- [x] Run `cargo test -p xrayc-core -- --nocapture`, `cargo test -p xrayc-xray-config -- --nocapture`, `cargo test -p xrayc-access-agent -- --nocapture`, and `cargo test -p xrayc-db -- --nocapture`.
