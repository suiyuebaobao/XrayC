#!/usr/bin/env bash
# 用途：扫描运行时代码中遗留 node-agent、限速和 quota-pool 关键词。
# 范围：覆盖核心运行目录，跳过依赖、构建产物、迁移和文档目录。
# 输入：读取工作树中的文本文件，并按白名单判断是否属于运行文件。
# 输出：仅输出规则名、相对路径和行号，避免打印命中内容。
# 依赖：使用 find、grep 和临时文件完成二进制跳过与匹配汇总。
# 安全：不展示可能含有敏感配置的原始匹配行，只报告定位信息。
# 约束：本脚本自身命中会被跳过，避免规则文本造成自检失败。
# 行为：所有遗留命中都会累计为失败，扫描结束后统一退出。
# 失败：任一受管文件出现禁止关键词时返回非零用于 CI 阻断。
# 维护：迁移完成后如有新遗留术语，应追加到对应规则中。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SELF_PATH="$ROOT_DIR/scripts/check-no-legacy.sh"
MATCH_FILE="$(mktemp)"
failures=0

cleanup() {
  rm -f "$MATCH_FILE"
}
trap cleanup EXIT

report() {
  local rule="$1"
  local rel="$2"
  local line="$3"

  printf 'check-no-legacy: %s in %s:%s\n' "$rule" "$rel" "$line" >&2
  failures=$((failures + 1))
}

print_redacted_matches() {
  local rule="$1"
  local rel="$2"

  while IFS=: read -r line _; do
    report "$rule" "$rel" "$line"
  done <"$MATCH_FILE"
}

is_runtime_file() {
  local rel="$1"

  case "$rel" in
    crates/*|frontend/src/*|frontend/e2e/*|scripts/*|deploy/*) return 0 ;;
    Dockerfile|docker-compose.yml|Makefile|Cargo.toml|Cargo.lock) return 0 ;;
    frontend/vite.config.ts|frontend/playwright.config.ts|frontend/package.json|frontend/package-lock.json) return 0 ;;
    *) return 1 ;;
  esac
}

scan_content() {
  local file="$1"
  local rel="${file#$ROOT_DIR/}"

  [[ "$file" == "$SELF_PATH" ]] && return 0
  is_runtime_file "$rel" || return 0
  grep -Iq . "$file" || return 0

  if grep -En -- '(^|[^A-Za-z0-9_])(node-agent|node_agent|NodeAgent)([^A-Za-z0-9_]|$)' "$file" >"$MATCH_FILE" 2>/dev/null; then
    print_redacted_matches "legacy node agent keyword" "$rel"
  fi

  legacy_rate_terms="$(printf '(%s|%s|%s|%s|%s|%s|%s)' \
    "Speed""Limit" \
    "speed""_limit" \
    "User""Speed""Limit""Channel" \
    "dedicated""_port" \
    "rp""_limit" \
    "rate""_limit""_profiles" \
    "user""_rate""_limit""_assignments" \
    "tcp""_rate""_limit""_bps" \
    "udp""_rate""_limit""_bps")"
  if grep -En -- "(^|[^A-Za-z0-9_])${legacy_rate_terms}([^A-Za-z0-9_]|$)" "$file" >"$MATCH_FILE" 2>/dev/null; then
    print_redacted_matches "legacy user speed-limit keyword" "$rel"
  fi

  quota_field_terms="$(printf '(%s|%s|%s)' "traffic""Pool" "traffic""_pool" "Traffic""Pool")"
  if grep -En -- "(^|[^A-Za-z0-9_])${quota_field_terms}([^A-Za-z0-9_]|$)" "$file" >"$MATCH_FILE" 2>/dev/null; then
    print_redacted_matches "legacy quota-pool field keyword" "$rel"
  fi
}

find_args=(
  -path "$ROOT_DIR/.git" -prune -o
  -path "$ROOT_DIR/node_modules" -prune -o
  -path "$ROOT_DIR/target" -prune -o
  -path "$ROOT_DIR/dist" -prune -o
  -path "$ROOT_DIR/build" -prune -o
  -path "$ROOT_DIR/migrations" -prune -o
  -path "$ROOT_DIR/docs" -prune -o
  -path "$ROOT_DIR/文档" -prune -o
  -path "$ROOT_DIR/frontend/node_modules" -prune -o
  -path "$ROOT_DIR/frontend/dist" -prune -o
  -path "$ROOT_DIR/frontend/build" -prune -o
)

while IFS= read -r -d '' dir; do
  rel="${dir#$ROOT_DIR/}"
  report "legacy node agent directory" "$rel" "-"
done < <(find "$ROOT_DIR" "${find_args[@]}" \( -type d \( -name node-agent -o -name node_agent -o -name nodeagent \) \) -print0)

while IFS= read -r -d '' file; do
  rel="${file#$ROOT_DIR/}"
  base="$(basename "$file")"

  case "$base" in
    go.mod|go.sum|*.go) report "legacy Go file" "$rel" "-" ;;
  esac

  scan_content "$file"
done < <(find "$ROOT_DIR" "${find_args[@]}" -type f -print0)

if [[ "$failures" -gt 0 ]]; then
  printf 'check-no-legacy: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi

bash "$ROOT_DIR/scripts/check-docs-no-stale.sh"

echo "check-no-legacy: passed"
