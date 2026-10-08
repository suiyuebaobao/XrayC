# Traffic Log Retention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable detailed traffic log retention while preserving long-term operations statistics through daily and hourly rollups.

**Architecture:** Keep recent detail in `usage_ledgers`; roll old rows into `usage_daily_rollups` and `usage_hourly_rollups`; have read models combine retained detail with rollups.

**Tech Stack:** Rust, SQLx, PostgreSQL migrations/tests, Vue operations settings UI, API contract smoke scripts, public docs.

---

### Task 1: Database Settings and Migrations

**Files:**
- Modify: `crates/db/src/settings_json.rs`
- Modify: `crates/db/src/store/settings.rs`
- Modify: `crates/db/src/types.rs`
- Add: `migrations/202606090001_usage_daily_rollups.sql`
- Add: `migrations/202606090002_usage_daily_rollups_traffic_source.sql`
- Add: `migrations/202606090003_usage_hourly_rollups.sql`

- [ ] Add normalized `traffic_log_retention` setting with safe defaults.
- [ ] Add daily and hourly rollup tables keyed by source, entity, user, and window.
- [ ] Expose the parsed retention policy to store maintenance code.

### Task 2: Worker Rollup and Prune

**Files:**
- Modify: `crates/db/src/store/maintenance.rs`
- Modify: `crates/db/src/tests/part33.rs`

- [ ] Before deleting old detail rows, insert old access-line ledgers into daily/hourly rollups.
- [ ] Delete old detail rows in configured batches.
- [ ] Return `pruned_usage_ledgers` from maintenance for observability.
- [ ] Prove idempotency and source filtering with PostgreSQL integration tests.

### Task 3: Read Models and UI

**Files:**
- Modify: `crates/db/src/store/operations.rs`
- Modify: `crates/db/src/store/traffic_health.rs`
- Modify: `crates/db/src/store/user_traffic_logs.rs`
- Modify: `frontend/src/views/operations/OperationsStatusSettingsSection.vue`
- Modify: `frontend/src/views/AdminTrafficLogsPage.vue`
- Modify: `frontend/src/views/users/UserTrafficLogsDrawer.vue`

- [ ] Make user detailed logs show only retained access-line detail.
- [ ] Make operations ranking, traffic health, and access-line metrics combine detail and rollups.
- [ ] Add operations setting input for detailed log retention days.
- [ ] Clarify UI text that detailed logs are retained while historical aggregates remain.

### Task 4: Contracts, Docs, and Verification

**Files:**
- Modify: `scripts/api-contract-smoke-draft.sh`
- Modify: `scripts/lib/api-contract-smoke-draft/core.sh`
- Modify: `README.md`
- Modify: `开发方案.md`
- Modify: `文档/接口/管理接口.md`
- Modify: `文档/接口/接口草案.md`
- Modify: `文档/接口/开放接口.yaml`
- Modify: `文档/架构/流量生命周期.md`
- Modify: `文档/运维手册/流量修复.md`
- Modify: `文档/测试/真实测试用例.md`

- [ ] Update API contract smoke to cover `traffic_log_retention` GET/PUT.
- [ ] Update public docs and plan text to describe detail retention and rollups.
- [ ] Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://xrayc:change-me@127.0.0.1:15432/xrayc cargo test -p xrayc-db -- --test-threads=1
cargo test --workspace --exclude xrayc-db
npm --prefix frontend run build
npm --prefix frontend run test:e2e -- --list
git diff --check
bash scripts/verify-no-secrets.sh
bash scripts/check-docs-no-stale.sh
bash scripts/check-no-legacy.sh
```
