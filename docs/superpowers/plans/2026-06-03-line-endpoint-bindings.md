# Superseded Line Endpoint Binding Plan

This historical implementation plan is superseded by
`docs/superpowers/plans/2026-06-12-entry-management-binding-nodes-cloudflare.md`
and the current product design in `开发方案.md`.

Current rule:

- Exit management stores real exit endpoints.
- Entry management creates transit-node user entries.
- Entry management binds entries to exit endpoints and creates binding nodes.
- Line groups select binding nodes.
- Plans authorize line groups and set group multipliers.
- Legacy endpoint membership tables are compatibility data only and are not the
  current product path.

Do not use the older endpoint-grouping flow from this date as implementation
guidance.
