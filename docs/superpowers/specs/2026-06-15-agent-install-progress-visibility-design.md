# Agent Install Progress Visibility Design

## Goal

Make Agent install progress understandable from the admin UI: administrators must see which server is being installed, which installation mode is used, which step is running, and the redacted reason when a task fails.

## Scope

This change affects deployment task read models, one-click/manual Agent install task progress, the admin UI that renders deployment tasks, and the one-click reinstall behavior. It does not change SSH credential storage or artifact authentication.

## Data Model

Deployment task JSON keeps the existing safe fields and adds no secret-bearing fields. The UI consumes:

- `safe_metadata`: install mode, access node name, public host/port, SSH host, install directory, compose project, SSH port/auth method, TLS summary, and force reinstall flag.
- `steps`: ordered install milestones with `key`, `title`, `detail`, and `status`.
- `current_step`, `progress_percent`, `error_summary`, and `result`: current state, high-level progress, failure message, and redacted install summary.

Step status must be derived server-side whenever a task report is recorded:

- steps before the current step become `done`
- the current step becomes `current` while running
- on success, every step through the current step becomes `done`
- on failure, the failed/current step becomes `failed`, earlier steps become `done`, later steps stay `pending`

Unknown failure step keys are appended as a final failed step with a readable title.

## UI

The deployment task card is compact by default and groups tasks by server address. Only unfinished, failed, timed-out, or cancelled tasks are shown; fully successful install tasks are hidden from the card. A server/IP appears once in the list even when it has multiple visible install attempts. The row shows address, node name, status, current progress, history count, updated time, and actions.

Clicking the address opens a detail dialog showing:

- install target: node name, public address, SSH host, public port, install mode, SSH port/auth method, install directory, compose project
- install history: every visible non-successful task for the same server address, ordered by latest update
- timeline: all install steps with translated status
- failure panel: failed step, redacted error summary, and suggested checks for common causes
- result summary: access node ID, whether auth code was received, and artifact/install stderr summary if present

One-click install tasks must enter `running` immediately after creation; the UI must not show them as waiting for manual execution. The one-click path is fully automatic after form submission: the platform SSHes to the server, uploads the install script and dependencies, and starts the remote install without requiring the admin to upload any files. Manual install guide tasks may show `等待服务器上报` because they depend on an administrator running the script on the server. Failed or stale running rows expose a retry action. Retry opens the one-click install dialog and pre-fills only non-secret values from `safe_metadata`, including SSH host when present; admins must re-enter SSH credentials because passwords/private keys are never stored.

The UI must never display SSH passwords, private keys, deployment artifact tokens, report tokens, raw install commands, or full logs.

## Reinstall Behavior

One-click Agent install always forces reinstall. The API ignores a client-provided `force_reinstall=false` and records `force_reinstall=true` in safe metadata. The deploy script stops existing Compose containers, removes stale runtime files, clears state/config/artifacts/env/compose files, removes LetsEncrypt material only for the current or previously configured SSL domains, and then downloads and starts the fresh runtime stack.

## Testing

Backend tests verify task reports update step statuses, expose safe metadata without secrets, and force one-click reinstall even when clients send false. Frontend normalizer tests are not present in this repo, so the front-end contract is covered through Playwright smoke tests for grouped deployment tasks, retry prefill, access node install UI, and TypeScript build.
