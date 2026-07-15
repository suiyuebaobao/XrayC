#!/usr/bin/env bash
# 用途：加载真实第三方协议矩阵中继 E2E 的拆分函数库。
# 本文件保持入口脚本 source 兼容，不再直接承载具体函数实现。
# 通用工具、API、协议校验、远端执行、断言和客户端逻辑拆到同目录 helper。
# 调试输出仍由各 helper 保持脱敏，避免泄露私有 endpoint 或凭据。

real_matrix_relay_lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/common.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/api.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/protocols.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/remote.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/client.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/assertions.sh"
# shellcheck disable=SC1091
. "$real_matrix_relay_lib_dir/subscription-redaction.sh"

unset real_matrix_relay_lib_dir
