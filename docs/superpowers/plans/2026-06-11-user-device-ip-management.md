# User Device IP Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-user "设备管理" view in admin user management that shows deduplicated subscription-pull IPs and real node-use IPs.

**Architecture:** Keep the current single subscription model. Add a lightweight subscription pull event table, record request IP during `/sub/{token}` downloads, then expose an admin read model that unions those events with `access_user_session_events` and deduplicates by plain IP first, hash fallback second.

**Tech Stack:** Rust Axum API, SQLx/PostgreSQL migrations, Vue 3 + TypeScript + Element Plus, Playwright smoke, Rust integration tests.

---

### Task 1: Backend Storage And Read Model

**Files:**
- Create: `migrations/202606110001_subscription_pull_events.sql`
- Modify: `crates/db/src/store/subscription_read.rs`
- Modify: `crates/db/src/store/user_traffic_logs.rs`
- Modify: `crates/db/src/store/mod.rs` only if a new store module is split out
- Test: `crates/db/src/tests/part21.rs` or a new focused test file wired through `crates/db/src/tests/mod.rs`

- [ ] **Step 1: Write the failing PostgreSQL test**

Add a test that inserts one subscription pull event and one node-use session event for the same user and same IP, then calls the new read model and expects one deduplicated row with source flags/counts.

Expected shape:

```rust
#[tokio::test]
async fn test_pg_admin_user_devices_merge_subscription_and_usage_ip_when_database_url_is_set() -> anyhow::Result<()> {
    let Some(store) = test_pg_store().await? else {
        return Ok(());
    };
    let user_id = create_test_user_with_subscription(&store).await?;
    store
        .record_subscription_pull_event_for_user(
            user_id,
            "203.0.113.8",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await?;
    insert_user_session_event(
        &store,
        user_id,
        "203.0.113.8",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .await?;

    let page = store.admin_user_devices_json(user_id, 1, 20).await?;

    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["client_ip"], "203.0.113.8");
    assert_eq!(page["items"][0]["source"], "both");
    assert_eq!(page["items"][0]["subscription_pull_count"], 1);
    assert_eq!(page["items"][0]["node_use_count"], 1);
    Ok(())
}
```

- [ ] **Step 2: Run the focused DB test and verify RED**

Run:

```bash
cargo test -p xrayc-db test_pg_admin_user_devices_merge_subscription_and_usage_ip_when_database_url_is_set -- --nocapture
```

Expected: fails because `admin_user_devices_json` and the subscription pull table/method do not exist.

- [ ] **Step 3: Add migration and DB methods**

Create `subscription_pull_events`:

```sql
CREATE TABLE IF NOT EXISTS subscription_pull_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_id UUID REFERENCES subscription_tokens(id) ON DELETE SET NULL,
    client_ip TEXT NOT NULL DEFAULT '',
    client_ip_hash TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_subscription_pull_events_user_time
    ON subscription_pull_events(user_id, observed_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_subscription_pull_events_user_ip
    ON subscription_pull_events(user_id, client_ip, client_ip_hash, observed_at DESC);
```

Add methods:

```rust
pub async fn record_subscription_pull_event_for_user(
    &self,
    user_id: Uuid,
    client_ip: &str,
    client_ip_hash: &str,
    user_agent: &str,
) -> Result<(), DbError>
```

```rust
pub async fn admin_user_devices_json(
    &self,
    user_id: Uuid,
    page: i64,
    page_size: i64,
) -> Result<Value, DbError>
```

`admin_user_devices_json` should union:
- `subscription_pull_events` as source `subscription_pull`
- `access_user_session_events` as source `node_use`

Dedup key:

```sql
CASE
  WHEN client_ip <> '' THEN 'ip:' || client_ip
  ELSE 'hash:' || client_ip_hash
END
```

Return fields:

```json
{
  "client_ip": "...",
  "client_ip_hash": "...",
  "source": "subscription_pull|node_use|both",
  "first_seen_at": "...",
  "last_seen_at": "...",
  "subscription_pull_count": 0,
  "node_use_count": 0,
  "last_access_line": {...},
  "last_access_node": {...},
  "last_session_status": "online|offline|unknown",
  "last_active_connection_count": 0
}
```

- [ ] **Step 4: Run focused DB test and verify GREEN**

Run the same `cargo test -p xrayc-db ...` command. Expected: passes.

### Task 2: API Recording And Admin Endpoint

**Files:**
- Modify: `crates/api/src/public.rs`
- Modify: `crates/api/src/admin_users.rs`
- Modify: `crates/api/src/lib.rs`
- Test: `crates/api/src/admin_user_endpoint_tests.rs` or existing subscription endpoint tests

- [ ] **Step 1: Write failing API tests**

