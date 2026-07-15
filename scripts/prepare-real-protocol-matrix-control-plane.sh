#!/usr/bin/env bash
# 用途：保留旧的真实协议矩阵控制面入口，兼容历史调用路径。
# 范围：本脚本不直接创建资源，只把参数转交给 endpoints 准备脚本。
# 输入：接受调用方传入的全部命令行参数，并原样透传给目标脚本。
# 输出：仅在标准错误输出委派提示，实际输出由目标脚本负责。
# 依赖：依赖同目录 prepare-real-protocol-matrix-endpoints.sh 存在且可执行。
# 安全：不读取、不打印、不拼接任何私有 token、密码或 endpoint 明文。
# 约束：不得在此处新增控制面逻辑，避免与新的 endpoints 入口分叉。
# 行为：使用 exec 替换当前进程，使退出码完全继承目标脚本结果。
# 失败：目标脚本缺失、不可执行或执行失败时，由 Bash 返回对应错误。
# 维护：后续协议矩阵控制面变更应落到 endpoints 脚本及其 lib 中。
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "prepare-real-protocol-matrix-control-plane: delegated to prepare-real-protocol-matrix-endpoints" >&2
exec "${SCRIPT_DIR}/prepare-real-protocol-matrix-endpoints.sh" "$@"
