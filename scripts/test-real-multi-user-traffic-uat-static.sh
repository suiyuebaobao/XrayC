#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

die() {
  echo "$*" >&2
  exit 1
}

# shellcheck source=scripts/lib/real-multi-user-traffic-uat/runtime-port.sh
. "$ROOT_DIR/scripts/lib/real-multi-user-traffic-uat/runtime-port.sh"

cat >"$tmp_dir/two-proxies.yaml" <<'YAML'
proxies:
  - name: runtime-vless
    type: vless
    server: example.test
    port: 24444
  - name: extra-trojan
    type: trojan
    server: example.test
    port: 24445
YAML

if extract_subscription_runtime_port "$tmp_dir/two-proxies.yaml" vless 24443 >/dev/null 2>&1; then
  echo "expected subscription with two proxies to fail strict parser" >&2
  exit 1
fi

cat >"$tmp_dir/one-proxy.yaml" <<'YAML'
proxies:
  - name: runtime-ss
    type: ss
    server: example.test
    port: 24446
YAML

actual_port="$(extract_subscription_runtime_port "$tmp_dir/one-proxy.yaml" shadowsocks 24443)"
[[ "$actual_port" == "24446" ]] || {
  echo "expected strict parser to return the only proxy port" >&2
  exit 1
}

captured="$tmp_dir/remote-command.txt"
ssh_transit() {
  printf '%s\n' "$*" >"$captured"
}

ACCESS_LINE_ID="line-a"
USER_RATE_LIMIT_BPS="1000000"
ports_file="$tmp_dir/runtime-ports.tsv"
printf '1\tuser-a\tu-a@xrayc.local\t24446\n' >"$ports_file"
assert_remote_strict_runtime_ports "$ports_file"

require_remote_contract() {
  local pattern="$1"
  local label="$2"
  if ! grep -Fq "$pattern" "$captured"; then
    echo "missing remote strict runtime port contract: $label" >&2
    exit 1
  fi
}

require_remote_contract "XRAYC_RATE_LIMIT_BPS" "rate limit env"
require_remote_contract "tc filter show dev" "live tc filters"
require_remote_contract "tc -s class show dev" "live tc class stats"
require_remote_contract "iptables -t mangle -S" "live iptables rules"
require_remote_contract "nft list ruleset" "live nft mark rules"
require_remote_contract "match ip protocol 6 0xff" "TCP ingress plan assertion"
require_remote_contract "match ip protocol 17 0xff" "UDP ingress plan assertion"
require_remote_contract "runtime UAT requires UDP ingress" "explicit UDP requirement guard"
require_remote_contract "CONNMARK --save-mark" "CONNMARK save assertion"
require_remote_contract "CONNMARK --restore-mark" "CONNMARK restore assertion"
require_remote_contract "flowid 1:" "egress class filter assertion"
require_remote_contract "flowid 2:" "IFB class filter assertion"

for pattern in \
  "XRAYC_REAL_MULTI_USER_ROUNDS" \
  "assert_remote_traffic_results" \
  "download_ok" \
  "upload_ok" \
	  "user_pids" \
	  "wait \"\$pid\"" \
	  "UPLOAD_URL" \
	  "upload_sink_readiness=failed" \
	  "multi-user cleanup db rows were not cleared" \
	  "multi-user cleanup remote clients were not cleared" \
	  "subscription_token_active_count" \
	  "user_access_assignment_count" \
		  "inventory_aliases=ok" \
	  "ENDPOINTS_TSV" \
	  "xrayc_multi_exit_endpoints" \
	  "real_multi_user_traffic_uat: exit_endpoint_count=" \
	  "real_multi_user_traffic_uat: verify endpoint=" \
	  "multi-user endpoint verification failed" \
	  "real_multi_user_traffic_uat: temp_plan_created" \
	  "cleanup_temp_plan" \
	  "DELETE FROM user_subscriptions" \
	  "real_multi_user_traffic_uat_cleanup_plan" \
	  "CALLER_ACCESS_LINE_ID" \
	  "preserve caller-provided runtime line" \
		  "reapply_explicit_exit_assignments" \
		  "real_multi_user_traffic_uat: explicit_exit_assignments=ok" \
			  "real_multi_user_traffic_uat: exit_pool_strategy=priority" \
	  "restore_exit_pool_strategy" \
	  "inventory_target_host_csv" \
	  "inventory_access_hosts" \
	  "n.public_host = ANY(string_to_array(:'inventory_access_hosts', ','))" \
	  "n.config_dirty = FALSE" \
	  "LINE_SELECTION_TSV" \
	  "active access line or plan is missing" \
	  "lower(COALESCE(NULLIF(trim(r.status), ''), 'healthy')) <> 'offline'" \
	  "COALESCE(s.effective_status, '') = 'healthy'" \
		  "s.access_node_id = :'access_node_id'::uuid" \
		  "bound_probe.effective_status" \
		  "bound_endpoint_resource.status" \
		  "public_domain" \
	  "TRANSIT_PUBLIC_HOST" \
	  "XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES:-3" \
	  "XRAYC_REAL_MULTI_USER_ROUND_SETTLE_SECONDS:-90" \
	  "round_settle_seconds=" \
	  "for attempt in 1 2" \
		  "XRAYC_REAL_MULTI_USER_REQUIRE_MULTI_EXIT:-0" \
	  "for _ in \$(seq 1 90)" \
		  "XRAYC_REAL_MULTI_USER_CLEANUP_ON_FAILURE" \
		  "XRAYC_REAL_MULTI_USER_EXPECTED_EXIT_IP" \
		  "readiness.err" \
		  "docker logs --tail 80" \
		  "def do_GET" \
		  "/download?bytes=1048576"; do
  if ! grep -R -Fq "$pattern" \
    scripts/real-multi-user-traffic-uat.sh \
    scripts/lib/real-multi-user-traffic-uat/control.sh \
    scripts/lib/real-multi-user-traffic-uat/selection.sh \
    scripts/lib/real-multi-user-traffic-uat/plan.sh \
    scripts/lib/real-multi-user-traffic-uat/remote-runner.sh; then
    echo "missing multi-user UAT static contract: $pattern" >&2
    exit 1
  fi
done

if grep -Eq '^[[:space:]]*wait[[:space:]]*$' scripts/lib/real-multi-user-traffic-uat/remote-runner.sh; then
  echo "remote runner must not use bare wait because upload sink is a long-lived background child" >&2
  exit 1
fi

if grep -Fq "WHERE active_exit_endpoint_count >= 1" scripts/real-multi-user-traffic-uat.sh; then
  echo "multi-user UAT must be able to select a clean line before adding healthy temporary exits" >&2
  exit 1
fi

if grep -Fq 'EXPECTED_EXIT_IP_FOR_REMOTE="${EXPECTED_EXIT_IP:-}"' scripts/real-multi-user-traffic-uat.sh; then
  echo "multi-user UAT must not inherit the global EXPECTED_EXIT_IP by default" >&2
  exit 1
fi

printf 'real-multi-user-traffic-uat-static: passed\n'