Add an API test that calls `GET /api/admin/users/:id/devices` and expects the page wrapper. Add a subscription download test that verifies the pull event is written when `/sub/:token` succeeds.

Expected route:

```text
GET /api/admin/users/:user_id/devices?page=1&page_size=20
```

- [ ] **Step 2: Run focused API test and verify RED**

Run:

```bash
cargo test -p xrayc-api admin_user_devices -- --nocapture
```

Expected: fails with 404 or missing method.

- [ ] **Step 3: Implement route and request IP capture**

In `download_subscription`, read client IP from forwarded headers using existing safe helper style. On successful YAML generation, call:

```rust
pg.record_subscription_pull_event_for_token(&token, &client_ip, &client_ip_hash, user_agent).await
```

Add admin handler:

```rust
pub(crate) async fn admin_user_devices(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response
```

Wire route:

```rust
.route("/api/admin/users/:user_id/devices", get(admin_user_devices))
```

- [ ] **Step 4: Run focused API tests and verify GREEN**

Run the same API test command. Expected: passes.

### Task 3: Frontend Device Drawer

**Files:**
- Modify: `frontend/src/services/api/paths.ts`
- Modify: `frontend/src/services/api/types/billing.ts`
- Modify: `frontend/src/services/api/normalizers/billing.ts`
- Modify: `frontend/src/services/api/clients/billing.ts`
- Create: `frontend/src/views/users/UserDevicesDrawer.vue`
- Modify: `frontend/src/views/UsersPage.vue`
- Test: `frontend/e2e/smoke/admin-users-devices.spec.ts`

- [ ] **Step 1: Write failing Playwright smoke**

Mock `/api/admin/users/:id/devices` and assert clicking "设备管理" opens a drawer with one merged IP row showing source "订阅拉取 + 节点使用".

- [ ] **Step 2: Run Playwright smoke and verify RED**

Run:

```bash
cd frontend && E2E_BASE_URL=http://127.0.0.1:4174 npx playwright test e2e/smoke/admin-users-devices.spec.ts --project=chromium
```

Expected: fails because the button/drawer do not exist.

- [ ] **Step 3: Add API types and drawer**

Add type:

```ts
export type AdminUserDeviceIp = {
  clientIp: string;
  clientIpHash: string;
  source: 'subscription_pull' | 'node_use' | 'both' | 'unknown';
  firstSeenAt: string;
  lastSeenAt: string;
  subscriptionPullCount: number;
  nodeUseCount: number;
  lastAccessLine: { id: string; name: string; protocol: string; transport: string };
  lastAccessNode: { id: string; name: string; publicHost: string };
  lastSessionStatus: string;
  lastActiveConnectionCount: number;
};
```

Add `UserDevicesDrawer.vue` with pagination, refresh, and table columns:
- IP
- 来源
- 首次出现
- 最近出现
- 订阅拉取次数
- 节点使用次数
- 最近入口
- 最近状态/连接数

Add a `设备管理` text button in `UsersPage.vue` row operations.

- [ ] **Step 4: Run Playwright smoke and frontend build**

Run:

```bash
cd frontend && E2E_BASE_URL=http://127.0.0.1:4174 npx playwright test e2e/smoke/admin-users-devices.spec.ts --project=chromium
cd frontend && npm run build
```

Expected: smoke and build pass.

### Task 4: Docs And Guards

**Files:**
- Modify: `开发方案.md`
- Modify: `README.md`
- Modify: `文档/前端/页面清单.md`
- Modify: `文档/接口/管理接口.md`
- Modify: `文档/接口/订阅接口.md`
- Modify: `文档/架构/订阅生命周期.md`
- Modify: `文档/测试/真实测试用例.md`

- [ ] **Step 1: Update docs**

Document that "设备管理" is an IP observation view and does not restore the removed client-side credential model:
- records subscription pull IPs
- records real node-use IPs
- deduplicates repeated IPs
- does not change the single subscription URL or user credential model

- [ ] **Step 2: Run docs/security checks**

Run:

```bash
bash scripts/check-docs-no-stale.sh
bash scripts/verify-no-secrets.sh
git diff --check
```

Expected: all exit 0.

### Final Verification

Run after all tasks:

```bash
cargo fmt --all -- --check
cargo test -p xrayc-db admin_user_devices -- --nocapture
cargo test -p xrayc-api admin_user_devices -- --nocapture
cd frontend && npm run build
cd frontend && E2E_BASE_URL=${E2E_BASE_URL:-http://127.0.0.1:8080} npx playwright test e2e/smoke/admin-users-devices.spec.ts --project=chromium
bash scripts/check-docs-no-stale.sh
bash scripts/verify-no-secrets.sh
```

Do not claim completion until every command has been run fresh and inspected.
