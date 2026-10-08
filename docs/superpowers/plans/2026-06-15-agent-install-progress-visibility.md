# Agent Install Progress Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Agent installation progress readable by showing target server, install mode, detailed steps, and redacted failure reasons.

**Architecture:** Reuse `deployment_tasks` as the source of truth. Add server-side step status derivation in `PgStore::record_deployment_task_report`, enrich safe metadata normalization, group deployment tasks by server address in `DeploymentTasksCard.vue`, and move detailed history into a dialog. One-click install must force reinstall and retry must reopen the one-click dialog with only non-secret fields prefilled.

**Tech Stack:** Rust/Axum/PostgreSQL for deployment task API, Vue 3 + Element Plus for admin UI, cargo tests and frontend production build for verification.

---

### Task 1: Backend Step Status Model

**Files:**
- Modify: `crates/db/src/store/deployment_tasks.rs`
- Modify: `crates/api/src/deploy_tests.rs`

- [ ] **Step 1: Write failing backend test**

Add assertions to `test_pg_one_click_install_creates_task_and_rebinds_installed_node_when_database_url_is_set` that fetched deployment task JSON contains:

```rust
let task = tasks.1["data"]["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["safe_metadata"]["access_node_name"] == "one-click-node")
    .expect("one-click task should be listed");
assert_eq!(task["safe_metadata"]["mode"], "one_click");
assert_eq!(task["safe_metadata"]["public_host"], "one-click.example.test");
let steps = task["steps"].as_array().unwrap();
assert!(steps.iter().any(|step| step["key"] == "ssh_connect" && step["status"] == "done"));
assert!(steps.iter().any(|step| step["key"] == "node_registered" && step["status"] == "done"));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xrayc-api test_pg_one_click_install_creates_task_and_rebinds_installed_node_when_database_url_is_set -- --nocapture`

Expected: FAIL because `steps` still keep their initial static statuses.

- [ ] **Step 3: Implement step status derivation**

Add helper functions in `deployment_tasks.rs`:

```rust
fn update_deployment_steps_statuses(steps: Value, current_key: &str, task_status: &str, message: &str) -> Value
fn step_status_for_position(index: usize, current_index: usize, task_status: &str) -> &'static str
fn readable_deployment_step_title(key: &str) -> String
```

Use the helper inside `record_deployment_task_report` so the SQL update writes both `current_step` and the updated `steps`.

- [ ] **Step 4: Run backend test to verify pass**

Run: `cargo test -p xrayc-api test_pg_one_click_install_creates_task_and_rebinds_installed_node_when_database_url_is_set -- --nocapture`

Expected: PASS.

### Task 2: Frontend Deployment Task Details

**Files:**
- Modify: `frontend/src/services/api/types/routing.ts`
- Modify: `frontend/src/services/api/normalizers/deployment.ts`
- Modify: `frontend/src/views/access-lines/DeploymentTasksCard.vue`

- [ ] **Step 1: Add safe metadata type fields**

Extend `DeploymentTask` with `safeMetadata: Record<string, unknown>`.

- [ ] **Step 2: Normalize safe metadata**

In `normalizeDeploymentTask`, map `safe_metadata` / `safeMetadata` to `safeMetadata`.

- [ ] **Step 3: Render compact grouped details**

Update `DeploymentTasksCard.vue` so one server address appears as one row. Clicking the address opens a dialog showing target, install history, step timeline, failure reason, and result summary. Use only `safeMetadata`, `steps`, `errorSummary`, and sanitized `result`.

- [ ] **Step 4: Add retry prefill**

Add a retry event from `DeploymentTasksCard.vue` and handle it in `useAccessLinesPage.ts` by opening the one-click install dialog with node name, public host/port, SSH port, install directory and compose project prefilled. Do not prefill SSH password or private key.

- [ ] **Step 5: Verify frontend build**

Run: `cd frontend && npm run build`

Expected: PASS.

### Task 3: Full Verification And Publish

**Files:**
- No new implementation files.

- [ ] **Step 1: Run targeted checks**

Run:

```bash
cargo test -p xrayc-api test_pg_one_click_install_creates_task_and_rebinds_installed_node_when_database_url_is_set -- --nocapture
cd frontend && npm run build
```

- [ ] **Step 2: Run full check**

Run: `make check`

- [ ] **Step 3: Rebuild containers**

Run:

```bash
make rebuild
bash scripts/check-running-images-current.sh
```

- [ ] **Step 4: Commit and push**

Run:

```bash
git status -sb
git add crates/db/src/store/deployment_tasks.rs crates/api/src/deploy_tests.rs frontend/src/services/api/types/routing.ts frontend/src/services/api/normalizers/deployment.ts frontend/src/views/access-lines/DeploymentTasksCard.vue docs/superpowers/specs/2026-06-15-agent-install-progress-visibility-design.md docs/superpowers/plans/2026-06-15-agent-install-progress-visibility.md
git commit -m "Improve agent install progress visibility"
git push origin main
```
