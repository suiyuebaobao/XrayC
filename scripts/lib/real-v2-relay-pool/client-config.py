#!/usr/bin/env python3
"""Build the Xray client config used by the real V2 relay-pool E2E."""

from __future__ import annotations

import json
import os
import sys


def env_text(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def env_int(name: str) -> int:
    value = int(env_text(name, "0") or "0")
    if value <= 0:
        raise SystemExit(f"{name} must be a positive integer")
    return value


def vless_outbound() -> dict:
    user = {"id": env_text("CLIENT_UUID"), "encryption": "none"}
    if not user["id"]:
        raise SystemExit("CLIENT_UUID is required for VLESS")
    flow = env_text("CLIENT_FLOW")
    if flow:
        user["flow"] = flow
    stream_settings = {"network": env_text("CLIENT_NETWORK", "tcp") or "tcp"}
    security = env_text("CLIENT_SECURITY", "none").lower()
    if security and security != "none":
        stream_settings["security"] = security
        server_name = env_text("CLIENT_SERVER_NAME")
        fingerprint = env_text("CLIENT_FINGERPRINT", "chrome") or "chrome"
        if security == "reality":
            public_key = env_text("CLIENT_REALITY_PUBLIC_KEY")
            if not public_key:
                raise SystemExit("CLIENT_REALITY_PUBLIC_KEY is required")
            reality_settings = {
                "fingerprint": fingerprint,
                "publicKey": public_key,
            }
            if server_name:
                reality_settings["serverName"] = server_name
            short_id = env_text("CLIENT_REALITY_SHORT_ID")
            if short_id:
                reality_settings["shortId"] = short_id
            stream_settings["realitySettings"] = reality_settings
        elif security == "tls" and server_name:
            stream_settings["tlsSettings"] = {"serverName": server_name}
    return {
        "tag": "proxy",
        "protocol": "vless",
        "settings": {
            "vnext": [
                {
                    "address": env_text("CLIENT_SERVER"),
                    "port": env_int("CLIENT_PORT"),
                    "users": [user],
                }
            ]
        },
        "streamSettings": stream_settings,
        "mux": {
            "enabled": True,
            "concurrency": 8,
            "xudpConcurrency": 16,
            "xudpProxyUDP443": "allow",
        },
    }


def shadowsocks_outbound() -> dict:
    method = env_text("CLIENT_CIPHER", env_text("CLIENT_METHOD"))
    password = env_text("CLIENT_PASSWORD")
    if not method:
        raise SystemExit("CLIENT_CIPHER is required for Shadowsocks")
    if not password:
        raise SystemExit("CLIENT_PASSWORD is required for Shadowsocks")
    return {
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": {
            "address": env_text("CLIENT_SERVER"),
            "port": env_int("CLIENT_PORT"),
            "method": method,
            "password": password,
        },
    }


def outbound() -> dict:
    protocol = env_text("CLIENT_PROTOCOL", "vless").lower()
    if protocol in {"", "vless"}:
        return vless_outbound()
    if protocol in {"ss", "shadowsocks"}:
        return shadowsocks_outbound()
    raise SystemExit(f"unsupported client protocol: {protocol}")


def build_config() -> dict:
    return {
        "log": {"loglevel": "warning"},
        "inbounds": [
            {
                "tag": "socks-in",
                "listen": "127.0.0.1",
                "port": env_int("CLIENT_SOCKS_PORT"),
                "protocol": "socks",
                "settings": {"udp": True},
            },
            {
                "tag": "udp-in",
                "listen": "127.0.0.1",
                "port": env_int("CLIENT_UDP_PORT"),
                "protocol": "dokodemo-door",
                "settings": {
                    "address": env_text("UDP_TARGET_HOST"),
                    "port": env_int("UDP_TARGET_PORT"),
                    "network": "udp",
                },
            },
        ],
        "outbounds": [outbound()],
    }


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: client-config.py OUTPUT")
    path = sys.argv[1]
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(build_config(), separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
