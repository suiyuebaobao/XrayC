#!/usr/bin/env bash
# 用途：检查真实发布资产变量是否同步写入脚本和中文清单文档。
# 范围：只做本地静态一致性校验，不访问远端服务或真实基础设施。
# 输入：从 check-real-release.sh --print-required-env 提取必需环境变量名。
# 输出：报告缺失变量所在文件，并在全部覆盖时输出 passed。
# 依赖：依赖真实发布门禁脚本和资产准备清单文档保持可读。
# 安全：只匹配变量名，不读取或打印私有环境变量的真实取值。
# 约束：新增真实发布变量时，必须同步更新本脚本覆盖的路径集合。
# 行为：任一变量未在目标文件出现即累加失败并最终返回非零。
# 失败：用于 CI 或本地门禁快速发现文档、模板、脚本列表漂移。
# 维护：若资产文档迁移路径，应同时调整 paths 数组。
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

required_output="$(bash scripts/check-real-release.sh --print-required-env)"
mapfile -t required_names < <(
  printf '%s\n' "$required_output" \
    | grep -Eo '[A-Z][A-Z0-9_]*' \
    | grep -E '_' \
    | sort -u
)

paths=(
  ".env.real-release.example"
  "scripts/prepare-real-release-env.sh"
  "scripts/lib/validate-real-release-env/rules.sh"
  "scripts/real-release-gap-report.sh"
  "文档/测试/真实发布资产准备清单.md"
)

failures=0
for name in "${required_names[@]}"; do
  for path in "${paths[@]}"; do
    if ! grep -Fq "$name" "$path"; then
      printf 'check-real-release-assets-docs: missing %s in %s\n' "$name" "$path" >&2
      failures=$((failures + 1))
    fi
  done
done

if [[ "$failures" -gt 0 ]]; then
  printf 'check-real-release-assets-docs: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi

echo "check-real-release-assets-docs: passed"
