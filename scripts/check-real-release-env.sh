#!/usr/bin/env bash
# 用途：执行真实发布环境的本地无密钥门禁，验证配置完整性。
# 范围：校验私有 env、真实协议矩阵环境和变量列表同步状态。
# 输入：默认读取 .env.real-release，可由 XRAYC_REAL_RELEASE_ENV_FILE 覆盖。
# 输出：仅输出门禁结果和缺失变量名，不输出任何私有变量值。
# 依赖：调用 validate-real-release-env.sh、check-real-protocol-matrix-env.sh。
# 安全：不执行 SSH、部署、Playwright、稳定性或负载类真实动作。
# 约束：协议集合固定由 REAL_RELEASE_PROTOCOLS 传入矩阵环境检查。
# 行为：命令行参数只允许帮助选项，其他参数视为用法错误。
# 失败：任一本地校验失败或同步漂移都会返回非零退出码。
# 维护：真实发布必需变量变化时需同步 prepare 与 gap-report 覆盖。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
REAL_RELEASE_PROTOCOLS="socks,http,vless,trojan,shadowsocks,hy2"

usage() {
  cat <<'EOF'
usage: bash scripts/check-real-release-env.sh

Runs the local, no-secret real-release environment gate. It validates the
private env file, checks the full real protocol matrix, and verifies that the
gate/template/gap-report variable lists stay synchronized. It does not run
remote SSH, deploy, Playwright, smoke, stability, or loadtest actions.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

bash scripts/validate-real-release-env.sh
REAL_PROTOCOL_MATRIX_PROTOCOLS="$REAL_RELEASE_PROTOCOLS" \
bash scripts/check-real-protocol-matrix-env.sh --env-file "$REAL_RELEASE_ENV_FILE"

required_output="$(bash scripts/check-real-release.sh --print-required-env)"
mapfile -t required_names < <(
  printf '%s\n' "$required_output" \
    | grep -Eo '[A-Z][A-Z0-9_]*' \
    | grep -E '_' \
    | sort -u
)

paths=(
  "scripts/prepare-real-release-env.sh"
  "scripts/real-release-gap-report.sh"
)

failures=0
for name in "${required_names[@]}"; do
  for path in "${paths[@]}"; do
    if ! grep -Fq "$name" "$path"; then
      printf 'real-release-env: %s is missing from %s\n' "$name" "$path" >&2
      failures=$((failures + 1))
    fi
  done
done

if [[ "$failures" -gt 0 ]]; then
  printf 'real-release-env: local sync failed with %s issue(s)\n' "$failures" >&2
  exit 2
fi

echo "real-release-env: local gate passed"
