# Entry Management Binding Nodes and Cloudflare Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class entry management, make `entry + exit` bindings the selectable subscription nodes, keep the visible product name as exit management, and support manual Cloudflare VLESS/Trojan TLS WebSocket or gRPC entries on current private-inventory test nodes.

**Architecture:** Add `access_entries`, `access_entry_exit_bindings`, and `line_group_binding_nodes` while preserving existing `access_lines` compatibility during migration. Subscription and agent runtime continue using stable binding IDs so accounting, rate limits, sessions, and Xray stats remain attributable to existing users. Frontend moves binding workflow from transit nodes to entry management, and groups select binding nodes instead of raw exits.

**Tech Stack:** Rust workspace (`xrayc-core`, `xrayc-db`, `xrayc-api`, `xrayc-xray-config`, `xrayc-access-agent`), PostgreSQL migrations with `sqlx`, Vue 3 + Element Plus frontend, Playwright E2E, Docker Compose runtime.

---

## Guardrails

- Do not reset existing user subscription tokens, package assignments, package grants, or active subscription rows.
- Real tests must create temporary marked data and remove only that marked data.
- Real Cloudflare test nodes must be read from the latest private inventory and private role notes.
  `server_4` is the Cloudflare/orange-cloud node.
  `server_1` is direct/non-CF and must not be used for orange-cloud validation.
  Public examples use RFC 5737 IPs and example domains, such as `203.0.113.10 / cdn-entry.example.test` and `198.51.100.20 / panel.example.test`.
- The "优选 IP" automatic screening menu is out of scope for this plan.
- AnyTLS is a standalone sing-box protocol, not a VLESS transport/security mode. Do not implement or document `VLESS + AnyTLS`; Cloudflare orange-cloud entries use HTTP proxy paths such as TLS + WebSocket or gRPC.

## File Map

- Migration: create `migrations/202606120001_entry_management_binding_nodes.sql`.
- Core model/subscription: modify `crates/core/src/model.rs`, `crates/core/src/store.rs`, `crates/core/src/subscription.rs`, `crates/core/src/subscription/proxy.rs`, `crates/core/src/subscription/profile.rs`, and focused tests under `crates/core/src/subscription/*tests.rs`.
- DB store/read model: modify `crates/db/src/types.rs`, `crates/db/src/store/load.rs`, `crates/db/src/store/rows/routing.rs`, `crates/db/src/store/routing_entries.rs`, `crates/db/src/store/routing_groups.rs`, `crates/db/src/store/assignments_access.rs`, `crates/db/src/store/assignments_exit.rs`, `crates/db/src/read_models.rs`, `crates/db/src/subscription_read_models.rs`, `crates/db/src/xray_render.rs`, and add focused tests in `crates/db/src/tests/part42.rs`.
- API: modify `crates/api/src/lib.rs`, `crates/api/src/admin_lines.rs`, `crates/api/src/admin_nodes.rs`, and add `crates/api/src/admin_entries_tests.rs`.
- Xray/agent compatibility: modify `crates/xray-config/src/inbound.rs`, `crates/xray-config/src/types.rs`, `crates/xray-config/src/tests/inbound.rs`, and `crates/db/src/xray_protocol.rs`.
- Frontend API: modify `frontend/src/services/api/paths.ts`, `frontend/src/services/api/types/routing.ts`, `frontend/src/services/api/normalizers/routing.ts`, `frontend/src/services/api/clients/routing.ts`.
- Frontend views: add `frontend/src/views/AccessEntriesPage.vue`; modify `frontend/src/router/index.ts`, `frontend/src/layouts/AppShell.vue`, `frontend/src/views/LinePoolPage.vue` (legacy filename for the visible exit management page), `frontend/src/views/LineGroupsPage.vue`, `frontend/src/views/AccessLinesPage.vue`, and existing access-lines components only for compatibility links.
- E2E: add `frontend/e2e/smoke/admin-entry-management.spec.ts`; modify `frontend/e2e/smoke/admin-routing.spec.ts`, `frontend/e2e/smoke/admin-core.spec.ts`, `frontend/e2e/smoke/admin-line-pool.spec.ts`, and no-mock runtime tests if current fixtures require it.
- Docs: update `README.md`, `开发方案.md`, `文档/架构/当前方案与运营闭环.md`, `文档/架构/架构说明.md`, `文档/接口/管理接口.md`, `文档/接口/接口草案.md`, `文档/接口/订阅接口.md`, and `文档/前端/页面清单.md`.

