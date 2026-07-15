#!/usr/bin/env bash
# 用途：验证真实认证 HA UAT 的强制变量优先于私有 env 文件。
# 约束：不启动 Docker、不访问网络、不打印账号密码或 token。
set -euo pipefail
IFS=$'\n\t'

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

env_file="$TMP_DIR/real-release.env"
cat >"$env_file" <<'ENV'
UAT_COMPOSE_START=0
UAT_COMPOSE_NO_BUILD=0
UAT_MIN_API_REPLICAS=2
UAT_VALIDATE_ONLY=0
ADMIN_LOGIN_ACCOUNT=admin@example.test
ADMIN_LOGIN_PASSWORD=admin123456
UAT_RATE_LIMIT_ATTEMPTS=35
CURL_TIMEOUT=15
ENV

output="$(
  XRAYC_REAL_RELEASE_ENV_FILE="$env_file" \
  XRAYC_AUTH_HA_FORCE_COMPOSE_START=1 \
  XRAYC_AUTH_HA_FORCE_COMPOSE_NO_BUILD=1 \
  XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY=1 \
  bash scripts/real-auth-ha-uat.sh
)"

if [[ "$output" != "auth HA UAT validation passed" ]]; then
  echo "expected force env to make auth HA UAT validate without current compose replicas" >&2
  exit 1
fi

echo "real-auth-ha-force-env: passed"
