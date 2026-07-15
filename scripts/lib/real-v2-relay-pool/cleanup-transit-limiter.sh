#!/usr/bin/env bash

cleanup_transit_limiter() {
  remote_exec 2 "$(cat <<'SH'
set +e
plan=/opt/xrayc-real-relay/state/limiter-plan.sh
iface=""
ifb=""
if [ -s "$plan" ]; then
  iface=$(awk '/tc qdisc replace dev/ && /root handle 1:/ { print $5; exit }' "$plan" 2>/dev/null)
  ifb=$(awk '/tc qdisc replace dev/ && /root handle 2:/ { print $5; exit }' "$plan" 2>/dev/null)
fi
if [ -z "$iface" ]; then
  iface=$(ip route show default 2>/dev/null | awk '/default/ { for (i = 1; i <= NF; i++) if ($i == "dev") { print $(i + 1); exit } }')
fi
[ -n "$ifb" ] || ifb=ifb-xrayc
if [ -n "$iface" ]; then
  tc qdisc del dev "$iface" root >/dev/null 2>&1 || true
  tc qdisc del dev "$iface" ingress >/dev/null 2>&1 || true
fi
tc qdisc del dev "$ifb" root >/dev/null 2>&1 || true
ip link delete "$ifb" type ifb >/dev/null 2>&1 || true
while iptables -t mangle -D OUTPUT -j XRAYC_LIMITER_OUTPUT >/dev/null 2>&1; do :; done
while iptables -t mangle -D PREROUTING -j XRAYC_LIMITER_PREROUTING >/dev/null 2>&1; do :; done
iptables -t mangle -F XRAYC_LIMITER_OUTPUT >/dev/null 2>&1 || true
iptables -t mangle -F XRAYC_LIMITER_PREROUTING >/dev/null 2>&1 || true
iptables -t mangle -X XRAYC_LIMITER_OUTPUT >/dev/null 2>&1 || true
iptables -t mangle -X XRAYC_LIMITER_PREROUTING >/dev/null 2>&1 || true
SH
)"
}