---

### Task 1: Schema and Migration

**Files:**
- Create: `migrations/202606120001_entry_management_binding_nodes.sql`
- Modify: `crates/db/src/tests/mod.rs`
- Create or extend: `crates/db/src/tests/part42.rs`

- [ ] **Step 1: Add failing PostgreSQL migration test**

Add a `test_pg_entry_management_migration_preserves_existing_access_lines_when_database_url_is_set` test in `crates/db/src/tests/part42.rs`. It should call `pg_store().await`, query the new tables, and assert that every old `access_lines.id` exists as `access_entry_exit_bindings.id`.

Run:

```bash
XRAYC_FIELD_ENCRYPTION_KEYS="$TEST_FIELD_ENCRYPTION_KEYS" DATABASE_URL="$DATABASE_URL" cargo test -p xrayc-db test_pg_entry_management_migration_preserves_existing_access_lines_when_database_url_is_set -- --test-threads=1 --nocapture
```

Expected before implementation: fail because `access_entries` or `access_entry_exit_bindings` does not exist.

- [ ] **Step 2: Create migration**

Create `migrations/202606120001_entry_management_binding_nodes.sql` with:

```sql
CREATE TABLE IF NOT EXISTS access_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_node_id UUID NOT NULL REFERENCES access_nodes(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    listen_host TEXT NOT NULL DEFAULT '',
    listen_port INTEGER NOT NULL CHECK (listen_port BETWEEN 1 AND 65535),
    protocol TEXT NOT NULL DEFAULT 'vless',
    transport TEXT NOT NULL DEFAULT 'tcp',
    security TEXT NOT NULL DEFAULT '',
    user_uuid TEXT NOT NULL DEFAULT '',
    server_name TEXT NOT NULL DEFAULT '',
    public_key TEXT NOT NULL DEFAULT '',
    short_id TEXT NOT NULL DEFAULT '',
    flow TEXT NOT NULL DEFAULT '',
    udp_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    udp_packet_encoding TEXT NOT NULL DEFAULT '',
    ws_path TEXT NOT NULL DEFAULT '',
    ws_host TEXT NOT NULL DEFAULT '',
    xhttp_path TEXT NOT NULL DEFAULT '',
    xhttp_host TEXT NOT NULL DEFAULT '',
    xhttp_mode TEXT NOT NULL DEFAULT 'auto',
    cdn_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    cdn_provider TEXT NOT NULL DEFAULT '',
    cdn_hostname TEXT NOT NULL DEFAULT '',
    cdn_server TEXT NOT NULL DEFAULT '',
    inbound_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_weight INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS access_entry_exit_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    access_entry_id UUID NOT NULL REFERENCES access_entries(id) ON DELETE CASCADE,
    exit_endpoint_id UUID NOT NULL REFERENCES exit_endpoints(id) ON DELETE CASCADE,
    exit_pool_id UUID REFERENCES exit_pools(id) ON DELETE SET NULL,
    name TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_weight INTEGER NOT NULL DEFAULT 100,
    remark TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS access_entries_node_port_transport_idx
    ON access_entries(access_node_id, listen_port, protocol, transport, COALESCE(NULLIF(ws_path, ''), NULLIF(xhttp_path, ''), ''));

CREATE INDEX IF NOT EXISTS access_entry_exit_bindings_entry_idx
    ON access_entry_exit_bindings(access_entry_id);
CREATE INDEX IF NOT EXISTS access_entry_exit_bindings_exit_idx
    ON access_entry_exit_bindings(exit_endpoint_id);

CREATE TABLE IF NOT EXISTS line_group_binding_nodes (
    line_group_id UUID NOT NULL REFERENCES line_groups(id) ON DELETE CASCADE,
    entry_exit_binding_id UUID NOT NULL REFERENCES access_entry_exit_bindings(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 100,
    PRIMARY KEY (line_group_id, entry_exit_binding_id)
);
```

