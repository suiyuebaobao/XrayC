# Rule Library Group Binding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace direct line-group dedicated rules with reusable subscription rule sets that can be bound to one or more line groups and synchronize changes everywhere they are used.

**Architecture:** Add first-class rule-set persistence and a line-group binding table. Subscription rendering consumes bound rule sets per generated proxy group and rewrites proxy actions to the current group. The old `line_groups.dedicated_rules` field is migrated into rule sets and retained only as compatibility data during the transition.

**Tech Stack:** Rust, sqlx, PostgreSQL migrations, Axum API handlers, Vue 3, Element Plus, Playwright, existing cargo integration tests.

---

### Task 1: Database Rule-Set Persistence

**Files:**
- Create: `migrations/202606110003_subscription_rule_sets.sql`
- Modify: `crates/db/src/types.rs`
- Modify: `crates/db/src/store/mod.rs`
- Create: `crates/db/src/store/subscription_rule_sets.rs`
- Modify: `crates/db/src/lib.rs`
- Modify: `crates/db/src/tests/mod.rs`
- Create or extend: `crates/db/src/tests/part41.rs`

- [ ] **Step 1: Write failing DB tests for rule-set CRUD and binding validation**

Add tests that call the wished-for PgStore methods:

```rust
#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn test_subscription_rule_set_crud_and_delete_conflict(pool: PgPool) {
    let store = PgStore::new(pool);
    let rule_set_id = store
        .create_subscription_rule_set(AdminSubscriptionRuleSetInput {
            name: "AI 规则".to_string(),
            description: "OpenAI and Claude".to_string(),
            enabled: Some(true),
            rules: vec![
                "DOMAIN-SUFFIX,openai.com,PROXY".to_string(),
                "DOMAIN-SUFFIX,baidu.com,DIRECT".to_string(),
            ],
        })
        .await
        .expect("rule set should be created");

    let rule_sets = store
        .list_subscription_rule_sets()
        .await
        .expect("rule sets should load");
    assert!(rule_sets.iter().any(|rule_set| rule_set.id == rule_set_id));

    store
        .update_subscription_rule_set(
            rule_set_id,
            AdminSubscriptionRuleSetInput {
                name: "AI 新规则".to_string(),
                description: "updated".to_string(),
                enabled: Some(false),
                rules: vec!["DOMAIN-SUFFIX,anthropic.com,PROXY".to_string()],
            },
        )
        .await
        .expect("rule set should update");

    let updated = store
        .list_subscription_rule_sets()
        .await
        .expect("rule sets should reload")
        .into_iter()
        .find(|rule_set| rule_set.id == rule_set_id)
        .expect("updated rule set should exist");
    assert_eq!(updated.name, "AI 新规则");
    assert!(!updated.enabled);
    assert_eq!(updated.rules, vec!["DOMAIN-SUFFIX,anthropic.com,PROXY"]);
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```bash
cargo test -p xrayc-db subscription_rule_set -- --nocapture
```

Expected: FAIL because PgStore rule-set methods and types do not exist.

- [ ] **Step 3: Add migration and data types**

Create `subscription_rule_sets` and `line_group_rule_set_bindings` with UUID primary keys, JSONB array checks, unique binding per group/rule-set, and ordered `position`. Migration must also convert non-empty `line_groups.dedicated_rules` into `分组名-历史规则` rule sets and bind them back to the original group.

Add types:

```rust
pub struct AdminSubscriptionRuleSetInput {
    pub name: String,
    pub description: String,
    pub enabled: Option<bool>,
    pub rules: Vec<String>,
}

pub struct SubscriptionRuleSetSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub rules: Vec<String>,
    pub binding_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct LineGroupRuleSetBindingInput {
    pub rule_set_id: Uuid,
    pub position: i32,
    pub enabled: bool,
}
```

- [ ] **Step 4: Implement PgStore CRUD**

Implement:

```rust
create_subscription_rule_set(input) -> Result<Uuid, DbError>
list_subscription_rule_sets() -> Result<Vec<SubscriptionRuleSetSummary>, DbError>
update_subscription_rule_set(id, input) -> Result<(), DbError>
delete_subscription_rule_set(id) -> Result<u64, DbError>
```

Normalize names with existing admin text helpers, trim empty rules, reject empty names, return conflict-style invalid input when deleting a bound rule set.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test -p xrayc-db subscription_rule_set -- --nocapture
cargo fmt
```

