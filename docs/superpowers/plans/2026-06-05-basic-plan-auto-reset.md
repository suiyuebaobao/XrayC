# Basic Plan Auto Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Free default-plan subscriptions automatically reset each cycle, and expired paid subscriptions fall back to the default plan.

**Architecture:** Keep the existing `user_subscriptions` balance model. Use `plans.is_default` to identify the free basic plan inside Worker maintenance; reset expired default subscriptions and move expired non-default subscriptions to that plan in one maintenance transaction.

**Current Product Boundaries:** The basic plan resets on its configured plan cycle; the default seed is a 30-day monthly-style cycle. Paid purchases and redeem-code renewals keep the single-subscription balance model: extend from `max(existing_expires_at, now)`, add the paid traffic to `limit_bytes`, and preserve `used_bytes`.

**Tech Stack:** Rust, SQLx, PostgreSQL integration tests, existing `PgStore::run_worker_maintenance`.

---

### Task 1: Add Maintenance Regression Tests

**Files:**
- Create: `crates/db/src/tests/part33.rs`
- Modify: `crates/db/src/tests/mod.rs`

- [ ] **Step 1: Write failing PostgreSQL tests**

Add tests proving:

```rust
// Expired default-plan subscription is renewed:
// plan_id remains default, active=true, used_bytes=0,
// limit_bytes=default plan traffic, expires_at is in the future,
// assignments are cleared, nodes are dirty.

// Expired paid-plan subscription falls back:
// plan_id becomes default, active=true, used_bytes=0,
// limit_bytes=default plan traffic, expires_at is in the future,
// assignments are cleared, nodes are dirty.
```

- [ ] **Step 2: Verify RED**

Run with an isolated PostgreSQL database:

```bash
XRAYC_FIELD_ENCRYPTION_KEYS="test:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" \
DATABASE_URL="$db_url" \
cargo test -p xrayc-db expired_subscription -- --nocapture
```

Expected before implementation: tests fail because maintenance deactivates expired subscriptions.

### Task 2: Implement Maintenance Reset

**Files:**
- Modify: `crates/db/src/store/maintenance.rs`

- [ ] **Step 1: Load default plan in the maintenance transaction**

Select enabled, non-deleted default plan with `id`, `traffic_limit_bytes`, and `duration_days`.

- [ ] **Step 2: Replace the old deactivation CTE**

For all active expired subscriptions, update:

```sql
plan_id = default_plan.id,
active = TRUE,
expires_at = now() + default_duration_days,
used_bytes = 0,
limit_bytes = default_plan.traffic_limit_bytes,
updated_at = now()
```

Return affected user IDs for assignment cleanup.

- [ ] **Step 3: Preserve sync behavior**

Delete `user_access_line_assignments` and `user_exit_assignments` for affected users and keep `mark_all_nodes_dirty_in_tx(..., "worker_expired_subscription")`.

### Task 3: Verify and Publish

**Files:**
- Modify as needed from Tasks 1-2.

- [ ] **Step 1: Run target tests**

```bash
cargo test -p xrayc-db expired_subscription -- --nocapture
```

- [ ] **Step 2: Run broader checks**

```bash
cargo fmt --check
cargo test -p xrayc-db -- --nocapture
cargo test --workspace --lib
npm --prefix frontend run build
git diff --check
bash scripts/check-source-file-length.sh
bash scripts/verify-no-secrets.sh
```

- [ ] **Step 3: Verify paid renewal semantics remain unchanged**

Confirm existing order/redeem paths still use:

```text
expires_at = max(existing_expires_at, now) + paid_duration_days
limit_bytes = old_limit_bytes + paid_traffic_limit_bytes
used_bytes unchanged
```

- [ ] **Step 4: Commit and push**

```bash
git add docs/superpowers/specs/2026-06-05-basic-plan-auto-reset-design.md \
  docs/superpowers/plans/2026-06-05-basic-plan-auto-reset.md \
  crates/db/src/store/maintenance.rs \
  crates/db/src/tests/mod.rs \
  crates/db/src/tests/part33.rs
git commit -m "feat: auto-reset basic subscriptions"
git push
```
