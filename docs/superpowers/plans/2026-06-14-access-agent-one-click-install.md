# Access Agent One-Click Install Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a safe optional one-click access-agent installer that temporarily SSHes to a server, installs agent with optional SSL, and auto-registers the transit node while keeping the manual install guide.

**Architecture:** Reuse existing deployment task storage and `deploy-access-agent.sh`. Add a new admin API that validates SSH input, creates a deployment task, executes an isolated remote install, captures the node auth code, and creates the access node without persisting SSH credentials. Keep `/install-guide` as the manual fallback path.

**Tech Stack:** Rust/Axum/sqlx/Tokio process, Vue 3/Element Plus, existing Docker image tools `ssh`, `sshpass`, `scp`, existing deployment script.

---

### Task 1: Backend DTO and API Contract

**Files:**
- Modify: `crates/api/src/dto.rs`
- Modify: `crates/api/src/deploy_tests.rs`
- Modify: `crates/api/src/lib.rs`

- [ ] Write a failing PostgreSQL API test that posts `/api/admin/access-nodes/one-click-install` with SSH password and SSL fields, expects HTTP 202 or 200, and asserts the deployment task JSON does not contain the SSH password.
- [ ] Run the focused test and confirm it fails because the route does not exist.
- [ ] Add `OneClickAccessAgentInstallRequest` with node fields, SSH fields, deploy fields, and SSL fields.
- [ ] Register `POST /api/admin/access-nodes/one-click-install`.

### Task 2: Remote Installer

**Files:**
- Create: `crates/api/src/remote_install.rs`
- Modify: `crates/api/src/deploy.rs`

- [ ] Add a small `RemoteInstaller` abstraction that can run a remote install command.
- [ ] In tests, use a fake installer implementation that returns a sample node auth code.
- [ ] In production, the backend uses `sshpass` or `ssh` plus `scp` to upload scripts and execute them automatically; one-click install never requires the admin to upload scripts manually.
- [ ] Ensure temporary files are mode `0600` and are removed after execution.

### Task 3: Node Auto Registration

**Files:**
- Modify: `crates/api/src/deploy.rs`
- Modify: `crates/db/src/store/deployment_tasks.rs`

- [ ] Extract the existing access-node create path or call the store method used by `POST /api/admin/access-nodes`.
- [ ] On install success, parse the auth code from script output and create the node.
- [ ] Update deployment task result with node ID, node name, public host, SSL requested flag, and success status.
- [ ] Do not store SSH credential fields in task metadata or result.

### Task 4: Frontend UI

**Files:**
- Modify: `frontend/src/services/api/types/routing.ts`
- Modify: `frontend/src/services/api/normalizers/routing.ts`
- Modify: `frontend/src/services/api/clients/routing.ts`
- Create: `frontend/src/views/access-lines/OneClickInstallDialog.vue`
- Modify: `frontend/src/views/access-lines/useAccessLinesPage.ts`
- Modify: `frontend/src/views/AccessLinesPage.vue`

- [ ] Add API client method and types.
- [ ] Add a dialog with node fields, SSH auth mode, install dir, compose project, SSL domain and ACME email.
- [ ] Add button “一键安装 Agent”.
- [ ] On success, close dialog, reload nodes and deployment tasks, and show task status.

### Task 5: Docs and Verification

**Files:**
- Modify: `README.md`
- Modify: `开发方案.md`
- Modify: `文档/架构/当前方案与运营闭环.md`

- [ ] Update docs to state one-click install is optional, manual install remains available, SSH credentials are request-only, successful installs output a node auth code for platform auto-creation, and SSL is automatic when domain and email are filled.
- [ ] Run `cargo fmt --check`.
- [ ] Run focused backend tests.
- [ ] Run `cd frontend && npm run build`.
- [ ] Rebuild containers and check running image freshness.