Expected: new rule-set persistence tests pass and formatting is clean.

### Task 2: Line-Group Binding and Subscription Rendering

**Files:**
- Modify: `crates/core/src/model.rs`
- Modify: `crates/core/src/store.rs`
- Modify: `crates/core/src/subscription.rs`
- Modify: `crates/core/src/subscription/profile.rs`
- Modify: `crates/core/src/subscription/option_tests.rs`
- Modify: `crates/db/src/store/load.rs`
- Modify: `crates/db/src/read_models.rs`
- Modify: `crates/db/src/subscription_read_models.rs`
- Modify: `crates/db/src/store/routing_groups.rs`
- Extend: `crates/db/src/tests/part41.rs`

- [ ] **Step 1: Write failing core test for reusable rule sets bound to two groups**

Add a test that creates two line groups using the same rule set and asserts rendered YAML contains two group-targeted rule lines:

```rust
assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,AI分组"));
assert!(yaml.contains("DOMAIN-SUFFIX,openai.com,游戏分组"));
assert!(!yaml.contains("DOMAIN-SUFFIX,openai.com,PROXY"));
```

- [ ] **Step 2: Run test to verify RED**

Run:

```bash
cargo test -p xrayc-core subscription_merges_bound_rule_sets -- --nocapture
```

Expected: FAIL because `LineGroup` has no `rule_set_bindings` and renderer only consumes `dedicated_rules`.

- [ ] **Step 3: Add core model fields and renderer merge**

Add:

```rust
pub struct LineGroupRuleSetBinding {
    pub rule_set_id: Uuid,
    pub rule_set_name: String,
    pub enabled: bool,
    pub position: i32,
    pub rules: Vec<String>,
}
```

`LineGroup` gets `rule_set_bindings: Vec<LineGroupRuleSetBinding>`. During `VisibleLine::new`, carry section group rules from active bindings in binding order. Fall back to legacy `dedicated_rules` only when bindings are empty.

- [ ] **Step 4: Add DB load/read-model support**

Load rule-set bindings after line groups, sorted by `position, rule_set.name, rule_set.id`. Include `rule_set_bindings` in admin control-plane JSON and subscription read models. Keep `dedicated_rules` in JSON as compatibility, but do not treat it as the preferred write path.

- [ ] **Step 5: Add line-group update binding validation**

Extend `AdminLineGroupInput` with `rule_set_bindings: Option<Vec<LineGroupRuleSetBindingInput>>`. In create/update group, replace bindings inside the existing transaction when the field is present. Reject duplicate rule-set IDs and unknown rule-set IDs.

- [ ] **Step 6: Verify GREEN**

Run:

```bash
cargo test -p xrayc-core subscription_ -- --nocapture
cargo test -p xrayc-db subscription_rule_set -- --nocapture
cargo test -p xrayc-db subscription -- --nocapture
```

Expected: reusable rule-set tests pass, old dedicated-rule tests still pass through fallback, and subscription read models include all authorized groups.

### Task 3: Admin API

**Files:**
- Modify: `crates/api/src/admin_lines.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/admin_nodes_tests.rs` or create focused API test file if the crate already has admin route helpers.
- Modify: `crates/api/src/deploy_tests.rs` only if request structs require compile updates.

- [ ] **Step 1: Write failing API test**

Add an admin API test that creates a rule set, lists it, updates it, binds it to a line group through the line group update API, and verifies deleting it while bound returns a conflict/unprocessable response.

- [ ] **Step 2: Run API test to verify RED**

Run:

```bash
cargo test -p xrayc-api rule_set -- --nocapture
```

Expected: FAIL because `/api/admin/subscription-rule-sets` routes do not exist.

- [ ] **Step 3: Add request/response handlers**

Add request DTOs:

```rust
struct RuleSetRequest {
    name: String,
    #[serde(default)]
    description: String,
    enabled: Option<bool>,
    #[serde(default)]
    rules: Vec<String>,
}

struct LineGroupRuleSetBindingRequest {
    rule_set_id: Uuid,
    position: Option<i32>,
    enabled: Option<bool>,
}
```

Expose:

```text
GET    /api/admin/subscription-rule-sets
POST   /api/admin/subscription-rule-sets
PUT    /api/admin/subscription-rule-sets/:id
DELETE /api/admin/subscription-rule-sets/:id
```

