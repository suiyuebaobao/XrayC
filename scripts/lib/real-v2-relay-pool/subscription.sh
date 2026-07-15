# 真实 V2 中转池订阅解析辅助函数。
# 由 scripts/real-v2-relay-pool-e2e.sh source 使用。
# 负责从订阅 YAML 中提取客户端连接参数。
# 同时校验订阅代理没有泄漏真实出口主机信息。
# 输出为主脚本可 source 的临时环境变量文件。
# 本文件不直接执行，依赖主脚本传入订阅路径和主机参数。
# shellcheck shell=bash

write_subscription_client_env() {
  local subscription_yaml="$1"
  local proxy_name="$2"
  local transit_host="$3"
  local exit_a_host="$4"
  local exit_b_host="$5"
  local output="$6"
  python3 - "$subscription_yaml" "$proxy_name" "$transit_host" "$exit_a_host" "$exit_b_host" > "$output" <<'PY'
import json
import shlex
import sys
import yaml

def normalized_protocol(value):
    raw = str(value or "").strip().lower()
    return "shadowsocks" if raw in {"ss", "shadowsocks"} else (raw or "vless")

path, proxy_name, transit_host, exit_a_host, exit_b_host = sys.argv[1:6]
with open(path, "r", encoding="utf-8") as fh:
    doc = yaml.safe_load(fh) or {}
proxies = doc.get("proxies") if isinstance(doc, dict) else None
if not isinstance(proxies, list):
    raise SystemExit("subscription proxies are missing")
matching = [
    item for item in proxies
    if isinstance(item, dict)
    and proxy_name in str(item.get("name", ""))
]
if len(matching) != 1:
    raise SystemExit(f"relay proxy match count is {len(matching)}")
proxy = matching[0]
serialized = json.dumps(proxy, ensure_ascii=False)
if exit_a_host in serialized or exit_b_host in serialized:
    raise SystemExit("subscription proxy leaked exit material")
if str(proxy.get("server", "")).strip() != transit_host:
    raise SystemExit("subscription proxy server is not the transit ingress")
port = int(proxy.get("port", 0))
if port <= 0:
    raise SystemExit("subscription proxy port is missing")
protocol = normalized_protocol(proxy.get("type"))
values = {
    "CLIENT_PROTOCOL": protocol,
    "CLIENT_SERVER": str(proxy.get("server", "")).strip(),
    "CLIENT_PORT": str(port),
    "CLIENT_UUID": "",
    "CLIENT_NETWORK": str(proxy.get("network", "tcp") or "tcp").strip() or "tcp",
    "CLIENT_SECURITY": "none",
    "CLIENT_SERVER_NAME": str(proxy.get("servername", "") or "").strip(),
    "CLIENT_FINGERPRINT": str(proxy.get("client-fingerprint", "") or "").strip(),
    "CLIENT_REALITY_PUBLIC_KEY": "",
    "CLIENT_REALITY_SHORT_ID": "",
    "CLIENT_FLOW": "",
    "CLIENT_CIPHER": "",
    "CLIENT_PASSWORD": "",
}
if protocol == "vless":
    uuid = str(proxy.get("uuid", "")).strip()
    if not uuid:
        raise SystemExit("subscription proxy uuid is missing")
    values["CLIENT_UUID"] = uuid
    values["CLIENT_SECURITY"] = "tls" if bool(proxy.get("tls")) else "none"
    values["CLIENT_FLOW"] = str(proxy.get("flow", "") or "").strip()
    reality_opts = proxy.get("reality-opts") if isinstance(proxy.get("reality-opts"), dict) else {}
    if reality_opts:
        public_key = str(
            reality_opts.get("public-key") or reality_opts.get("publicKey") or ""
        ).strip()
        short_id = str(
            reality_opts.get("short-id") or reality_opts.get("shortId") or ""
        ).strip()
        if not public_key:
            raise SystemExit("subscription reality public key is missing")
        values["CLIENT_SECURITY"] = "reality"
        values["CLIENT_REALITY_PUBLIC_KEY"] = public_key
        values["CLIENT_REALITY_SHORT_ID"] = short_id
elif protocol == "shadowsocks":
    cipher = str(proxy.get("cipher", proxy.get("method", "")) or "").strip()
    password = str(proxy.get("password", "") or "").strip()
    if not cipher:
        raise SystemExit("subscription proxy cipher is missing")
    if not password:
        raise SystemExit("subscription proxy password is missing")
    values["CLIENT_CIPHER"] = cipher
    values["CLIENT_PASSWORD"] = password
else:
    raise SystemExit(f"unsupported relay subscription protocol: {protocol}")
for key, value in values.items():
    print(f"{key}={shlex.quote(value)}")
PY
}
