#!/usr/bin/env bash
# 真实多用户流量 UAT 的严格 runtime 入口断言 helper。
# shellcheck shell=bash

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

require_positive_int() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

extract_subscription_runtime_port() {
  local subscription_file="$1"
  local protocol="$2"
  local base_port="$3"
  python3 - "$subscription_file" "$protocol" "$base_port" <<'PY'
import sys

try:
    import yaml
except Exception as exc:
    raise SystemExit("PyYAML is required for strict runtime port parsing") from exc

def norm(value):
    raw = str(value or "").strip().lower()
    return "shadowsocks" if raw in {"ss", "shadowsocks"} else raw

path, protocol, base_port = sys.argv[1], norm(sys.argv[2]), int(sys.argv[3])
with open(path, "r", encoding="utf-8") as fh:
    data = yaml.safe_load(fh)
if not isinstance(data, dict):
    raise SystemExit("subscription root is invalid")
proxies = data.get("proxies")
if not isinstance(proxies, list):
    raise SystemExit("subscription proxies are missing")
if len(proxies) != 1:
    raise SystemExit("strict runtime UAT requires exactly one subscription proxy in total")
if not isinstance(proxies[0], dict):
    raise SystemExit("subscription proxy is invalid")
matches = [proxy for proxy in proxies if norm(proxy.get("type")) == protocol]
if len(matches) != 1:
    raise SystemExit("strict runtime UAT proxy protocol is invalid")
try:
    port = int(matches[0].get("port") or 0)
except (TypeError, ValueError) as exc:
    raise SystemExit("subscription proxy port is invalid") from exc
if port <= 0 or port > 65535:
    raise SystemExit("subscription proxy port is outside TCP range")
if port == base_port:
    raise SystemExit("subscription still uses the base access port")
print(port)
PY
}

verify_runtime_ports_file() {
  local ports_file="$1"
  local expected_count="$2"
  local base_port="$3"
  local row_count unique_count
  row_count="$(wc -l <"$ports_file" | tr -d '[:space:]')"
  [[ "$row_count" == "$expected_count" ]] || die "runtime port row count mismatch"
  unique_count="$(cut -f4 "$ports_file" | sort -u | wc -l | tr -d '[:space:]')"
  [[ "$unique_count" == "$expected_count" ]] || die "runtime ports are not unique per user"
  ! cut -f4 "$ports_file" | grep -qx "$base_port" || die "runtime port reused the base access port"
}

