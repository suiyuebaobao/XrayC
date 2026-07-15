#!/usr/bin/env bash
# 用途：生成真实协议上游实验所需的本地 payload 和私有 env 片段。
# 包含 Xray、Hysteria、Docker Compose 配置和敏感连接参数的写入逻辑。
# 该文件仅由 scripts/real-protocol-upstream-lab.sh source，不直接执行。

generate_env_values() {
  python3 - "$ENV_VALUES_JSON" "$BASIC_EXIT_HOST" "$TLS_EXIT_HOST" "${BASIC_EXIT_DOMAIN:-}" "${TLS_EXIT_DOMAIN:-}" "${TLS_EXIT_REMOTE_INDEX:-}" "$HTTP_PORT" "$SOCKS_PORT" "$VLESS_PORT" "$SHADOWSOCKS_PORT" "$TROJAN_PORT" "$HY2_PORT" "$PUBLIC_IP_URL" <<'PY'
import base64
import json
import secrets
import string
import sys
import uuid
from urllib.parse import quote

output, basic_host, tls_host, basic_domain, tls_domain, tls_remote_index, http_port, socks_port, vless_port, ss_port, trojan_port, hy2_port, public_ip_url = sys.argv[1:14]
alphabet = string.ascii_letters + string.digits

def token(length=24):
    return "".join(secrets.choice(alphabet) for _ in range(length))

def require_domain(value, label):
    value = value.strip()
    if not value or value.replace(".", "").isdigit():
        raise SystemExit(f"{label} is required for TLS protocol lab")
    return value

http_user = "xrayc"
http_password = token()
socks_user = "xrayc"
socks_password = token()
vless_uuid = str(uuid.uuid4())
trojan_password = token(28)
ss_method = "aes-128-gcm"
ss_password = token(28)
hy2_password = token(28)
trojan_domain = require_domain(tls_domain, "TLS exit public domain")
hy2_domain = require_domain(tls_domain, "TLS exit public domain for HY2")
tls_connect_host = "127.0.0.1" if tls_remote_index == "2" else (tls_host or tls_domain)

ss_userinfo = base64.urlsafe_b64encode(f"{ss_method}:{ss_password}".encode()).decode().rstrip("=")

values = {
    "HTTP_HOST": basic_host,
    "HTTP_PORT": http_port,
    "HTTP_USERNAME": http_user,
    "HTTP_PASSWORD": http_password,
    "HTTP_RAW_URL": f"{'http'}://{quote(http_user)}:{quote(http_password)}@{basic_host}:{http_port}",
    "SOCKS_HOST": basic_host,
    "SOCKS_PORT": socks_port,
    "SOCKS_USERNAME": socks_user,
    "SOCKS_PASSWORD": socks_password,
    "SOCKS_RAW_URL": f"{'socks5'}://{quote(socks_user)}:{quote(socks_password)}@{basic_host}:{socks_port}",
    "VLESS_HOST": basic_host,
    "VLESS_PORT": vless_port,
    "VLESS_UUID": vless_uuid,
    "VLESS_RAW_URL": f"vless://{vless_uuid}@{basic_host}:{vless_port}?encryption=none&security=none&type=tcp#xrayc-real-vless",
    "SHADOWSOCKS_HOST": basic_host,
    "SHADOWSOCKS_PORT": ss_port,
    "SHADOWSOCKS_METHOD": ss_method,
    "SHADOWSOCKS_PASSWORD": ss_password,
    "SHADOWSOCKS_RAW_URL": f"ss://{ss_userinfo}@{basic_host}:{ss_port}#xrayc-real-shadowsocks",
    "TROJAN_HOST": tls_connect_host,
    "TROJAN_PORT": trojan_port,
    "TROJAN_PASSWORD": trojan_password,
    "TROJAN_SERVER_NAME": trojan_domain,
    "TROJAN_SECURITY": "tls",
    "TROJAN_RAW_URL": f"trojan://{quote(trojan_password)}@{tls_connect_host}:{trojan_port}?security=tls&sni={quote(trojan_domain)}&type=tcp#xrayc-real-trojan",
    "HY2_HOST": tls_connect_host,
    "HY2_PORT": hy2_port,
    "HY2_PASSWORD": hy2_password,
    "HY2_SERVER_NAME": hy2_domain,
    "HY2_RAW_URL": f"hysteria2://{quote(hy2_password)}@{tls_connect_host}:{hy2_port}?sni={quote(hy2_domain)}#xrayc-real-hy2",
    "PUBLIC_IP_URL": public_ip_url,
}

with open(output, "w", encoding="utf-8") as fh:
    json.dump(values, fh, indent=2)
    fh.write("\n")
PY
  chmod 600 "$ENV_VALUES_JSON"
}