Then add `INSERT ... SELECT` backfills:

- `access_entries.id = access_lines.id` for one-to-one compatibility.
- `access_entry_exit_bindings.id = access_lines.id`.
- `line_group_binding_nodes` from `line_group_exit_endpoints` joined to `access_lines.exit_endpoint_id`.

- [ ] **Step 3: Run migration test**

Run:

```bash
make check-postgres-integration
```

Expected: new migration tests pass; existing DB/API integration still passes.

---

### Task 2: Core Binding Node Semantics and Subscription Output

**Files:**
- Modify: `crates/core/src/model.rs`
- Modify: `crates/core/src/store.rs`
- Modify: `crates/core/src/subscription.rs`
- Modify: `crates/core/src/subscription/proxy.rs`
- Modify: `crates/core/src/subscription/option_tests.rs`
- Modify or add focused tests in `crates/core/src/subscription/tests.rs`

- [ ] **Step 1: Add failing core tests**

Add tests proving:

- two entries bound to one exit emit two proxies if both binding IDs are in the group
- one entry bound to two exits emits two proxies with distinct names and credentials
- VLESS TLS WebSocket emits `network: ws` and `ws-opts.headers.Host`
- VLESS is never emitted with AnyTLS fields

Run:

```bash
cargo test -p xrayc-core binding_node -- --nocapture
```

Expected before implementation: fail because groups still select raw exit IDs and `ws-opts` is missing.

- [ ] **Step 2: Add model structures**

Add structs to `crates/core/src/model.rs`:

```rust
pub struct AccessEntry { /* fields from migration */ }
pub struct AccessEntryExitBinding { pub id: Uuid, pub access_entry_id: Uuid, pub exit_endpoint_id: Uuid, ... }
```

Keep `AccessLine` as compatibility view. Add `binding_node_ids: Vec<Uuid>` to `LineGroup` while retaining `line_ids` for compatibility.

- [ ] **Step 3: Add stable binding credential helper**

Implement a helper in `crates/core/src/subscription/proxy.rs` or a small sibling module:

```rust
fn binding_credential(protocol: &str, user_credential: &str, binding_id: uuid::Uuid) -> String
```

For VLESS/HY2 use a deterministic UUID v5 from user credential + binding ID. For Trojan/Shadowsocks use a deterministic base64url SHA-256 prefix that remains stable. The helper must not mutate user subscription tokens or user records.

- [ ] **Step 4: Update subscription visibility**

Change `access_line_belongs_to_group` to prefer `LineGroup.binding_node_ids` and use `line.id` as binding node ID. Fallback to old `line_ids` only when `binding_node_ids` is empty.

- [ ] **Step 5: Add WebSocket subscription fields**

In `proxy.rs`, when `line.transport == "ws"`:

```yaml
network: ws
ws-opts:
  path: line.xhttp_path or "/"
  headers:
    Host: line.xhttp_host or line.server_name
```

Cloudflare entries set `server` to `cdn_server` when present, otherwise entry `listen_host`; `servername` and Host use `cdn_hostname` or `server_name`.

- [ ] **Step 6: Run core tests**

Run:

```bash
cargo test -p xrayc-core subscription_ -- --nocapture
cargo test -p xrayc-core -- --nocapture
```

Expected: all core subscription and runtime tests pass.

---

### Task 3: DB Load, Write Paths, and Assignment Compatibility