Extend create/update line-group requests with `rule_set_bindings`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p xrayc-api rule_set -- --nocapture
cargo test -p xrayc-api admin_lines -- --nocapture
```

Expected: API rule-set tests pass and existing line-group tests compile.

### Task 4: Frontend Rule Library UI and Group Binding

**Files:**
- Modify: `frontend/src/services/api/paths.ts`
- Modify: `frontend/src/services/api/types/routing.ts`
- Modify: `frontend/src/services/api/normalizers/routing.ts`
- Modify: `frontend/src/services/api/clients/routing.ts`
- Modify: `frontend/src/views/RuleSettingsPage.vue`
- Modify: line group editing code in the visible exit management page (`frontend/src/views/LinePoolPage.vue` is a legacy filename) or related access-lines page components where group create/update lives.
- Modify: `frontend/e2e/smoke/admin-rule-settings.spec.ts`
- Add or extend: focused frontend smoke for line-group rule-set binding.

- [ ] **Step 1: Write failing frontend smoke assertions**

Update E2E expectations so `/admin/rule-settings` creates a named rule library entry and no longer saves `dedicated_rules` directly to a line group. Add a line-group form assertion for selecting rule libraries and ordering bindings.

- [ ] **Step 2: Run E2E to verify RED**

Run:

```bash
cd frontend
npx playwright test e2e/smoke/admin-rule-settings.spec.ts --project=chromium
```

Expected: FAIL because the UI still shows direct group dedicated-rule editing and no rule-set API exists.

- [ ] **Step 3: Add frontend API support**

Add types:

```ts
export type SubscriptionRuleSet = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  rules: string[];
  bindingCount: number;
  createdAt: string;
  updatedAt: string;
};

export type LineGroupRuleSetBinding = {
  ruleSetId: string;
  ruleSetName: string;
  position: number;
  enabled: boolean;
  rules: string[];
};
```

Add `listSubscriptionRuleSets`, `createSubscriptionRuleSet`, `updateSubscriptionRuleSet`, and `deleteSubscriptionRuleSet` to the routing API client.

- [ ] **Step 4: Rebuild RuleSettingsPage as rule library + common rules**

Keep common rules editing. Replace the old "select line group and edit dedicated rules" section with a rule-set list/dialog. Rule-set edit dialog reuses `RuleEditor`. Remove old text saying rules are saved to `line_groups.dedicated_rules`.

- [ ] **Step 5: Add group binding UI**

In the line-group create/update dialog, add a "绑定规则" multi-select/table backed by rule sets. Save `rule_set_bindings` with sorted `position` and per-binding `enabled`. Do not send `dedicated_rules` from the UI.

- [ ] **Step 6: Verify GREEN**

Run:

```bash
npm --prefix frontend run build
E2E_BASE_URL=http://127.0.0.1:4174 npx playwright test frontend/e2e/smoke/admin-rule-settings.spec.ts --project=chromium
```

Expected: build passes, E2E can create/edit rule sets and bind them to groups.

### Task 5: Documentation, Deployment, and Full Verification

**Files:**
- Modify: `开发方案.md`
- Modify: `文档/架构/分组模型.md`
- Modify: `文档/接口/订阅接口.md`
- Modify: `文档/测试/真实测试用例.md`
- Modify: `docs/superpowers/plans/2026-06-10-group-dedicated-subscription-rules.md`

- [ ] **Step 1: Update docs and mark old plan superseded**

Document "规则库 -> 分组绑定 -> 套餐授权分组 -> 订阅生成" as the current model. Remove or mark old direct `dedicated_rules` guidance as legacy migration compatibility.

- [ ] **Step 2: Run full automated verification**

Run:

```bash
cargo fmt --check
cargo test -p xrayc-core -- --nocapture
cargo test -p xrayc-db subscription -- --nocapture
cargo test -p xrayc-api -- --nocapture
npm --prefix frontend run build
make check-postgres-integration
```

- [ ] **Step 3: Rebuild and restart current stack**

Run:

```bash
docker compose build api worker
docker compose up -d api worker
docker compose ps
```

- [ ] **Step 4: Subscription smoke with real data**

Create or reuse two rule sets, bind them to two authorized groups, pull the current valid subscription through local Caddy and public URL, and assert:

```text
proxy-groups >= 2
rules include group-specific targets
rules do not contain raw PROXY for dedicated rule-set entries
MATCH points to the default authorized group
```

- [ ] **Step 5: Final review**

Run `git diff --check`, inspect changed docs for stale "分组直接写专用规则" wording, and report any tests that could not be run.