write_xray_config() {
  local output="$1"
  local server_role="$2"
  python3 - "$ENV_VALUES_JSON" "$output" "$server_role" <<'PY'
import json
import sys

values_file, output, role = sys.argv[1:4]
with open(values_file, "r", encoding="utf-8") as fh:
    values = json.load(fh)

inbounds = []
if role == "basic-exit":
    inbounds.extend(
        [
            {
                "tag": "http-connect",
                "listen": "0.0.0.0",
                "port": int(values["HTTP_PORT"]),
                "protocol": "http",
                "settings": {
                    "accounts": [
                        {
                            "user": values["HTTP_USERNAME"],
                            "pass": values["HTTP_PASSWORD"],
                        }
                    ]
                },
            },
            {
                "tag": "socks",
                "listen": "0.0.0.0",
                "port": int(values["SOCKS_PORT"]),
                "protocol": "socks",
                "settings": {
                    "auth": "password",
                    "accounts": [
                        {
                            "user": values["SOCKS_USERNAME"],
                            "pass": values["SOCKS_PASSWORD"],
                        }
                    ],
                    "udp": True,
                },
            },
            {
                "tag": "vless",
                "listen": "0.0.0.0",
                "port": int(values["VLESS_PORT"]),
                "protocol": "vless",
                "settings": {
                    "clients": [
                        {
                            "id": values["VLESS_UUID"],
                            "email": "xrayc-real-vless",
                        }
                    ],
                    "decryption": "none",
                },
                "streamSettings": {"network": "tcp", "security": "none"},
            },
            {
                "tag": "shadowsocks",
                "listen": "0.0.0.0",
                "port": int(values["SHADOWSOCKS_PORT"]),
                "protocol": "shadowsocks",
                "settings": {
                    "method": values["SHADOWSOCKS_METHOD"],
                    "password": values["SHADOWSOCKS_PASSWORD"],
                    "network": "tcp,udp",
                },
            },
        ]
    )
else:
    trojan_domain = values["TROJAN_SERVER_NAME"]
    inbounds.append(
        {
            "tag": "trojan",
            "listen": "0.0.0.0",
            "port": int(values["TROJAN_PORT"]),
            "protocol": "trojan",
            "settings": {
                "clients": [
                    {
                        "password": values["TROJAN_PASSWORD"],
                        "email": "xrayc-real-trojan",
                    }
                ]
            },
            "streamSettings": {
                "network": "tcp",
                "security": "tls",
                "tlsSettings": {
                    "serverName": trojan_domain,
                    "certificates": [
                        {
                            "certificateFile": f"/etc/letsencrypt/live/{trojan_domain}/fullchain.pem",
                            "keyFile": f"/etc/letsencrypt/live/{trojan_domain}/privkey.pem",
                        }
                    ],
                },
            },
        }
    )

config = {
    "log": {"loglevel": "warning"},
    "inbounds": inbounds,
    "outbounds": [{"tag": "direct", "protocol": "freedom"}],
}
with open(output, "w", encoding="utf-8") as fh:
    json.dump(config, fh, indent=2)
    fh.write("\n")
PY
  chmod 644 "$output"
}

write_hysteria_config() {
  local output="$1"
  python3 - "$ENV_VALUES_JSON" "$output" <<'PY'
import json
import sys

values_file, output = sys.argv[1:3]
with open(values_file, "r", encoding="utf-8") as fh:
    values = json.load(fh)

with open(output, "w", encoding="utf-8") as fh:
    fh.write(f"listen: :{int(values['HY2_PORT'])}\n")
    fh.write("tls:\n")
    fh.write(f"  cert: /etc/letsencrypt/live/{values['HY2_SERVER_NAME']}/fullchain.pem\n")
    fh.write(f"  key: /etc/letsencrypt/live/{values['HY2_SERVER_NAME']}/privkey.pem\n")
    fh.write("auth:\n")
    fh.write("  type: password\n")
    fh.write(f"  password: {json.dumps(values['HY2_PASSWORD'])}\n")
    fh.write("masquerade:\n")
    fh.write("  type: proxy\n")
    fh.write("  proxy:\n")
    fh.write("    url: https://example.com\n")
    fh.write("    rewriteHost: true\n")
PY
  chmod 644 "$output"
}