**Files:**
- Modify: `crates/db/src/types.rs`
- Modify: `crates/db/src/store/load.rs`
- Modify: `crates/db/src/store/rows/routing.rs`
- Modify: `crates/db/src/store/routing_entries.rs`
- Modify: `crates/db/src/store/routing_groups.rs`
- Modify: `crates/db/src/store/assignments_access.rs`
- Modify: `crates/db/src/store/assignments_exit.rs`
- Modify: `crates/db/src/read_models.rs`
- Modify: `crates/db/src/subscription_read_models.rs`
- Modify: `crates/db/src/tests/part42.rs`

- [ ] **Step 1: Add failing DB tests**

Add tests in `part42.rs`:

- `test_pg_group_selects_binding_nodes_not_raw_exits_when_database_url_is_set`
- `test_pg_one_entry_many_exits_are_multiple_subscription_nodes_when_database_url_is_set`
- `test_pg_many_entries_one_exit_are_multiple_subscription_nodes_when_database_url_is_set`

Run each targeted test and confirm it fails before implementation.

- [ ] **Step 2: Load new tables**

Extend `load_store_data` to load `access_entries` and `access_entry_exit_bindings`, then materialize compatibility `AccessLine` rows keyed by binding ID. The materialized `AccessLine` uses entry connection fields and binding exit fields.

- [ ] **Step 3: Change group loading**

Load `line_group_binding_nodes` into `LineGroup.binding_node_ids`. Keep loading `line_group_exit_endpoints` into `line_ids` as compatibility fallback.

- [ ] **Step 4: Add DB write methods**

Add store methods:

- `create_admin_access_entry`
- `update_admin_access_entry`
- `delete_admin_access_entry`
- `list_admin_access_entries_json`
- `list_admin_entry_exit_bindings_json`
- `create_admin_access_entry_exit_binding`
- `update_admin_access_entry_exit_binding`
- `delete_admin_access_entry_exit_binding`
- `replace_admin_line_group_binding_nodes`

Each entry/binding write marks the related access node dirty.

- [ ] **Step 5: Keep legacy line-entry endpoint compatible**

Modify `create_admin_access_node_group_entries` so it writes one `access_entries` row, one `access_entry_exit_bindings` row, and the compatibility `access_lines` row in the same transaction while the old route remains active.

- [ ] **Step 6: Update assignments**

Treat `user_access_line_assignments.access_line_id` and `user_exit_assignments.access_line_id` as binding node IDs during the transition. Assignment sync must only assign binding nodes present in the user's authorized groups.

- [ ] **Step 7: Run DB integration**

Run:

```bash
make check-postgres-integration
```

Expected: DB and API PostgreSQL integration pass.

---

### Task 4: API Routes for Entry Management and Binding Nodes

