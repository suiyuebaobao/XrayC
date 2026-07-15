#!/usr/bin/env bash
# 中文说明：订阅客户端兼容门禁的本地静态回归测试，不访问真实订阅。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cat >"$tmp_dir/subscription.yaml" <<'YAML'
mixed-port: 7890
allow-lan: false
mode: rule
log-level: info
proxies:
  - name: vless-reality-fixture
    type: vless
    server: example.com
    port: 443
    uuid: 00000000-0000-0000-0000-000000000001
    network: tcp
    tls: true
    udp: true
    servername: www.cloudflare.com
    client-fingerprint: chrome
    reality-opts:
      public-key: fixture-public-key
      short-id: fixture-short-id
proxy-groups:
  - name: fixture-group
    type: select
    proxies:
      - vless-reality-fixture
rules:
  - MATCH,fixture-group
YAML

bash -n scripts/check-real-subscription-client-compat.sh
bash -n scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "prepare_compat_subscription_if_needed" scripts/check-real-subscription-client-compat.sh
grep -Fq "real-access-inbound-matrix-db-prepare.sh prepare" scripts/check-real-subscription-client-compat.sh
grep -Fq "cleanup_compat_subscription_prepare" scripts/check-real-subscription-client-compat.sh
grep -Fq 'FETCH_STATUS" == "422"' scripts/check-real-subscription-client-compat.sh
grep -Fq "REAL_SUBSCRIPTION_CLIENT_COMPAT_KEEP_PREPARE" scripts/check-real-subscription-client-compat.sh
grep -Fq "REAL_SUBSCRIPTION_CLIENT_COMPAT_PREPARE_MANIFEST" scripts/check-real-subscription-client-compat.sh
grep -Fq "RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC" scripts/check-real-subscription-client-compat.sh
grep -Fq "lib/real-subscription-client-compat/traffic.sh" scripts/check-real-subscription-client-compat.sh
grep -Fq "run_mihomo_or_clash_traffic" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "run_remote_subscription_client_traffic_checks" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "XRAYC_REAL_SUBSCRIPTION_CLIENT_TRAFFIC_TARGET" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq 'profile["allow-lan"] = False' scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq 'profile["bind-address"] = "127.0.0.1"' scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq 'profile["mixed-port"] = listen_port' scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "remote_select_free_subscription_client_ports" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "stage_remote_subscription_client_configs" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "remote subscription traffic setup failed; raw output redacted" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "remote_subscription_client_cleanup" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "remote_status=0" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq 'return "$remote_status"' scripts/lib/real-subscription-client-compat/traffic.sh
! grep -Fq 'profile["allow-lan"] = True' scripts/lib/real-subscription-client-compat/traffic.sh
! grep -Fq 'profile["bind-address"] = "*"' scripts/lib/real-subscription-client-compat/traffic.sh
! grep -Fq 'profile["bind-address"] = "0.0.0.0"' scripts/lib/real-subscription-client-compat/traffic.sh
! grep -Fq 'docker logs "$container"' scripts/lib/real-subscription-client-compat/traffic.sh
! grep -Fq "trap remote_subscription_client_cleanup RETURN" scripts/lib/real-subscription-client-compat/traffic.sh
grep -Fq "compat_prepare_manifest" scripts/check-real-release.sh
grep -Fq "RUN_REAL_SUBSCRIPTION_CLIENT_TRAFFIC" scripts/check-real-release.sh
echo "test-real-subscription-client-compat-static: passed"
