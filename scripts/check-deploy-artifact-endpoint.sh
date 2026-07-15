#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# 校验控制面公开的部署产物是否等于本机刚生成的部署产物。
# 该脚本只比较 manifest 和 sha256 清单，不打印真实域名、Token 或下载地址。

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
fi

BASE_URL="${BASE_URL:-${XRAYC_CONTROL_PLANE_URL:-}}"
TOKEN="${DEPLOY_ARTIFACT_TOKEN:-${XRAYC_DEPLOY_ARTIFACT_TOKEN:-}}"
ARTIFACT_DIR="${DEPLOY_ARTIFACT_DIR_LOCAL:-deploy/artifacts}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

die() {
  printf 'deploy-artifact-endpoint-check: %s\n' "$1" >&2
  exit 1
}

[[ -n "$BASE_URL" ]] || die "BASE_URL is required"
[[ -n "$TOKEN" ]] || die "DEPLOY_ARTIFACT_TOKEN is required"
[[ -f "${ARTIFACT_DIR}/access-agent-manifest.json" ]] || die "local manifest is missing"
[[ -f "${ARTIFACT_DIR}/access-agent.sha256" ]] || die "local checksum file is missing"

curl_config="${TMP_DIR}/curl.conf"
printf 'header = "Authorization: Bearer %s"\n' "$TOKEN" >"$curl_config"

fetch_artifact() {
  local name="$1"
  local output="$2"
  curl --fail --silent --show-error --location --max-time 120 \
    --config "$curl_config" \
    --output "$output" \
    "${BASE_URL%/}/api/deploy/artifacts/${name}" >/dev/null
}

fetch_artifact access-agent-manifest.json "${TMP_DIR}/access-agent-manifest.json"
fetch_artifact access-agent.sha256 "${TMP_DIR}/access-agent.sha256"

cmp -s "${ARTIFACT_DIR}/access-agent-manifest.json" "${TMP_DIR}/access-agent-manifest.json" \
  || die "remote manifest does not match local artifact manifest"

cmp -s "${ARTIFACT_DIR}/access-agent.sha256" "${TMP_DIR}/access-agent.sha256" \
  || die "remote checksum file does not match local artifact checksum"

grep -Eq 'access-agent-manifest\.json' "${TMP_DIR}/access-agent.sha256" \
  || die "remote checksum file does not include manifest"

echo "deploy-artifact-endpoint-check: passed"