write_compose() {
  local output="$1"
  local server_role="$2"
  local project_name="${PROJECT_PREFIX}-${server_role}"
  local xray_container="${project_name}-xray"
  local hysteria_container="${project_name}-hy2"

  {
    printf 'services:\n'
    printf '  xray:\n'
    printf '    image: %s\n' "$XRAY_IMAGE"
    printf '    container_name: %s\n' "$xray_container"
    printf '    user: "0:0"\n'
    printf '    restart: unless-stopped\n'
    printf '    volumes:\n'
    printf '      - ./xray.json:/etc/xray/config.json:ro\n'
    if [[ "$server_role" == "tls-exit" ]]; then
      printf '      - /etc/letsencrypt:/etc/letsencrypt:ro\n'
    fi
    printf '    command: ["run", "-config", "/etc/xray/config.json"]\n'
    printf '    ports:\n'
    if [[ "$server_role" == "basic-exit" ]]; then
      printf '      - "%s:%s/tcp"\n' "$HTTP_PORT" "$HTTP_PORT"
      printf '      - "%s:%s/tcp"\n' "$SOCKS_PORT" "$SOCKS_PORT"
      printf '      - "%s:%s/udp"\n' "$SOCKS_PORT" "$SOCKS_PORT"
      printf '      - "%s:%s/tcp"\n' "$VLESS_PORT" "$VLESS_PORT"
      printf '      - "%s:%s/tcp"\n' "$SHADOWSOCKS_PORT" "$SHADOWSOCKS_PORT"
      printf '      - "%s:%s/udp"\n' "$SHADOWSOCKS_PORT" "$SHADOWSOCKS_PORT"
    else
      printf '      - "%s:%s/tcp"\n' "$TROJAN_PORT" "$TROJAN_PORT"
      printf '  hysteria:\n'
      printf '    image: %s\n' "$HYSTERIA_IMAGE"
      printf '    container_name: %s\n' "$hysteria_container"
      printf '    restart: unless-stopped\n'
      printf '    volumes:\n'
      printf '      - ./hysteria.yaml:/etc/hysteria/config.yaml:ro\n'
      printf '      - /etc/letsencrypt:/etc/letsencrypt:ro\n'
      printf '    command: ["server", "-c", "/etc/hysteria/config.yaml"]\n'
      printf '    ports:\n'
      printf '      - "%s:%s/udp"\n' "$HY2_PORT" "$HY2_PORT"
    fi
  } > "$output"
  chmod 600 "$output"
}

write_payload() {
  local server_role="$1"
  local payload_dir="$2"
  mkdir -p "$payload_dir"
  write_xray_config "${payload_dir}/xray.json" "$server_role"
  write_compose "${payload_dir}/docker-compose.yml" "$server_role"
  printf 'xrayc-real-protocol-upstream-lab\n' > "${payload_dir}/.xrayc-real-protocol-upstream-lab"
  if [[ "$server_role" == "tls-exit" ]]; then
    write_hysteria_config "${payload_dir}/hysteria.yaml"
  fi
}

