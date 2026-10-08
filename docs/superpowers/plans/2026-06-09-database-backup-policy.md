# 数据库自动备份策略执行计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-06-09-database-backup-policy-design.md`

## Tasks

- [ ] Add normalized `database_backup` settings under `access_operations`.
  - Default to enabled, 1 day interval, 30 day retention.
  - Clamp interval to 1-30 days and retention to 1-3650 days.
  - Add focused unit tests for default and clamp behavior.

- [ ] Add Worker backup runner.
  - Read backup policy and state from PostgreSQL.
  - Use PostgreSQL advisory lock to prevent concurrent backups.
  - Run `pg_dump --format=custom --no-owner --no-acl`.
  - Avoid passing `DATABASE_URL` through command argv.
  - Add dump timeout and failed-attempt backoff.
  - Write success/failure state and prune old XrayC backup files.

- [ ] Add management platform settings UI and API contract coverage.
  - Frontend reads and saves `database_backup`.
  - API smoke validates GET and idempotent PUT shape.
  - OpenAPI schema documents fields and ranges.

- [ ] Update deployment and operations documentation.
  - Document `DATABASE_BACKUP_DIR`.
  - Document default daily backup and retention.
  - Document restore command and non-PostgreSQL backup boundary.

- [ ] Verify.
  - Run Rust formatting and tests covering db and worker changes.
  - Run frontend build.
  - Run script syntax and documentation stale checks.
