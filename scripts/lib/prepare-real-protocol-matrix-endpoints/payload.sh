#!/usr/bin/env bash
# 用途：根据私有协议输入生成真实协议矩阵 endpoint 创建 payload。
# 范围：仅供主脚本 source，主逻辑通过 build_endpoint_payload 调用这里的实现。
# 安全：payload 文件权限设为 600，解析错误不输出原始 URL、密码或主机内容。
# 约束：这里不调用控制面 API、不写 env，只负责校验和构造 JSON payload。

build_endpoint_payload() {
  local protocol="$1"
  local output="$2"
  PROTOCOL_NAME="$protocol" RUN_ID_VALUE="$RUN_ID" python3 - "$output" <<'PY'
import base64
import ipaddress
import json
import os
import sys
from urllib.parse import parse_qs, unquote, urlsplit

output = sys.argv[1]
protocol = os.environ["PROTOCOL_NAME"]
run_id = os.environ["RUN_ID_VALUE"]

PLACEHOLDER_BITS = (
    "<",
    "copy-from-",
    "from-private-env",
    "set-in-env",
    "private-host",
    "private-file",
    "provider-console",
    "generated-exit",
    "client-runtime",
    "observation",
    "admin-backend",
    "database",
    "compose-postgres",
    "change-me",
    "placeholder",
)

class ConfigError(Exception):
    pass

def raw(name):
    return os.environ.get(name, "").strip()

def ready_value(name):
    value = raw(name)
    if not value:
        return ""
    lowered = value.lower()
    if any(bit in lowered for bit in PLACEHOLDER_BITS):
        return ""
    return value

def first_value(*names):
    for name in names:
        value = ready_value(name)
        if value:
            return value
    return ""

def first_query(query, *names):
    for name in names:
        values = query.get(name)
        if values:
            value = values[0].strip()
            if value:
                return value
    return ""

def parse_port(value, label):
    try:
        port = int(str(value))
    except (TypeError, ValueError) as exc:
        raise ConfigError(f"{label} must be a valid port") from exc
    if port <= 0 or port > 65535:
        raise ConfigError(f"{label} must be within 1..65535")
    return port

def parse_standard_url(url, label, schemes):
    try:
        parsed = urlsplit(url)
        scheme = parsed.scheme.lower()
        if scheme not in schemes:
            raise ConfigError(f"{label} has unsupported scheme")
        host = parsed.hostname or ""
        port = parsed.port
        query = parse_qs(parsed.query, keep_blank_values=True)
    except ConfigError:
        raise
    except Exception as exc:
        raise ConfigError(f"{label} could not be parsed") from exc
    return parsed, host, port, query

def require_host_port(host, port, host_label, port_label):
    if not host:
        raise ConfigError(f"{host_label} is required")
    if port is None:
        raise ConfigError(f"{port_label} is required")
    return host, parse_port(port, port_label)

def is_ip_address(value):
    try:
        ipaddress.ip_address(value)
        return True
    except ValueError:
        return False

def optional_auth_config(username, password, label):
    if bool(username) != bool(password):
        raise ConfigError(f"{label} username and password must be set together")
    if username and password:
        return {"username": username, "password": password}
    return {}

def decode_base64_urlsafe(value):
    padded = value + ("=" * (-len(value) % 4))
    return base64.urlsafe_b64decode(padded.encode()).decode()

def parse_hostport(value, label):
    try:
        parsed = urlsplit("//" + value)
        host = parsed.hostname or ""
        port = parsed.port
    except Exception as exc:
        raise ConfigError(f"{label} host and port could not be parsed") from exc
    return require_host_port(host, port, f"{label} host", f"{label} port")

def parse_shadowsocks_url(url):
    if not url.lower().startswith("ss://"):
        raise ConfigError("THIRD_PARTY_SHADOWSOCKS_RAW_URL has unsupported scheme")
    rest = url[5:]
    rest = rest.split("#", 1)[0].split("?", 1)[0]
    if "@" in rest:
        userinfo, hostport = rest.rsplit("@", 1)
        userinfo = unquote(userinfo)
        if ":" not in userinfo:
            try:
                userinfo = decode_base64_urlsafe(userinfo)
            except Exception as exc:
                raise ConfigError("THIRD_PARTY_SHADOWSOCKS_RAW_URL credentials could not be decoded") from exc
    else:
        try:
            decoded = decode_base64_urlsafe(rest)
        except Exception as exc:
            raise ConfigError("THIRD_PARTY_SHADOWSOCKS_RAW_URL could not be decoded") from exc
        if "@" not in decoded:
            raise ConfigError("THIRD_PARTY_SHADOWSOCKS_RAW_URL is missing host")
        userinfo, hostport = decoded.rsplit("@", 1)
    if ":" not in userinfo:
        raise ConfigError("THIRD_PARTY_SHADOWSOCKS_RAW_URL is missing method/password")
    method, password = userinfo.split(":", 1)
    host, port = parse_hostport(hostport, "THIRD_PARTY_SHADOWSOCKS_RAW_URL")
    return host, port, method.strip(), password.strip()

def common_payload(outbound_type, host, port, outbound_config):
    provider_name = first_value("REAL_PROTOCOL_MATRIX_PROVIDER_NAME", "XRAYC_REAL_PROTOCOL_MATRIX_PROVIDER_NAME") or "real-protocol-matrix"
    region_code = first_value("REAL_PROTOCOL_MATRIX_REGION_CODE", "XRAYC_REAL_PROTOCOL_MATRIX_REGION_CODE") or "REAL"
    weight = parse_port(first_value("REAL_PROTOCOL_MATRIX_ENDPOINT_WEIGHT", "XRAYC_REAL_PROTOCOL_MATRIX_ENDPOINT_WEIGHT") or "100", "endpoint weight")
    priority = int(first_value("REAL_PROTOCOL_MATRIX_ENDPOINT_PRIORITY", "XRAYC_REAL_PROTOCOL_MATRIX_ENDPOINT_PRIORITY") or "100")
    return {
        "resource_name": f"real-protocol-matrix-{outbound_type}-{run_id}",
        "endpoint_name": f"real-{outbound_type}-{run_id}",
        "region_code": region_code,
        "provider_name": provider_name,
        "ownership": "third_party",
        "outbound_type": outbound_type,
        "host": host,
        "port": port,
        "outbound_config": outbound_config,
        "stream_config": {},
        "probe_config": {},
        "enabled": True,
        "weight": weight,
        "priority": max(0, priority),
        "status": "healthy",
        "allow_new_assignments": True,
    }

def build_socks():
    raw_url = first_value("THIRD_PARTY_SOCKS_RAW_URL")
    host = first_value("THIRD_PARTY_SOCKS_HOST")
    port = first_value("THIRD_PARTY_SOCKS_PORT")
    username = first_value("THIRD_PARTY_SOCKS_USERNAME", "THIRD_PARTY_SOCKS_USER")
    password = first_value("THIRD_PARTY_SOCKS_PASSWORD", "THIRD_PARTY_SOCKS_PASS", "THIRD_PARTY_SOCKS_KEY")
    if raw_url:
        parsed, url_host, url_port, _ = parse_standard_url(raw_url, "THIRD_PARTY_SOCKS_RAW_URL", {"socks", "socks4", "socks4a", "socks5", "socks5h"})
        # raw URL is the provider intent for endpoint and optional auth; stale
        # split env can only be used when no raw URL was supplied.
        host = url_host
        port = url_port
        username = unquote(parsed.username) if parsed.username else ""
        password = unquote(parsed.password) if parsed.password else ""
    host, port = require_host_port(host, port, "THIRD_PARTY_SOCKS_HOST", "THIRD_PARTY_SOCKS_PORT")
    return common_payload("socks", host, port, optional_auth_config(username, password, "SOCKS"))

def build_http():
    raw_url = first_value("THIRD_PARTY_HTTP_RAW_URL")
    host = first_value("THIRD_PARTY_HTTP_HOST")
    port = first_value("THIRD_PARTY_HTTP_PORT")
    username = first_value("THIRD_PARTY_HTTP_USERNAME", "THIRD_PARTY_HTTP_USER")
    password = first_value("THIRD_PARTY_HTTP_PASSWORD", "THIRD_PARTY_HTTP_PASS")
    key = first_value("THIRD_PARTY_HTTP_KEY")
    if raw_url:
        parsed, url_host, url_port, _ = parse_standard_url(raw_url, "THIRD_PARTY_HTTP_RAW_URL", {"http", "https"})
        # raw URL without auth means a no-auth HTTP proxy; stale split auth must
        # not be re-injected from an older private release file.
        host = url_host
        port = url_port
        username = unquote(parsed.username) if parsed.username else ""
        password = unquote(parsed.password) if parsed.password else ""
    if username and not raw_url:
        password = password or key
    if not username:
        password = ""
    host, port = require_host_port(host, port, "THIRD_PARTY_HTTP_HOST", "THIRD_PARTY_HTTP_PORT")
    return common_payload("http", host, port, optional_auth_config(username, password, "HTTP"))

def build_vless():
    raw_url = first_value("THIRD_PARTY_VLESS_RAW_URL")
    host = first_value("THIRD_PARTY_VLESS_HOST", "THIRD_PARTY_HOST")
    port = first_value("THIRD_PARTY_VLESS_PORT", "THIRD_PARTY_PORT")
    uuid = first_value("THIRD_PARTY_VLESS_UUID", "THIRD_PARTY_UUID")
    security = first_value("THIRD_PARTY_VLESS_SECURITY", "THIRD_PARTY_SECURITY")
    flow = first_value("THIRD_PARTY_VLESS_FLOW", "THIRD_PARTY_FLOW")
    server_name = first_value("THIRD_PARTY_VLESS_SERVER_NAME", "THIRD_PARTY_VLESS_SNI", "THIRD_PARTY_SERVER_NAME", "THIRD_PARTY_SNI")
    public_key = first_value("THIRD_PARTY_VLESS_PUBLIC_KEY", "THIRD_PARTY_PUBLIC_KEY")
    short_id = first_value("THIRD_PARTY_VLESS_SHORT_ID", "THIRD_PARTY_SHORT_ID")
    fingerprint = first_value("THIRD_PARTY_VLESS_FINGERPRINT", "THIRD_PARTY_FINGERPRINT")
    if raw_url:
        parsed, url_host, url_port, query = parse_standard_url(raw_url, "THIRD_PARTY_VLESS_RAW_URL", {"vless"})
        # raw URL values override stale split fields; split env only supplements
        # provider fields omitted by the URL, such as a separately supplied Reality public key.
        host = url_host or host
        port = url_port or port
        uuid = (unquote(parsed.username) if parsed.username else "") or uuid
        security = first_query(query, "security") or security
        flow = first_query(query, "flow") or flow
        server_name = first_query(query, "sni", "servername", "serverName", "peer") or server_name
        public_key = first_query(query, "pbk", "public_key", "publicKey") or public_key
        short_id = first_query(query, "sid", "short_id", "shortId") or short_id
        fingerprint = first_query(query, "fp", "fingerprint", "client_fingerprint") or fingerprint
    host, port = require_host_port(host, port, "THIRD_PARTY_VLESS_HOST", "THIRD_PARTY_VLESS_PORT")
    if not uuid:
        raise ConfigError("THIRD_PARTY_UUID or THIRD_PARTY_VLESS_RAW_URL UUID is required")
    config = {"uuid": uuid}
    if security:
        config["security"] = security.lower()
    if flow:
        config["flow"] = flow
    if server_name:
        config["server_name"] = server_name
    if public_key:
        config["public_key"] = public_key
    if short_id:
        config["short_id"] = short_id
    if fingerprint:
        config["fingerprint"] = fingerprint
    if config.get("security", "").lower() == "reality" or (not config.get("security") and public_key):
        if not server_name or not public_key:
            raise ConfigError("VLESS Reality requires server_name and public_key")
    return common_payload("vless", host, port, config)

def build_trojan():
    raw_url = first_value("THIRD_PARTY_TROJAN_RAW_URL")
    host = first_value("THIRD_PARTY_TROJAN_HOST")
    port = first_value("THIRD_PARTY_TROJAN_PORT")
    password = first_value("THIRD_PARTY_TROJAN_PASSWORD", "THIRD_PARTY_TROJAN_KEY")
    server_name = first_value("THIRD_PARTY_TROJAN_SERVER_NAME", "THIRD_PARTY_TROJAN_SNI")
    security = first_value("THIRD_PARTY_TROJAN_SECURITY")
    if raw_url:
        parsed, url_host, url_port, query = parse_standard_url(raw_url, "THIRD_PARTY_TROJAN_RAW_URL", {"trojan"})
        host = url_host
        port = url_port
        password = (unquote(parsed.username) if parsed.username else "") or password
        server_name = first_query(query, "sni", "servername", "serverName", "peer") or server_name
        security = first_query(query, "security") or security
    host, port = require_host_port(host, port, "THIRD_PARTY_TROJAN_HOST", "THIRD_PARTY_TROJAN_PORT")
    if not password:
        raise ConfigError("THIRD_PARTY_TROJAN_PASSWORD or THIRD_PARTY_TROJAN_RAW_URL password is required")
    security = (security or "tls").lower()
    if security != "tls":
        raise ConfigError("Trojan endpoints must use TLS; security=none is not allowed")
    if not server_name and not is_ip_address(host):
        server_name = host
    if not server_name:
        raise ConfigError("Trojan TLS requires THIRD_PARTY_TROJAN_SNI/server_name or a domain host")
    config = {"password": password, "security": "tls"}
    if server_name:
        config["server_name"] = server_name
    return common_payload("trojan", host, port, config)

def build_shadowsocks():
    raw_url = first_value("THIRD_PARTY_SHADOWSOCKS_RAW_URL")
    host = first_value("THIRD_PARTY_SHADOWSOCKS_HOST")
    port = first_value("THIRD_PARTY_SHADOWSOCKS_PORT")
    method = first_value("THIRD_PARTY_SHADOWSOCKS_METHOD", "THIRD_PARTY_SHADOWSOCKS_CIPHER")
    password = first_value("THIRD_PARTY_SHADOWSOCKS_PASSWORD")
    key = first_value("THIRD_PARTY_SHADOWSOCKS_KEY")
    if raw_url:
        url_host, url_port, url_method, url_password = parse_shadowsocks_url(raw_url)
        host = url_host
        port = url_port
        method = url_method
        password = url_password
    if key and not password:
        try:
            decoded_key = decode_base64_urlsafe(key)
        except Exception:
            decoded_key = key
        if ":" in decoded_key and not method:
            method, password = decoded_key.split(":", 1)
        else:
            password = decoded_key
    host, port = require_host_port(host, port, "THIRD_PARTY_SHADOWSOCKS_HOST", "THIRD_PARTY_SHADOWSOCKS_PORT")
    if not method:
        raise ConfigError("THIRD_PARTY_SHADOWSOCKS_METHOD or raw URL method is required")
    if not password:
        raise ConfigError("THIRD_PARTY_SHADOWSOCKS_PASSWORD or raw URL password is required")
    return common_payload("shadowsocks", host, port, {"method": method, "password": password})

def build_hy2():
    raw_url = first_value("THIRD_PARTY_HY2_RAW_URL")
    host = first_value("THIRD_PARTY_HY2_HOST")
    port = first_value("THIRD_PARTY_HY2_PORT")
    password = first_value("THIRD_PARTY_HY2_PASSWORD", "THIRD_PARTY_HY2_AUTH", "THIRD_PARTY_HY2_KEY")
    server_name = first_value("THIRD_PARTY_HY2_SERVER_NAME", "THIRD_PARTY_HY2_SNI")
    allow_insecure = first_value("THIRD_PARTY_HY2_ALLOW_INSECURE")
    raw_allow_insecure = ""
    if raw_url:
        parsed, url_host, url_port, query = parse_standard_url(raw_url, "THIRD_PARTY_HY2_RAW_URL", {"hy2", "hysteria", "hysteria2"})
        host = url_host
        port = url_port
        password = (unquote(parsed.username) if parsed.username else "") or first_query(query, "auth", "password") or password
        server_name = first_query(query, "sni", "servername", "serverName", "peer") or server_name
        raw_allow_insecure = first_query(query, "insecure", "allow_insecure", "allowInsecure")
    host, port = require_host_port(host, port, "THIRD_PARTY_HY2_HOST", "THIRD_PARTY_HY2_PORT")
    if not password:
        raise ConfigError("THIRD_PARTY_HY2_PASSWORD/_AUTH or THIRD_PARTY_HY2_RAW_URL password is required")
    config = {"password": password}
    if server_name:
        config["server_name"] = server_name
    # raw URL is a complete provider intent; do not let a stale env default
    # from an older private release file re-enable insecure HY2.
    allow_insecure_source = raw_allow_insecure if raw_url else allow_insecure
    if str(allow_insecure_source).lower() in {"1", "true", "yes"}:
        config["allow_insecure"] = True
    return common_payload("hysteria", host, port, config)

builders = {
    "socks": build_socks,
    "http": build_http,
    "vless": build_vless,
    "trojan": build_trojan,
    "shadowsocks": build_shadowsocks,
    "hy2": build_hy2,
}

try:
    payload = builders[protocol]()
except KeyError:
    print("endpoint payload: unsupported protocol", file=sys.stderr)
    raise SystemExit(2)
except ConfigError as exc:
    print(f"endpoint payload: {exc}", file=sys.stderr)
    raise SystemExit(2)
except Exception:
    print("endpoint payload: failed to parse private protocol input", file=sys.stderr)
    raise SystemExit(2)

with open(output, "w", encoding="utf-8") as fh:
    json.dump(payload, fh, ensure_ascii=False)
    fh.write("\n")
os.chmod(output, 0o600)
PY
}