write_env_output() {
  local basic_exit_ip="$1"
  local tls_exit_ip="$2"
  python3 - "$ENV_VALUES_JSON" "$ENV_OUT" "$basic_exit_ip" "$tls_exit_ip" <<'PY'
import json
import os
import shlex
import sys

values_file, output, basic_exit_ip, tls_exit_ip = sys.argv[1:5]
with open(values_file, "r", encoding="utf-8") as fh:
    values = json.load(fh)

env = {
    "THIRD_PARTY_SOCKS_HOST": values["SOCKS_HOST"],
    "THIRD_PARTY_SOCKS_PORT": values["SOCKS_PORT"],
    "THIRD_PARTY_SOCKS_USERNAME": values["SOCKS_USERNAME"],
    "THIRD_PARTY_SOCKS_PASSWORD": values["SOCKS_PASSWORD"],
    "THIRD_PARTY_SOCKS_KEY": values["SOCKS_PASSWORD"],
    "THIRD_PARTY_SOCKS_RAW_URL": values["SOCKS_RAW_URL"],
    "THIRD_PARTY_HTTP_HOST": values["HTTP_HOST"],
    "THIRD_PARTY_HTTP_PORT": values["HTTP_PORT"],
    "THIRD_PARTY_HTTP_USERNAME": values["HTTP_USERNAME"],
    "THIRD_PARTY_HTTP_PASSWORD": values["HTTP_PASSWORD"],
    "THIRD_PARTY_HTTP_KEY": values["HTTP_PASSWORD"],
    "THIRD_PARTY_HTTP_RAW_URL": values["HTTP_RAW_URL"],
    "THIRD_PARTY_HOST": values["VLESS_HOST"],
    "THIRD_PARTY_VLESS_HOST": values["VLESS_HOST"],
    "THIRD_PARTY_VLESS_PORT": values["VLESS_PORT"],
    "THIRD_PARTY_UUID": values["VLESS_UUID"],
    "THIRD_PARTY_PUBLIC_KEY": f"xrayc-real-vless-none-{values['VLESS_UUID']}",
    "THIRD_PARTY_VLESS_PUBLIC_KEY": f"xrayc-real-vless-none-{values['VLESS_UUID']}",
    "THIRD_PARTY_VLESS_RAW_URL": values["VLESS_RAW_URL"],
    "THIRD_PARTY_TROJAN_HOST": values["TROJAN_HOST"],
    "THIRD_PARTY_TROJAN_PORT": values["TROJAN_PORT"],
    "THIRD_PARTY_TROJAN_PASSWORD": values["TROJAN_PASSWORD"],
    "THIRD_PARTY_TROJAN_KEY": values["TROJAN_PASSWORD"],
    "THIRD_PARTY_TROJAN_SECURITY": values["TROJAN_SECURITY"],
    "THIRD_PARTY_TROJAN_SNI": values["TROJAN_SERVER_NAME"],
    "THIRD_PARTY_TROJAN_SERVER_NAME": values["TROJAN_SERVER_NAME"],
    "THIRD_PARTY_TROJAN_RAW_URL": values["TROJAN_RAW_URL"],
    "THIRD_PARTY_SHADOWSOCKS_HOST": values["SHADOWSOCKS_HOST"],
    "THIRD_PARTY_SHADOWSOCKS_PORT": values["SHADOWSOCKS_PORT"],
    "THIRD_PARTY_SHADOWSOCKS_METHOD": values["SHADOWSOCKS_METHOD"],
    "THIRD_PARTY_SHADOWSOCKS_PASSWORD": values["SHADOWSOCKS_PASSWORD"],
    "THIRD_PARTY_SHADOWSOCKS_KEY": values["SHADOWSOCKS_PASSWORD"],
    "THIRD_PARTY_SHADOWSOCKS_RAW_URL": values["SHADOWSOCKS_RAW_URL"],
    "THIRD_PARTY_HY2_HOST": values["HY2_HOST"],
    "THIRD_PARTY_HY2_PORT": values["HY2_PORT"],
    "THIRD_PARTY_HY2_PASSWORD": values["HY2_PASSWORD"],
    "THIRD_PARTY_HY2_KEY": values["HY2_PASSWORD"],
    "THIRD_PARTY_HY2_SERVER_NAME": values["HY2_SERVER_NAME"],
    "THIRD_PARTY_HY2_RAW_URL": values["HY2_RAW_URL"],
}
if basic_exit_ip:
    env["EXPECTED_EXIT_IP_SOCKS"] = basic_exit_ip
    env["EXPECTED_EXIT_IP_HTTP"] = basic_exit_ip
    env["EXPECTED_EXIT_IP_VLESS"] = basic_exit_ip
    env["EXPECTED_EXIT_IP_SHADOWSOCKS"] = basic_exit_ip
if tls_exit_ip:
    env["EXPECTED_EXIT_IP_TROJAN"] = tls_exit_ip
    env["EXPECTED_EXIT_IP_HY2"] = tls_exit_ip

with open(output, "w", encoding="utf-8") as fh:
    fh.write("# Private real protocol upstream lab env fragment.\n")
    fh.write("# Generated by scripts/real-protocol-upstream-lab.sh.\n")
    fh.write("# Do not print or commit this file.\n")
    for key in sorted(env):
        fh.write(f"{key}={shlex.quote(str(env[key]))}\n")
os.chmod(output, 0o600)
PY
}
