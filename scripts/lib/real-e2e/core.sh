#!/usr/bin/env bash
# 用途：提供 real e2e 脚本通用布尔值和环境变量校验函数。
# 这些函数由 scripts/real-e2e-lib.sh 统一加载，供旧调用方继续 source 使用。
set -euo pipefail

xrayc_real_e2e_bool_is_true() {
  case "${1:-}" in
    true|TRUE|1|yes|YES|on|ON)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

xrayc_real_e2e_require_env() {
  local name="$1"
  local context="${2:-real e2e}"

  if [[ -z "${!name:-}" ]]; then
    echo "${name} is required for ${context}" >&2
    exit 2
  fi
}

xrayc_real_e2e_require_any_env() {
  local label="$1"
  local context="$2"
  local name=""
  local found=0
  shift 2

  for name in "$@"; do
    if [[ -n "${!name:-}" ]]; then
      found=1
      break
    fi
  done

  if [[ "$found" -ne 1 ]]; then
    echo "${label} is required for ${context}" >&2
    exit 2
  fi
}

xrayc_real_e2e_require_url_scheme_if_set() {
  local name="$1"
  local value="${!name:-}"

  [[ -n "$value" ]] || return 0
  if [[ ! "$value" =~ ^[A-Za-z][A-Za-z0-9+.-]*:// ]]; then
    echo "${name} must include a URL scheme" >&2
    exit 2
  fi
}

xrayc_real_e2e_require_url_scheme() {
  local name="$1"
  local context="${2:-real e2e}"

  xrayc_real_e2e_require_env "$name" "$context"
  xrayc_real_e2e_require_url_scheme_if_set "$name"
}
