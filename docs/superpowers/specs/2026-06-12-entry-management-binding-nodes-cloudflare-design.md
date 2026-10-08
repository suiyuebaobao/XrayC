# Entry Management, Binding Nodes, and Cloudflare Entry Design

## Goal

Rebuild the product model around explicit entry management:

```text
出口管理 -> 入口管理 -> 入口出口绑定节点 -> 分组 -> 套餐 -> 订阅
```

The previous model creates a user-visible node directly when a transit node binds an exit line. The new model makes entries first-class resources, makes exits the managed outbound inventory, and treats each `entry + exit` binding as one selectable and subscribable node.

## Non-Negotiable Safety Rules

- Do not reset existing user subscription tokens, package assignments, group grants, or active subscription state.
- Real testing may create new temporary users, entries, bindings, and groups with an explicit test marker. Existing user data must remain intact.
- Existing nodes must keep working until their specific entry is intentionally migrated. Real Cloudflare 443 test targets must be read from the latest private inventory and private role notes.
  `server_4` is the Cloudflare/orange-cloud node.
  `server_1` is direct/non-CF and must not be used for orange-cloud validation.
  Public examples must use RFC 5737 IPs and example domains.
- AnyTLS is a standalone sing-box protocol, not a VLESS transport/security mode. Do not implement or document `VLESS + AnyTLS`; Cloudflare orange-cloud entries use HTTP proxy paths such as TLS + WebSocket or gRPC.

## Product Definitions

### 出口管理

The current "出口管理" becomes "出口管理". It manages outbound endpoints only:

- third-party SOCKS/HTTP/VLESS/Trojan/Shadowsocks/HY2 exits
- ordinary self-hosted upstream exits
- exits created by a transit node's local exit service
- probing, editing, deletion, ownership, and source display

An exit is not enough to create a subscription item. It becomes subscribable only after being bound to an entry.

### 入口管理

Entry management owns user-facing ingress on a specific transit node:

- transit node
- listen host and port
- protocol: VLESS, Trojan, HY2, Shadowsocks
- transport/security: Reality/TCP, TLS/WebSocket, TLS/TCP, UDP, etc.
- credential material needed by users and Xray
- TLS/SNI, WebSocket path/host, CDN configuration
- synchronization and runtime status

An entry belongs to one transit node. A transit node can have many entries.

### 入口出口绑定节点

Each `entry + exit_endpoint` binding is one selectable node.

Examples:

```text
入口 A + 出口 1 = 节点 A-1
入口 B + 出口 1 = 节点 B-1
入口 A + 出口 2 = 节点 A-2
```

The system supports one-to-one, one-to-many, many-to-one, and many-to-many relationships between entries and exits. No automatic de-duplication happens at grouping or subscription time. If three bindings are added to a group, the subscription emits three nodes.

### 分组

Line groups no longer select raw exits. They select binding nodes. A binding node row must show enough context for administrators to make a deliberate choice:

- subscription node name
- entry protocol, transport, security, host, port, CDN state
- exit name, protocol, region, source
- transit node
- enabled/sync/health status

Packages continue to authorize groups. Rules continue to attach to groups through the rule library.

## Subscription Semantics

Subscription generation starts from the user's authorized groups, reads the binding nodes in those groups, and emits one proxy per binding node.

Rules:

- One binding node in an authorized group equals one subscription proxy.
- Do not merge rows that share the same entry.
- Do not merge rows that share the same exit.
- Do not expose exit credentials, exit host, exit proxy URL, agent token, or Reality private key.
- The emitted proxy uses entry connection parameters and user credential material.
- The internal runtime route for that proxy uses the binding's exit endpoint.

## Binding Node Identity

When one entry is bound to multiple exits, several subscription proxies may share the same `server`, `port`, `network`, `servername`, and path. They must still be distinguishable at Xray runtime. Therefore each binding node gets a binding-scoped user credential for subscription and Xray clients:

```text
binding_user_credential = stable_derive(user.access_credential, entry_exit_binding_id, protocol)
stats email line id = entry_exit_binding_id
stats email user key = user.xray_user_key
```

This keeps traffic attribution at the user level while allowing routing rules to direct each binding node to its own exit endpoint. The user's public subscription token and account are not reset. Existing user-level rate limits still group by `xray_user_key`, not by binding credential.

## Cloudflare/CDN Entry

Cloudflare is an entry capability, not an exit capability and not a group capability.

First implementation target:

```text
Transit node: example-transit-1 / 203.0.113.10
Domain: cdn-entry.example.test
Port: 443
Protocol: VLESS
Security: TLS
Transport: WebSocket
WS path: /vless-ws
CDN provider: cloudflare
```

Second allowed Cloudflare test target:

```text
Transit node: example-transit-2 / 198.51.100.20
Domain: panel.example.test
Port: 443
Protocol: VLESS
Security: TLS
Transport: WebSocket
WS path: /vless-ws
CDN provider: cloudflare
```

Real Cloudflare test domains from the private inventory may reach Cloudflare edge while source port 443 is still handled by Xray and returns a Cloudflare "Edge IP Restricted" fallback page.
Implementation must replace `server_4`'s selected 443 ingress with the new WebSocket entry before claiming the Cloudflare path works.
`server_1` remains a direct/non-CF target.

Subscription output for Cloudflare WebSocket entries:

```yaml
type: vless
server: cdn-entry.example.test
port: 443
tls: true
servername: cdn-entry.example.test
network: ws
ws-opts:
  path: /vless-ws
  headers:
    Host: cdn-entry.example.test
```

The first release does not include an "优选 IP" menu and does not automatically benchmark Cloudflare IPs. Manual hostname or manual preferred IP fields may exist on the entry, but automatic screening is out of scope.

## 443 Migration Rule

The current private-inventory Cloudflare test node's 443 entry may be a VLESS Reality-style entry. For the Cloudflare entry test, port 443 may be migrated to VLESS TLS WebSocket only on `server_4`. Existing clients using the old 443 Reality node must pull a fresh subscription after this intentional migration. Broader node changes require explicit current-task authorization.

## Data Model Direction

Add first-class entry and binding records while keeping compatibility during migration.

Suggested tables:

- `access_entries`
  - `id`
  - `access_node_id`
  - `name`
  - `listen_host`
  - `listen_port`
  - `protocol`
  - `transport`
  - `security`
  - `server_name`
  - `public_key`
  - `short_id`
  - `flow`
  - `udp_enabled`
  - `udp_packet_encoding`
  - `ws_path`
  - `ws_host`
  - `xhttp_path`
  - `xhttp_host`
  - `xhttp_mode`
  - `cdn_enabled`
  - `cdn_provider`
  - `cdn_hostname`
  - `cdn_server`
  - `inbound_config`
  - `enabled`
  - `sort_weight`

- `access_entry_exit_bindings`
  - `id`
  - `entry_id`
  - `exit_endpoint_id`
  - `name`
  - `enabled`
  - `sort_weight`
  - `remark`

- `line_group_binding_nodes`
  - `line_group_id`
  - `entry_exit_binding_id`
  - `position`
  - unique pair on `(line_group_id, entry_exit_binding_id)`

Compatibility:

- Existing `access_lines` rows migrate into one `access_entries` row and one `entry_exit_bindings` row per old row.
- Existing `line_group_exit_endpoints` membership migrates by selecting current bindings whose `exit_endpoint_id` was in the group.
- During migration, old tables and fields are kept until new generation paths pass tests.

## API Direction

New admin routes:

- `GET /api/admin/access-entries`
- `POST /api/admin/access-entries`
- `PUT /api/admin/access-entries/{id}`
- `DELETE /api/admin/access-entries/{id}`
- `GET /api/admin/access-entry-exit-bindings`
- `POST /api/admin/access-entries/{entry_id}/exit-bindings`
- `PUT /api/admin/access-entry-exit-bindings/{id}`
- `DELETE /api/admin/access-entry-exit-bindings/{id}`
- `PUT /api/admin/line-groups/{id}/binding-nodes`

Existing routes remain temporarily:

- `/api/admin/access-nodes/{id}/line-entries` is removed from the public API; use `/api/admin/access-entries` and `/api/admin/access-entries/{id}/exit-bindings`.
- `/api/admin/line-groups/{id}/lines` becomes compatibility-only or read-only after the UI migration.

## Frontend Direction

Navigation:

- Rename "出口管理" to "出口管理".
- Add "入口管理".
- Keep "分组", but the selector changes from exits to binding nodes.

Entry management page:

- list entries by transit node
- create/edit entry protocol, port, domain, transport, TLS/Reality/CDN fields
- show bound exits under each entry
- bind/unbind exits from the entry page

Line group page:

- display and select binding nodes, not raw exits
- include entry and exit context in each option row
- keep rule library binding at the group level

Transit node page:

- show entry count, binding count, sync state, and health
- link to entry management
- no longer serve as the primary binding workflow

## Agent and Xray Direction

The access-agent receives entries and binding routes:

- Xray inbound is generated per entry.
- Routing/outbound selection is generated per entry-exit binding.
- User-level credentials and traffic stats still map to the emitted subscription proxy.
- VLESS Reality remains TCP-only.
- Cloudflare first release uses VLESS/Trojan TLS WebSocket on 443; gRPC remains the same HTTP proxy class if enabled later.
- AnyTLS is not a Cloudflare orange-cloud VLESS mode and must not be encoded as VLESS transport.
- Xray directly terminates TLS/WebSocket for access-node Cloudflare entries in this release. Caddy remains the control-plane web/API front and is not introduced into the access-node ingress path.

## Testing Requirements

Automated tests:

- migration creates entries and binding nodes from old `access_lines`
- group membership migrates from exit endpoint membership to binding node membership
- subscription emits every selected binding node without de-duplication
- multiple entries to one exit produce multiple group-selectable nodes
- one entry to multiple exits produces multiple group-selectable nodes
- Cloudflare WebSocket subscription fields are correct
- disabling an entry or binding removes only that binding node from subscriptions
- deleting an exit cleans affected binding nodes and group membership

Real tests:

- do not reset current users' subscriptions
- create temporary test binding nodes and a temporary test user/group when needed
- validate the selected private-inventory Cloudflare edge hostname for `server_4` is reachable
- keep public examples limited to `cdn-entry.example.test` or `panel.example.test`
- validate the test entry can be synced to the allowed transit node
- validate a real client can import and connect through VLESS TLS WebSocket
- validate traffic reaches the selected exit and accounting is attributed to the test user