**Files:**
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/admin_lines.rs`
- Modify: `crates/api/src/admin_nodes.rs`
- Add: `crates/api/src/admin_entries_tests.rs`

- [ ] **Step 1: Add API tests**

Add tests for:

- create/list/update/delete access entry
- bind exit to entry
- replace group binding nodes
- Cloudflare WS validation rejects missing domain/path for `cdn_provider=cloudflare`

Run:

```bash
cargo test -p xrayc-api entry_management -- --nocapture
```

Expected before implementation: fail because routes do not exist.

- [ ] **Step 2: Register routes**

Add routes:

```rust
.route("/api/admin/access-entries", get(list_access_entries).post(create_access_entry))
.route("/api/admin/access-entries/:entry_id", put(update_access_entry).delete(delete_access_entry))
.route("/api/admin/access-entry-exit-bindings", get(list_access_entry_exit_bindings))
.route("/api/admin/access-entries/:entry_id/exit-bindings", post(create_access_entry_exit_binding))
.route("/api/admin/access-entry-exit-bindings/:binding_id", put(update_access_entry_exit_binding).delete(delete_access_entry_exit_binding))
.route("/api/admin/line-groups/:group_id/binding-nodes", put(replace_line_group_binding_nodes))
```

- [ ] **Step 3: Add request DTOs**

DTOs must include entry fields, binding fields, and group binding-node replacement payload:

```json
{ "entry_exit_binding_ids": ["..."] }
```

- [ ] **Step 4: Run API tests**

Run:

```bash
cargo test -p xrayc-api -- --nocapture
```

Expected: API tests pass.

---

### Task 5: Frontend Exit Management, Entry Management, and Group Binding Nodes

**Files:**
- Modify: `frontend/src/router/index.ts`
- Modify: `frontend/src/layouts/AppShell.vue`
- Modify: `frontend/src/services/api/paths.ts`
- Modify: `frontend/src/services/api/types/routing.ts`
- Modify: `frontend/src/services/api/normalizers/routing.ts`
- Modify: `frontend/src/services/api/clients/routing.ts`
- Add: `frontend/src/views/AccessEntriesPage.vue`
- Modify: `frontend/src/views/LinePoolPage.vue` (legacy filename for visible exit management)
- Modify: `frontend/src/views/LineGroupsPage.vue`
- Modify: `frontend/e2e/smoke/admin-entry-management.spec.ts`
- Modify: `frontend/e2e/smoke/admin-routing.spec.ts`
- Modify: `frontend/e2e/smoke/admin-core.spec.ts`

- [ ] **Step 1: Add failing Playwright smoke**

Add `admin-entry-management.spec.ts` that mocks:

- `GET /api/admin/access-entries`
- `POST /api/admin/access-entries`
- `POST /api/admin/access-entries/{id}/exit-bindings`
- `GET /api/admin/access-entry-exit-bindings`

Assert Cloudflare fields submit `transport=ws`, `security=tls`, `cdn_provider=cloudflare`, `cdn_hostname=cdn-entry.example.test`, `ws_path=/vless-ws`.

- [ ] **Step 2: Normalize exit management UI**

Keep visible product text as "出口管理" while retaining `/admin/line-pool` only as a compatibility route alias.

- [ ] **Step 3: Add API client types and methods**

Add `AccessEntrySummary`, `AccessEntryPayload`, `AccessEntryExitBindingSummary`, `AccessEntryExitBindingPayload`, and `LineGroupBindingNodePayload`.

- [ ] **Step 4: Implement `AccessEntriesPage.vue`**

Page sections:

- entry table grouped by transit node
- create/edit entry dialog
- Cloudflare fields shown only when CDN is enabled
- bound exits table under selected entry
- bind exit dialog

- [ ] **Step 5: Change `LineGroupsPage.vue`**

Replace raw exit selector with binding-node selector. The option label must show entry context and exit context. Save via `/binding-nodes` and keep rule library fields unchanged.

- [ ] **Step 6: Run frontend checks**

Run:

```bash
npm --prefix frontend run build
cd frontend && E2E_BASE_URL=http://127.0.0.1:8080 npx playwright test e2e/smoke/admin-entry-management.spec.ts e2e/smoke/admin-routing.spec.ts --project=chromium
```

Expected: build and smoke pass.

---

### Task 6: Cloudflare WebSocket Runtime and Real Validation

**Files:**
- Modify: `crates/db/src/xray_render.rs`
- Modify: `crates/db/src/xray_protocol.rs`
- Modify: `crates/xray-config/src/inbound.rs`
- Modify: `crates/xray-config/src/types.rs`
- Modify: `crates/xray-config/src/tests/inbound.rs`
- Add a focused real test helper under `scripts/` for the Cloudflare entry smoke path.

- [ ] **Step 1: Add Xray config tests**

Add a test that compiles VLESS TLS WebSocket with:

- `listen_port=443`
- `transport=ws`
- `server_name=cdn-entry.example.test`
- `xhttp_path=/vless-ws`
- `xhttp_host=cdn-entry.example.test`
- TLS certificate and key files set

Assert Xray JSON contains `streamSettings.network = ws`, `security = tls`, `tlsSettings.certificates`, and `wsSettings.headers.Host`.

- [ ] **Step 2: Implement Cloudflare WS mapping**

Use the existing Xray `ws` branch and ensure entry fields map to `AccessLine.transport`, `xhttp_path`, `xhttp_host`, `server_name`, and TLS certificate fields.

- [ ] **Step 3: Deploy to current private-inventory test node**

Read the real Cloudflare test node from the latest private inventory and private role notes.
Select `server_4` for orange-cloud validation.
Do not select `server_1`.
Create a temporary test entry/binding/group/user and sync only the selected test node unless the current task explicitly authorizes broader changes. Public examples may use `203.0.113.10 / cdn-entry.example.test`.

- [ ] **Step 4: Verify Cloudflare path**

Run:

```bash
curl --noproxy '*' -fsS --max-time 12 https://cdn-entry.example.test/cdn-cgi/trace
curl --noproxy '*' -kIsS --max-time 12 https://cdn-entry.example.test/vless-ws
```

Expected after WebSocket entry is active: Cloudflare no longer returns `Edge IP Restricted` for the WebSocket path. A plain HTTP probe may return an upgrade-related status; that is acceptable if the real client connects.

- [ ] **Step 5: Real client test**

Use a temporary subscription token for a temporary user. Import into mihomo/sing-box test client from a test server. Verify:

- subscription contains the Cloudflare WS node
- client connects through the private-inventory Cloudflare hostname on port 443; public examples use `cdn-entry.example.test:443`
- traffic exits through the selected exit endpoint
- `usage_ledgers` rows are written for the temporary user and binding node

- [ ] **Step 6: Clean temporary test data**

Delete only temporary test user, temporary test group, temporary entry/binding, and temporary package rows created by this plan. Do not delete existing subscriptions or existing non-test entries.

---

### Task 7: Documentation and Release Checks

**Files:**
- Modify: `README.md`
- Modify: `开发方案.md`
- Modify: `文档/架构/当前方案与运营闭环.md`
- Modify: `文档/架构/架构说明.md`
- Modify: `文档/接口/管理接口.md`
- Modify: `文档/接口/接口草案.md`
- Modify: `文档/接口/订阅接口.md`
- Modify: `文档/前端/页面清单.md`

- [ ] **Step 1: Update docs**

Replace old product wording:

- Visible product name stays "出口管理"; `/admin/line-pool` is only a compatibility route alias.
- groups select binding nodes, not raw exits
- transit node page no longer owns the primary binding workflow
- Cloudflare/CDN belongs to entry management
- no "优选 IP" automatic screening menu in this release

- [ ] **Step 2: Search stale wording**

Run:

```bash
rg -n "中转节点.*绑定.*具体线路|分组.*出口管理|优选 IP|优选IP" README.md 开发方案.md 文档 frontend/src
```

Expected: remaining `/admin/line-pool` references are compatibility notes; visible UI docs use `出口管理`.

- [ ] **Step 3: Full verification**

Run:

```bash
cargo fmt --check
cargo test -p xrayc-core -- --nocapture
cargo test -p xrayc-api -- --nocapture
make check-postgres-integration
npm --prefix frontend run build
BASE_URL=http://127.0.0.1:8080 SMOKE_LOGIN_ACCOUNT=demo@example.test SMOKE_LOGIN_PASSWORD=demo123456 REQUIRE_SUBSCRIPTION_DOWNLOAD=1 bash scripts/real-smoke.sh
```

Expected: all commands pass. If private real-client env is ready, run the temporary Cloudflare WS real client test from Task 6.

---

## Self-Review

- Spec coverage: The plan covers entry management, exit management rename, binding nodes as group members, subscription no-dedup behavior, Cloudflare VLESS TLS WebSocket/gRPC direction, explicit `VLESS + AnyTLS` rejection, no automatic preferred-IP menu, existing subscription preservation, and temporary-test-data cleanup.
- Type consistency: The canonical table names are `access_entries`, `access_entry_exit_bindings`, and `line_group_binding_nodes`. API route names use `access-entry-exit-bindings`.
- Risk note: one-entry-many-exits requires binding-scoped user credentials so Xray can distinguish which binding node the client selected. This is included in Task 2 and must not be skipped.
