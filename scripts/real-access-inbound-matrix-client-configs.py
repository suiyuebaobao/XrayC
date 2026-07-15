#!/usr/bin/env python3
"""Build redacted client configs for the real inbound matrix E2E."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

try:
    import yaml
except Exception as exc:  # pragma: no cover - shell gate reports this directly.
    raise SystemExit("PyYAML is required for subscription parsing") from exc


def norm_type(value: object) -> str:
    raw = str(value or "").strip().lower()
    if raw in {"ss", "shadowsocks"}:
        return "shadowsocks"
    if raw in {"hy2", "hysteria", "hysteria2"}:
        return "hysteria2"
    return raw


def norm_protocol_key(value: object) -> str:
    raw = str(value or "").strip().lower().replace("-", "_")
    if raw == "ss":
        return "shadowsocks"
    return raw


def actual_protocol_type(protocol: str) -> str:
    protocol = norm_protocol_key(protocol)
    return norm_type(protocol)


def load_proxies(subscription_path: pathlib.Path) -> list[dict]:
    with subscription_path.open("r", encoding="utf-8") as fh:
        data = yaml.safe_load(fh)
    if not isinstance(data, dict):
        raise SystemExit("subscription YAML root is not a mapping")
    proxies = data.get("proxies")
    if not isinstance(proxies, list):
        raise SystemExit("subscription has no proxies")
    return [proxy for proxy in proxies if isinstance(proxy, dict)]


def text(proxy: dict, *keys: str) -> str:
    for key in keys:
        value = proxy.get(key)
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""


def bool_value(proxy: dict, key: str, default: bool = False) -> bool:
    value = proxy.get(key)
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.lower() in {"1", "true", "yes", "on"}
    return default


def network(proxy: dict) -> str:
    return text(proxy, "network") or "tcp"


def stream_settings(proxy: dict) -> dict:
    net = network(proxy)
    settings: dict = {"network": net}
    if net == "tcp":
        pass
    elif net == "ws":
        settings["wsSettings"] = {
            "path": text(proxy, "ws-path", "path") or "/",
            "headers": {"Host": text(proxy, "host")} if text(proxy, "host") else {},
        }
    elif net == "grpc":
        settings["grpcSettings"] = {
            "serviceName": text(proxy, "grpc-service-name", "serviceName") or "xrayc"
        }
    elif net == "xhttp":
        opts = proxy.get("xhttp-opts") if isinstance(proxy.get("xhttp-opts"), dict) else {}
        settings["xhttpSettings"] = {
            "path": str(opts.get("path") or text(proxy, "path") or "/xrayc"),
            "mode": str(opts.get("mode") or "stream-one"),
        }
    else:
        raise SystemExit(f"unsupported subscription network for protocol {norm_type(proxy.get('type'))}")

    reality = proxy.get("reality-opts") if isinstance(proxy.get("reality-opts"), dict) else None
    sni = text(proxy, "servername", "sni")
    fingerprint = text(proxy, "client-fingerprint", "fingerprint")
    if reality:
        settings["security"] = "reality"
        settings["realitySettings"] = {
            "serverName": sni,
            "fingerprint": fingerprint or "chrome",
            "publicKey": str(reality.get("public-key") or reality.get("public_key") or ""),
            "shortId": str(reality.get("short-id") or reality.get("short_id") or ""),
        }
    elif bool_value(proxy, "tls", False):
        settings["security"] = "tls"
        settings["tlsSettings"] = {"serverName": sni}
        if fingerprint:
            settings["tlsSettings"]["fingerprint"] = fingerprint
    return settings


def xray_outbound(proxy: dict) -> dict:
    ptype = norm_type(proxy.get("type"))
    server = text(proxy, "server")
    port = int(proxy.get("port") or 0)
    if not server or port <= 0:
        raise SystemExit("subscription proxy is missing server or port")
    if ptype == "trojan" and (
        not bool_value(proxy, "tls", False) or not text(proxy, "servername", "sni")
    ):
        raise SystemExit("trojan inbound matrix requires tls=true and servername/sni")
    if ptype == "vless":
        user = {"id": text(proxy, "uuid"), "encryption": "none"}
        flow = text(proxy, "flow")
        if flow:
            user["flow"] = flow
        return {
            "tag": "proxy",
            "protocol": "vless",
            "settings": {"vnext": [{"address": server, "port": port, "users": [user]}]},
            "streamSettings": stream_settings(proxy),
        }
    if ptype == "trojan":
        return {
            "tag": "proxy",
            "protocol": "trojan",
            "settings": {"servers": [{"address": server, "port": port, "password": text(proxy, "password")}]},
            "streamSettings": stream_settings(proxy),
        }
    if ptype == "shadowsocks":
        return {
            "tag": "proxy",
            "protocol": "shadowsocks",
            "settings": {
                "servers": [{
                    "address": server,
                    "port": port,
                    "method": text(proxy, "cipher", "method"),
                    "password": text(proxy, "password"),
                }]
            },
        }
    raise SystemExit(f"unsupported protocol {ptype}")


def xray_config(proxy: dict, listen_port: int) -> dict:
    return {
        "log": {"loglevel": "warning"},
        "inbounds": [{
            "tag": "local-socks",
            "listen": "127.0.0.1",
            "port": listen_port,
            "protocol": "socks",
            "settings": {"auth": "noauth", "udp": True},
        }],
        "outbounds": [xray_outbound(proxy)],
    }


def proxy_map(proxies: list[dict]) -> dict[str, dict]:
    by_protocol: dict[str, dict] = {}
    for proxy in proxies:
        by_protocol.setdefault(norm_type(proxy.get("type")), proxy)
    return by_protocol


def preferred_ports(items: list[str]) -> dict[str, int]:
    ports: dict[str, int] = {}
    for item in items:
        protocol, separator, port = item.partition("=")
        if not separator:
            raise SystemExit("preferred port must use protocol=port")
        protocol = norm_protocol_key(protocol)
        try:
            parsed_port = int(port)
        except ValueError as exc:
            raise SystemExit("preferred port must be numeric") from exc
        if parsed_port <= 0 or parsed_port > 65535:
            raise SystemExit("preferred port must be a TCP port")
        ports[protocol] = parsed_port
    return ports


def proxy_port(proxy: dict) -> int:
    try:
        return int(proxy.get("port") or 0)
    except (TypeError, ValueError):
        return 0


def cdn_tunnel_proxy(proxy: dict) -> bool:
    if network(proxy).lower() not in {"ws", "grpc"}:
        return False
    return proxy_port(proxy) == 443 and bool_value(proxy, "tls", False)


def cloudflare_marked_proxy(proxy: dict) -> bool:
    joined = " ".join(
        text(proxy, key)
        for key in (
            "name",
            "server",
            "servername",
            "sni",
            "host",
            "cdn_provider",
            "cdn",
            "provider",
            "remark",
            "remarks",
            "note",
            "notes",
            "description",
        )
    ).lower()
    return any(
        marker in joined
        for marker in ("server_4", "cloudflare", "orange-cloud", "orange cloud", "橙云")
    )


def matrix_direct_proxy(proxy: dict) -> bool:
    name = text(proxy, "name").lower()
    return "real-inbound-matrix" in name or "real-matrix" in name


def select_proxy(
    by_protocol: dict[str, list[dict]],
    protocol: str,
    port_preferences: dict[str, int],
) -> dict:
    protocol = norm_protocol_key(protocol)
    candidates = [
        proxy
        for proxy in by_protocol.get(actual_protocol_type(protocol), [])
        if not cdn_tunnel_proxy(proxy) and not cloudflare_marked_proxy(proxy)
    ]
    if not candidates:
        raise SystemExit("subscription is missing required inbound protocol entries")
    preferred_port = port_preferences.get(protocol)
    if preferred_port is None:
        matrix_candidates = [proxy for proxy in candidates if matrix_direct_proxy(proxy)]
        return (matrix_candidates or candidates)[0]
    port_matches = [proxy for proxy in candidates if proxy_port(proxy) == preferred_port]
    matrix_matches = [proxy for proxy in port_matches if matrix_direct_proxy(proxy)]
    for proxy in matrix_matches or port_matches:
        if proxy_port(proxy) == preferred_port:
            return proxy
    raise SystemExit("subscription is missing the preferred inbound protocol port")


def command_missing(args: argparse.Namespace) -> None:
    by_protocol: dict[str, list[dict]] = {}
    for proxy in load_proxies(args.subscription):
        by_protocol.setdefault(norm_type(proxy.get("type")), []).append(proxy)
    port_preferences = preferred_ports(args.preferred_port or [])
    missing = []
    for protocol in args.protocols:
        protocol = norm_protocol_key(protocol)
        try:
            select_proxy(by_protocol, protocol, port_preferences)
        except SystemExit:
            missing.append(protocol)
    if missing:
        print(",".join(missing))


def command_build(args: argparse.Namespace) -> None:
    by_protocol: dict[str, list[dict]] = {}
    for proxy in load_proxies(args.subscription):
        by_protocol.setdefault(norm_type(proxy.get("type")), []).append(proxy)
    requested_protocols = [norm_protocol_key(protocol) for protocol in args.protocols]
    missing = [
        protocol
        for protocol in requested_protocols
        if actual_protocol_type(protocol) not in by_protocol
    ]
    if missing:
        raise SystemExit("subscription is missing required inbound protocol entries")
    port_preferences = preferred_ports(args.preferred_port or [])
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for offset, protocol in enumerate(requested_protocols):
        proxy = select_proxy(by_protocol, protocol, port_preferences)
        listen_port = args.port_base + offset
        config = xray_config(proxy, listen_port)
        (args.output_dir / f"{protocol}.json").write_text(
            json.dumps(config, separators=(",", ":")),
            encoding="utf-8",
        )
        print(f"{protocol}:{args.port_base + offset}")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    missing = subparsers.add_parser("missing")
    missing.add_argument("subscription", type=pathlib.Path)
    missing.add_argument(
        "--preferred-port",
        action="append",
        default=[],
        help="Prefer a subscription proxy by protocol=port when duplicates exist.",
    )
    missing.add_argument("protocols", nargs="+")
    missing.set_defaults(func=command_missing)

    build = subparsers.add_parser("build")
    build.add_argument("subscription", type=pathlib.Path)
    build.add_argument("output_dir", type=pathlib.Path)
    build.add_argument("port_base", type=int)
    build.add_argument(
        "--preferred-port",
        action="append",
        default=[],
        help="Prefer a subscription proxy by protocol=port when duplicates exist.",
    )
    build.add_argument("protocols", nargs="+")
    build.set_defaults(func=command_build)

    args = parser.parse_args()
    args.func(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
