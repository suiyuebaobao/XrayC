#!/usr/bin/env bash
# 用途：聚合 real e2e 公共函数，保持旧的 source 入口兼容。
# 本文件只负责加载拆分后的 helper，不直接执行测试流程。
set -euo pipefail

_xrayc_real_e2e_lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/core.sh"
source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/postgres.sh"
source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/http.sh"
source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/billing.sh"
source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/ledger.sh"
source "${_xrayc_real_e2e_lib_dir}/lib/real-e2e/subscription.sh"

unset _xrayc_real_e2e_lib_dir
