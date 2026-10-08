# User Shared TCP/UDP Rate Limit Design

## Goal

Document and validate the confirmed user-level limiter semantics: a user's existing `rate_limit_bps` caps the user's combined TCP and UDP traffic on each transit node. No UDP-specific field is added.

## Scope

- `plans.rate_limit_bps` remains the plan default.
- `users.rate_limit_bps` remains the highest-priority user override.
- `NULL` user rate inherits the plan rate.
- `0` means unlimited.
- A positive rate is one shared per-user cap for TCP and UDP together.
- The limiter does not parse application protocols. HY2, XUDP, QUIC, game UDP, DNS-style UDP, and any other UDP payload are treated as UDP packets after Xray has mapped them to a user-marked outbound.
- The cap is per user per transit node; multi-transit global aggregate limiting is out of scope.

## Architecture

The existing limiter already builds one HTB class per user and classifies traffic by Xray `sockopt.mark` restored through connmark. The class filters are `protocol ip` fwmark filters, not TCP-only filters, so marked UDP packets can share the same class as marked TCP packets.

Client-observed upload needs an ingress boundary. Xray only knows the authenticated user after it accepts the inbound connection, so packets entering a shared transit access port are not user-marked yet. If the limiter only shapes Xray's marked outbound and connmark-restored reverse traffic, download and relay-to-exit upload are capped, but the client can still push upload into Xray buffers too quickly.

The strict design keeps the administrator's logical access line unchanged, but renders positive-rate users as single-user runtime inbounds. Subscription rendering and agent heartbeat rendering share the same deterministic runtime port algorithm. A limited user's single public `/sub/{token}` subscription still contains one node for the logical line, but the port is that user's runtime port; this is runtime isolation inside the same single subscription model, not a second subscription path. The agent config renders `AccessConfig.access_lines[].id` as a runtime inbound identifier and `source_line_id` as the original logical `access_lines.id`; metrics, sessions, probes, and traffic reports use `source_line_id` so billing stays attached to the logical line. The limiter then installs the existing ingress u32 `skbedit mark` rule against each single-user runtime port and redirects it to the same IFB HTB class.

Documentation and validation plans should make this shared TCP/UDP behavior explicit:

- Public, design, and test documentation use TCP/UDP shared user limiting.
- Unit coverage should assert that the command plan remains protocol-agnostic and does not introduce TCP-only filters.
- Unit coverage should assert that two limited users on one logical line receive different runtime ports and that the agent reports `source_line_id` instead of runtime IDs.
- Unit coverage should assert fail-closed behavior: positive-rate configs are rejected before Xray reload when the limiter is disabled, dry-run is enabled, a limiter command fails, or the runtime port pool is exhausted.
- Unit coverage should assert marked exit probes report the source endpoint ID and access-log sessions match the exact Xray bracket email, not a substring.
- Real E2E coverage should send UDP traffic through the real client and confirm the transit limiter class sees marked UDP bytes for the same user rate plan.
- Real upload coverage should send a POST through a real subscription client into a real sink and confirm both the egress class and IFB ingress class record the uploaded bytes near the configured user rate.

## Validation

Local validation:

- `cargo test -p xrayc-access-agent runtime::tests::limiter -- --nocapture`
- `cargo test --workspace -- --nocapture`
- `npm run build`
- `bash scripts/test-v2-mainline-coverage.sh`
- `bash scripts/check-no-legacy.sh`

Real validation:

- Four-server V2 relay pool E2E keeps the existing TCP rate measurement.
- The same E2E run sends UDP through the real client path and checks that the relay's user limiter class records UDP bytes under the same user `rate_limit_bps` plan.
- 2026-06-04 real run used VLESS/XUDP user ingress subscription, a VLESS/XUDP exit endpoint, and a real Xray client UDP dokodemo-door inbound. With plan `100000` bps and user override `300000` bps, TCP measured `282592` bps; the UDP limiter class recorded `198440` bytes / `328` packets and the UDP target received `507600` bytes / `423` packets in the sample window.
- The same run added a 20-minute real data UAT phase: 14 iterations of subscription download, user/admin read operations, and real client proxied download traffic completed; final ledger growth was 55 rows and 6913128 billed bytes.
- A later 2026-06-04 regression run reproduced the fast.com upload issue and verified the single-user access-port ingress fix. With one VLESS/TCP access line and `rate_limit_bps=1000000`, a real target-side client uploaded 4194304 bytes to a real HTTP sink through the subscription path. Curl measured 96200 B/s over 43.599542 seconds; the sink received all 4194304 bytes; the transit egress class increased by 4389837 bytes and the IFB ingress class increased by 4479160 bytes.
- A subsequent strict-user implementation changed positive-rate users on the same logical line to per-user runtime inbounds. Local tests cover distinct runtime ports in subscription YAML, heartbeat rendering of two single-user runtime inbounds with the same `source_line_id`, and access-agent session/probe reporting back to the original logical line ID.
- Agent dry-run is command-plan evidence only: it may verify that two limited users generate independent class/mark rules while an unlimited user does not, but positive-rate runtime config application must reject dry-run and cannot count as real traffic evidence.
- Real multi-user UAT must create temporary users with positive `rate_limit_bps`, parse each downloaded subscription, verify exactly one runtime-port node per user, verify all runtime ports are unique and non-base, then check the target-node Xray active config and limiter plan before the 20-minute traffic phase.

## Non-Goals

- No separate `udp_rate_limit_bps`.
- No protocol-aware classification.
- No multi-transit global aggregate cap.
- No promise that UDP is lossless; UDP tests validate classification and effective limiter participation under real traffic.
- No modification to Xray source code.
- No protocol-aware user-space limiter.
- No promise that an already-imported old subscription keeps the same port after strict runtime isolation is enabled; clients must refresh the subscription to receive the per-user runtime port.
