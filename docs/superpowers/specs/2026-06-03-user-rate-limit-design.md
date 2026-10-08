# User Rate Limit Design

## Goal

Add per-user smooth bandwidth limiting on each transit node. The limit is enforced on the transit node that accepts the client connection, uses real Linux traffic control, does not modify Xray source code, and keeps the single public subscription entry `/sub/{token}`. It must not introduce client-device subscriptions, dedicated subscriptions, or a second subscription method.

## Confirmed Rules

- Limit scope is per transit node and per user. If the same user connects to multiple transit nodes at the same time, each node enforces its own limit.
- User-specific rate has the highest priority. If a user override is set, it wins over the plan rate even when it is higher than the plan rate.
- A missing user override inherits the plan rate. A rate of `0` means unlimited.
- Current implementation targets TCP+UDP shared smooth limiting with `tc` HTB/token bucket behavior. UDP traffic uses the same per-user class and the same `rate_limit_bps`; no UDP-specific field is added.
- Xray source code is not modified. The control plane only changes generated Xray config and agent-side system rules.
- The limiter is logically isolated but deployed inside `access-agent` first. It can be split into a separate Docker container later.

## Architecture

The control plane stores two rate settings:

- `plans.rate_limit_bps`: default speed for users on a plan.
- `users.rate_limit_bps`: nullable user override. `NULL` inherits the plan, `0` overrides to unlimited.

For every active user on a transit node, the heartbeat read model computes an effective rate. Users with a positive effective rate receive:

- a stable mark value for Xray outbound sockets;
- a stable class id for `tc`;
- a per-user rate entry in the `AccessConfig`.

Xray routing still matches authenticated users by stats email. When a limited user is routed to an exit endpoint, the renderer creates a user-marked outbound tag for that endpoint and sets `streamSettings.sockopt.mark`. Linux then sees a mark that the limiter can classify.

The agent applies Xray config first. If Xray config succeeds, the limiter reconciles local rules from the active `AccessConfig`. This avoids installing limiter rules for a config that Xray did not accept.

## Limiter Behavior

The agent limiter is an isolated module. It converts `AccessConfig.rate_limits` into an idempotent command plan:

- prepare root HTB qdisc for outbound shaping;
- prepare IFB ingress shaping for download direction when enabled;
- install per-user classes and fwmark filters;
- install mangle/connmark rules and tc ingress `connmark` action so reply traffic can be classified before IFB redirection;
- remove stale classes and filters by reconciling the full desired state on every config refresh.

The limiter module supports dry-run for command-plan unit tests and safe local inspection only. Positive-rate runtime config application must reject dry-run and must apply limiter rules before Xray reload, so production fail-closed behavior is verified with real command execution and Docker capabilities.

## Deployment

The access-agent container needs Linux networking permissions on transit nodes:

- host network namespace or equivalent access to the host interface;
- `CAP_NET_ADMIN` for Xray `SO_MARK` and `tc`;
- optional `CAP_SYS_MODULE` only if the host needs the agent to load `ifb`.

The Xray container also needs `CAP_NET_ADMIN` when it sets `SO_MARK`.

The control-plane Web entry remains Caddy-only. No second Web component is part of the current deployment path.

## Testing

Local tests cover:

- Xray outbound generation includes `streamSettings.sockopt.mark`.
- Heartbeat config includes per-user rate limits.
- User override beats plan rate, including higher-than-plan values.
- Agent limiter produces deterministic `tc`/mark command plans.
- Empty or unlimited configs clean limiter state.

Real tests use the current private server inventory, requiring at least three real servers for client, transit, and exit roles:

- deploy updated control plane and agents;
- create a real plan, real user, transit node, group, line, and exit binding;
- configure user speed above plan speed and verify the user override wins;
- import the subscription in a real client on the test client server;
- run real TCP download traffic and confirm the observed rate is capped near the configured user speed;
- send real UDP traffic through the same user path and confirm the same limiter class records UDP bytes under the configured user speed;
- change the user rate, wait for heartbeat, and verify the new cap takes effect;
- remove or disable the user/line and verify old traffic stops.
