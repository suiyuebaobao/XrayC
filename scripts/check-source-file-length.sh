#!/usr/bin/env bash
# 用途：检查源码文件长度是否超过团队规则。
# 范围：Rust、前端源码、E2E、Shell 脚本和 SQL 迁移。
# 说明：文档、OpenAPI 导出和包锁文件不属于代码长度门禁。
# 输出：只打印超限文件路径和行数，不打印文件内容。
# 规则：默认单个源码文件基准 500 行，允许 10% 弹性，上限 550 行。
# 配置：可通过 MAX_SOURCE_FILE_LINES 覆盖阈值。
# 依赖：find、wc、awk，避免引入额外运行时。
# 安全：不会修改任何文件。
# 失败：发现超限文件时返回非零。
# 维护：新增源码目录时同步 SEARCH_DIRS。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAX_LINES="${MAX_SOURCE_FILE_LINES:-550}"
SEARCH_DIRS=(
  "$ROOT_DIR/crates"
  "$ROOT_DIR/frontend/src"
  "$ROOT_DIR/frontend/e2e"
  "$ROOT_DIR/scripts"
  "$ROOT_DIR/migrations"
)

if [[ ! "$MAX_LINES" =~ ^[1-9][0-9]*$ ]]; then
  echo "check-source-file-length: MAX_SOURCE_FILE_LINES must be positive" >&2
  exit 2
fi

failures=0
while IFS= read -r -d '' file; do
  lines="$(wc -l <"$file" | awk '{print $1}')"
  if [[ "$lines" -gt "$MAX_LINES" ]]; then
    rel="${file#$ROOT_DIR/}"
    echo "check-source-file-length: ${rel} has ${lines} lines, max ${MAX_LINES}" >&2
    failures=$((failures + 1))
  fi
done < <(
  find "${SEARCH_DIRS[@]}" -type f \
    \( -name '*.rs' -o -name '*.ts' -o -name '*.vue' -o -name '*.js' -o -name '*.sh' -o -name '*.sql' \) \
    -print0
)

if [[ "$failures" -gt 0 ]]; then
  exit 1
fi

echo "check-source-file-length: passed"