assert_remote_strict_runtime_ports() {
  local ports_file="$1"
  local ports_b64
  local xray_config_path="${XRAYC_REAL_MULTI_USER_REMOTE_XRAY_CONFIG_PATH:-/opt/xrayc/access-agent/xray/config.json}"
  local limiter_plan_path="${XRAYC_REAL_MULTI_USER_REMOTE_LIMITER_PLAN_PATH:-/opt/xrayc/access-agent/state/limiter-plan.sh}"
  local agent_state_path="${XRAYC_REAL_MULTI_USER_REMOTE_AGENT_STATE_PATH:-/opt/xrayc/access-agent/state/state.json}"
  ports_b64="$(base64 -w0 "$ports_file")"
  ssh_transit \
    "XRAYC_RUNTIME_PORTS_B64=$(shell_quote "$ports_b64") XRAYC_ACCESS_LINE_ID=$(shell_quote "$ACCESS_LINE_ID") XRAYC_RATE_LIMIT_BPS=$(shell_quote "$USER_RATE_LIMIT_BPS") XRAYC_REQUIRE_UDP=$(shell_quote "${XRAYC_REAL_MULTI_USER_REQUIRE_UDP:-0}") XRAYC_XRAY_CONFIG_PATH=$(shell_quote "$xray_config_path") XRAYC_LIMITER_PLAN_PATH=$(shell_quote "$limiter_plan_path") XRAYC_AGENT_STATE_PATH=$(shell_quote "$agent_state_path") python3 - <<'PY'
import base64
import json
import os
import re
import shlex
import subprocess
import sys

def fail(message):
    raise SystemExit(message)

def tc_rate(rate_bps):
    if rate_bps >= 1_000_000 and rate_bps % 1_000_000 == 0:
        return f'{rate_bps // 1_000_000}mbit'
    if rate_bps >= 1_000 and rate_bps % 1_000 == 0:
        return f'{rate_bps // 1_000}kbit'
    return f'{rate_bps}bit'

def run_live(command, label):
    host_result = subprocess.run(
        command,
        shell=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
    )
    result = host_result
    container_result = None
    if host_result.returncode != 0:
        container_result = run_in_agent_container(command, timeout=20)
        result = container_result
    if result.returncode != 0:
        host_stderr_len = len(host_result.stderr or '')
        container_rc = container_result.returncode if container_result else -1
        container_stderr_len = len((container_result.stderr if container_result else '') or '')
        fail(
            f'runtime limiter live {label} is unavailable '
            f'host_rc={host_result.returncode} host_stderr_len={host_stderr_len} '
            f'container_rc={container_rc} '
            f'container_stderr_len={container_stderr_len}'
        )
    return result.stdout

def run_optional(command):
    result = subprocess.run(
        command,
        shell=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
    )
    if result.returncode != 0:
        result = run_in_agent_container(command, timeout=20)
    return result.stdout if result.returncode == 0 else ''

def run_in_agent_container(command, timeout=20):
    names = subprocess.run(
        ['docker', 'ps', '--format', '{{.Names}}', '--filter', 'name=xrayc-access-agent-'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
    )
    if names.returncode != 0:
        return names
    last_result = None
    for name in [line.strip() for line in names.stdout.splitlines() if line.strip()]:
        result = subprocess.run(
            ['docker', 'exec', name, 'sh', '-lc', command],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
        if result.returncode == 0:
            return result
        last_result = result
    return last_result or subprocess.CompletedProcess(command, 127, '', 'agent container command failed')

def agent_container_names():
    names = subprocess.run(
        ['docker', 'ps', '--format', '{{.Names}}', '--filter', 'name=xrayc-access-agent-'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
    )
    if names.returncode != 0:
        return []
    return [line.strip() for line in names.stdout.splitlines() if line.strip()]

def container_path_candidates(path):
    if path.endswith('/xray/config.json') or path == '/etc/xray/config.json':
        return ['/etc/xray/config.json']
    if path.endswith('/state/state.json') or path == '/var/lib/xrayc/access-agent/state.json':
        return ['/var/lib/xrayc/access-agent/state.json']
    if path.endswith('/state/limiter-plan.sh') or path == '/var/lib/xrayc/access-agent/limiter-plan.sh':
        return ['/var/lib/xrayc/access-agent/limiter-plan.sh']
    return [path]

def read_runtime_text(path):
    for name in agent_container_names():
        for candidate in container_path_candidates(path):
            result = subprocess.run(
                ['docker', 'exec', name, 'cat', candidate],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=20,
            )
            if result.returncode == 0 and result.stdout:
                return result.stdout
    with open(path, 'r', encoding='utf-8') as fh:
        return fh.read()

def find_one(pattern, text, message):
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        fail(message)
    return match

def require_contains(text, needle, message):
    if needle not in text:
        fail(message)

def mark_tokens(mark):
    return {str(mark), f'0x{mark:x}', f'0x{mark:08x}'}

def text_has_mark(text, mark):
    lowered = text.lower()
    return any(token.lower() in lowered for token in mark_tokens(mark))

def live_has_class(text, major, class_id, rate):
    lowered = text.lower()
    return f'{major}:{class_id}' in lowered and f'rate {rate}'.lower() in lowered

def live_has_filter(text, major, class_id, mark):
    lowered = text.lower()
    return (f'flowid {major}:{class_id}' in lowered or f'classid {major}:{class_id}' in lowered) and text_has_mark(text, mark)

def live_has_ingress(text, prio, mark, ifb):
    lowered = text.lower()
    has_prio = f'pref {prio}' in lowered or f'prio {prio}' in lowered
    return has_prio and text_has_mark(text, mark) and ifb.lower() in lowered

def active_ingress_protocols(active):
    protocol = active.get('protocol')
    if isinstance(protocol, dict):
        ptype = str(protocol.get('type') or '').strip().lower()
        network = str(protocol.get('network') or active.get('transport') or '').strip().lower()
    else:
        ptype = str(protocol or '').strip().lower()
        network = str(active.get('transport') or '').strip().lower()
    if ptype in {'hysteria', 'hysteria2', 'hy2'} or network == 'hysteria':
        return [(17, 'udp')]
    if ptype in {'shadowsocks', 'ss'}:
        parts = {part.strip() for part in network.split(',') if part.strip()}
        if not parts:
            parts = {'tcp'}
        protocols = []
        if 'tcp' in parts:
            protocols.append((6, 'tcp'))
        if 'udp' in parts:
            protocols.append((17, 'udp'))
        return protocols
    return [(6, 'tcp')]

def inbound_ingress_protocols(inbound):
    protocol = str(inbound.get('protocol') or '').strip().lower()
    stream = inbound.get('streamSettings') if isinstance(inbound.get('streamSettings'), dict) else {}
    network = str(stream.get('network') or '').strip().lower()
    if protocol in {'hysteria', 'hysteria2', 'hy2'} or network == 'hysteria':
        return [(17, 'udp')]
    if protocol in {'shadowsocks', 'ss'}:
        udp_enabled = bool(inbound.get('settings', {}).get('udp')) if isinstance(inbound.get('settings'), dict) else False
        protocols = [(6, 'tcp')]
        if udp_enabled:
            protocols.append((17, 'udp'))
        return protocols
    return [(6, 'tcp')]

def derived_limit_from_plan(port, ingress_protocols, iface, ifb):
    marks = set()
    prios = {}
    for protocol_number, label in ingress_protocols:
        plan_fragment = f'tc filter add dev {iface} parent ffff: protocol ip prio '
        ingress_match = re.search(
            re.escape(plan_fragment)
            + r'([0-9]+) u32 '
            + re.escape(required_ingress_needles[protocol_number])
            + re.escape(f' match ip dport {port} 0xffff action skbedit mark ')
            + r'([0-9]+)'
            + re.escape(f' action mirred egress redirect dev {ifb}'),
            limiter_plan,
        )
        if not ingress_match:
            fail(f'runtime limiter {label} ingress plan rule is missing')
        prios[label] = int(ingress_match.group(1))
        marks.add(int(ingress_match.group(2)))
    if len(marks) != 1:
        fail('runtime limiter ingress marks are inconsistent')
    mark = next(iter(marks))
    class_match = re.search(
        re.escape(f'tc filter replace dev {iface} parent 1: protocol ip prio ')
        + r'([0-9]+) '
        + re.escape(f'handle {mark} fw flowid 1:')
        + r'([0-9]+)',
        limiter_plan,
    )
    if not class_match:
        fail('runtime limiter egress filter plan is missing')
    class_id = int(class_match.group(2))
    if class_id <= 1:
        fail('runtime limiter class is invalid')
    return mark, class_id, prios

rows = []
for line in base64.b64decode(os.environ['XRAYC_RUNTIME_PORTS_B64']).decode().splitlines():
    parts = line.split('\t')
    if len(parts) != 4:
        fail('runtime port row is invalid')
    idx, user_id, xray_key, port = parts
    rows.append((idx, user_id, xray_key, int(port)))
if not rows:
    fail('runtime ports are empty')
if len({port for _, _, _, port in rows}) != len(rows):
    fail('runtime ports are not unique')

config = json.loads(read_runtime_text(os.environ['XRAYC_XRAY_CONFIG_PATH']))
limiter_plan = read_runtime_text(os.environ['XRAYC_LIMITER_PLAN_PATH'])
try:
    state = json.loads(read_runtime_text(os.environ['XRAYC_AGENT_STATE_PATH']))
except FileNotFoundError:
    state = {}

inbounds = config.get('inbounds', [])
line_id = os.environ['XRAYC_ACCESS_LINE_ID']
expected_rate_bps = int(os.environ['XRAYC_RATE_LIMIT_BPS'])
expected_rate = tc_rate(expected_rate_bps)
require_udp = os.environ.get('XRAYC_REQUIRE_UDP', '0') in {'1', 'true', 'TRUE', 'yes', 'YES'}
state_config = state.get('config') if isinstance(state, dict) else None
if not isinstance(state_config, dict):
    state_config = {}
active_lines = state_config.get('access_lines') if isinstance(state_config.get('access_lines'), list) else []
rate_limits = state_config.get('rate_limits') if isinstance(state_config.get('rate_limits'), list) else []
iface = find_one(
    r'^tc qdisc replace dev ([A-Za-z0-9_.:-]+) root handle 1: htb',
    limiter_plan,
    'runtime limiter egress qdisc is missing',
).group(1)
ifb = find_one(
    r'^tc qdisc replace dev ([A-Za-z0-9_.:-]+) root handle 2: htb',
    limiter_plan,
    'runtime limiter IFB qdisc is missing',
).group(1)

tc_class_egress = run_live(f'tc -s class show dev {shlex.quote(iface)}', 'egress classes')
tc_filter_egress = run_live(f'tc filter show dev {shlex.quote(iface)} parent 1:', 'egress filters')
tc_ingress = run_live(
    f'tc filter show dev {shlex.quote(iface)} ingress || tc filter show dev {shlex.quote(iface)} parent ffff:',
    'ingress filters',
)
tc_class_ifb = run_live(f'tc -s class show dev {shlex.quote(ifb)}', 'IFB classes')
tc_filter_ifb = run_live(f'tc filter show dev {shlex.quote(ifb)} parent 2:', 'IFB filters')
iptables_output = run_live('iptables -t mangle -S XRAYC_LIMITER_OUTPUT', 'CONNMARK save rules')
iptables_prerouting = run_live('iptables -t mangle -S XRAYC_LIMITER_PREROUTING', 'CONNMARK restore rules')
iptables_output_hook = run_live('iptables -t mangle -S OUTPUT', 'OUTPUT hook')
iptables_prerouting_hook = run_live('iptables -t mangle -S PREROUTING', 'PREROUTING hook')
nft_ruleset = run_optional('nft list ruleset')

required_ingress_needles = {
    6: 'match ip protocol 6 0xff',
    17: 'match ip protocol 17 0xff',
}
for _, _, xray_key, port in rows:
    matches = [item for item in inbounds if int(item.get('port') or 0) == port]
    if len(matches) != 1:
        fail('runtime inbound is missing or duplicated')
    clients = matches[0].get('settings', {}).get('clients', [])
    expected_email = f'xrayc-line-{line_id}--{xray_key}'
    if len(clients) != 1 or str(clients[0].get('email', '')) != expected_email:
        fail('runtime inbound user mapping is invalid')
    active_matches = [item for item in active_lines if int(item.get('listen_port') or 0) == port]
    active = active_matches[0] if len(active_matches) == 1 else None
    if active_lines:
        if active is None:
            fail('runtime active config line is missing or duplicated')
        if active.get('source_line_id') != line_id or active.get('id') == line_id:
            fail('runtime active config source_line_id is invalid')
        active_users = active.get('users', [])
        if len(active_users) != 1 or active_users[0].get('xray_user_key') != xray_key:
            fail('runtime active config user is invalid')

    limit_matches = [item for item in rate_limits if item.get('xray_user_key') == xray_key]
    ingress_protocols = active_ingress_protocols(active) if active is not None else inbound_ingress_protocols(matches[0])
    if require_udp and 17 not in {number for number, _ in ingress_protocols}:
        fail('runtime UAT requires UDP ingress, but selected access line does not support UDP')
    if len(limit_matches) == 1:
        limit = limit_matches[0]
        if int(limit.get('rate_limit_bps') or 0) != expected_rate_bps:
            fail('runtime limiter rate does not match expected UAT rate')
        mark = int(limit.get('mark') or 0)
        class_id = int(limit.get('class_id') or 0)
        if mark <= 0 or class_id <= 1:
            fail('runtime limiter mark or class is invalid')
        derived_mark, derived_class_id, ingress_prios = derived_limit_from_plan(port, ingress_protocols, iface, ifb)
        if derived_mark != mark or derived_class_id != class_id:
            fail('runtime limiter state and plan disagree')
    elif not rate_limits:
        mark, class_id, ingress_prios = derived_limit_from_plan(port, ingress_protocols, iface, ifb)
    else:
        fail('runtime limiter user rate entry is missing or duplicated')

    for protocol_number, label in ingress_protocols:
        ingress_prio = ingress_prios[label]
        if not live_has_ingress(tc_ingress, ingress_prio, mark, ifb):
            fail(f'runtime limiter {label} ingress live rule is missing')

    require_contains(
        limiter_plan,
        f'tc class replace dev {iface} parent 1: classid 1:{class_id} htb rate {expected_rate} ceil {expected_rate}',
        'runtime limiter egress class plan is missing',
    )
    require_contains(
        limiter_plan,
        f'tc filter replace dev {iface} parent 1: protocol ip prio {class_id} handle {mark} fw flowid 1:{class_id}',
        'runtime limiter egress filter plan is missing',
    )
    require_contains(
        limiter_plan,
        f'tc class replace dev {ifb} parent 2: classid 2:{class_id} htb rate {expected_rate} ceil {expected_rate}',
        'runtime limiter IFB class plan is missing',
    )
    require_contains(
        limiter_plan,
        f'tc filter replace dev {ifb} parent 2: protocol ip prio {class_id} handle {mark} fw flowid 2:{class_id}',
        'runtime limiter IFB filter plan is missing',
    )
    require_contains(
        limiter_plan,
        f'iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark {mark} -j CONNMARK --save-mark',
        'runtime limiter CONNMARK save plan is missing',
    )
    if not live_has_class(tc_class_egress, '1', class_id, expected_rate):
        fail('runtime limiter egress live class is missing')
    if not live_has_filter(tc_filter_egress, '1', class_id, mark):
        fail('runtime limiter egress live filter is missing')
    if not live_has_class(tc_class_ifb, '2', class_id, expected_rate):
        fail('runtime limiter IFB live class is missing')
    if not live_has_filter(tc_filter_ifb, '2', class_id, mark):
        fail('runtime limiter IFB live filter is missing')
    if 'CONNMARK --save-mark' not in iptables_output or not (
        text_has_mark(iptables_output, mark) or text_has_mark(nft_ruleset, mark)
    ):
        fail('runtime limiter live CONNMARK save rule is missing')

require_contains(
    limiter_plan,
    'iptables -t mangle -A XRAYC_LIMITER_PREROUTING -j CONNMARK --restore-mark',
    'runtime limiter CONNMARK restore plan is missing',
)
if 'CONNMARK --restore-mark' not in iptables_prerouting:
    fail('runtime limiter live CONNMARK restore rule is missing')
if 'XRAYC_LIMITER_OUTPUT' not in iptables_output_hook:
    fail('runtime limiter live OUTPUT hook is missing')
if 'XRAYC_LIMITER_PREROUTING' not in iptables_prerouting_hook:
    fail('runtime limiter live PREROUTING hook is missing')
print('strict_runtime_ports=ok')
PY"
}
