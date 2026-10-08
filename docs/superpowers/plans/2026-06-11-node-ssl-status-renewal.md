# Node SSL Status Renewal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-node SSL certificate status reporting and a single-node renewal action.

**Architecture:** The access-agent reports sanitized certificate summaries in heartbeat requests. The control plane stores the latest summaries on `access_nodes`, exposes them through the existing access-routing read model, and queues one renewal task per node. The agent receives that task in heartbeat responses, runs certbot renewal locally, reloads Xray, and reports the result in the next heartbeat.

**Tech Stack:** Rust Axum API, SQLx PostgreSQL, Rust access-agent, Bash deploy scripts, Vue 3 + TypeScript + Element Plus.

---

### Task 1: Database and API Contract

**Files:**
- Create: `migrations/202606110002_access_node_tls_status.sql`
- Modify: `crates/core/src/model.rs`
- Modify: `crates/db/src/store/rows/topology.rs`
- Modify: `crates/db/src/store/load.rs`
- Modify: `crates/db/src/read_models.rs`
- Modify: `crates/db/src/store/agent.rs`
- Modify: `crates/api/src/agent.rs`
- Modify: `crates/api/src/admin_nodes.rs`
- Modify: `crates/api/src/lib.rs`
- Test: `crates/db/src/tests/part38.rs`
- Test: `crates/api/src/admin_nodes_tests.rs`

- [ ] Write failing tests for storing heartbeat TLS reports and queueing a single-node renewal task.
- [ ] Run the focused Rust tests and confirm they fail because the fields and endpoint do not exist.
- [ ] Add migration columns to `access_nodes`.
- [ ] Extend core and DB row models.
- [ ] Add `record_agent_tls_status`, `request_access_node_tls_renewal`, and heartbeat task attachment.
- [ ] Add admin endpoint `POST /api/admin/access-nodes/:id/tls/renew`.
- [ ] Run focused Rust tests and confirm they pass.

### Task 2: Access-Agent Certificate Collection and Renewal

**Files:**
- Modify: `crates/access-agent/src/config.rs`
- Modify: `crates/access-agent/src/client.rs`
- Modify: `crates/access-agent/src/runtime.rs`
- Create: `crates/access-agent/src/runtime/tls.rs`
- Test: `crates/access-agent/src/client_tests.rs`
- Test: `crates/access-agent/src/runtime/tests/tls.rs`

- [ ] Write failing tests for heartbeat TLS JSON fields, certificate date parsing, domain collection, and renewal task execution command shape.
- [ ] Run focused access-agent tests and confirm they fail.
- [ ] Add `XRAYC_TLS_CERT_DOMAINS` parsing to settings.
- [ ] Add TLS DTOs to client heartbeat request/response.
- [ ] Add runtime TLS collector using `openssl x509`.
- [ ] Add renewal executor using `certbot renew --cert-name <domain> --non-interactive --no-random-sleep-on-renew` and existing reload command.
- [ ] Wire heartbeat loop to report TLS status, consume one renewal task, and report result on the next heartbeat.
- [ ] Run focused access-agent tests and confirm they pass.

### Task 3: Deploy Script Certificate Rules

**Files:**
- Modify: `scripts/lib/deploy-access-agent/tooling.sh`
- Modify: `scripts/lib/deploy-access-agent/common.sh`
- Modify: `scripts/deploy-access-agent.sh`
- Test: `scripts/check-v2-deploy-contract.sh`

- [ ] Add failing static/script check for “domain and email are both required for automatic certbot request”.
- [ ] Run the script check and confirm it fails.
- [ ] Add interactive prompts for domain and email when a TTY is present.
- [ ] Skip automatic certificate request when either final domain or email is empty.
- [ ] Remove unsafe no-email ACME registration mode.
- [ ] Persist `XRAYC_TLS_CERT_DOMAINS` into agent env so agent can report certificates.
- [ ] Run the script check and confirm it passes.

### Task 4: Frontend Display and Button

**Files:**
- Modify: `frontend/src/services/api/types/routing.ts`
- Modify: `frontend/src/services/api/normalizers/routing.ts`
- Modify: `frontend/src/services/api/clients/routing.ts`
- Modify: `frontend/src/views/access-lines/RelayNodeCard.vue`
- Modify: `frontend/src/views/access-lines/useAccessLinesPage.ts`
- Test: `frontend/e2e/smoke/admin-routing.spec.ts`

- [ ] Add failing front-end smoke or type-level coverage for SSL status fields and renew action.
- [ ] Run the focused frontend test/build and confirm failure.
- [ ] Extend routing types and normalizer for TLS certificate and renewal fields.
- [ ] Add API client method for single-node renewal.
- [ ] Show SSL status in each relay node card and add per-node renewal button/loading.
- [ ] Run frontend build and focused smoke test.

### Task 5: Documentation and Verification

**Files:**
- Modify: `开发方案.md`
- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `文档/部署/部署说明.md`
- Modify: `文档/部署/生产部署.md`
- Modify: `文档/架构/当前方案与运营闭环.md`
- Modify: `文档/每日记录/2026-06-11-日报.md`

- [ ] Update docs with SSL status reporting, single-node renewal, and email/domain certificate rule.
- [ ] Run `make check-plan`.
- [ ] Run affected Rust tests.
- [ ] Run `cd frontend && npm run build`.
- [ ] Run `bash scripts/check-v2-deploy-contract.sh`.
- [ ] Run any additional checks reported by `make check-plan`.
