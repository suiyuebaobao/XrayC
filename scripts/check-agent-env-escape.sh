#!/usr/bin/env bash
# 用途：回归守卫 access-agent.env 注入的 xray-test 命令在 compose v1/v2 两侧都收敛。
# 范围：只 source deploy-access-agent 的 common.sh 取纯函数，不构建/不部署/不访问外部服务。
# 背景：
#   BUG-C——env_escape 旧实现把 $→$$，compose v1 env_file 不反转义，容器内字面含 $$，
#       agent `sh -c` 把 $$ 展开成 PID → xray -test 的 -config 路径变垃圾、每心跳校验必败。
#   BUG-E——含运行时 $VAR 的命令型值经 env_file 注入，compose v1/v2 语义不一致：
#       v1 不插值（保留单 $，正确）；v2 会插值（此刻 $XRAYC_XRAY_CONFIG 未定义）→ -config "" →
#       xray-test 报 `flag needs an argument: -config` → agent 退回空配置 → 永不收敛、443 不监听。
#       任何单一转义都无法 v1/v2 两边都对，故根除依赖：部署脚本默认不再把 XRAYC_XRAY_TEST_COMMAND
#       写进 env_file，改由 agent config.rs 自带默认、由 agent 自己 sh -c 展开（不经 compose 插值）。
# 断言：
#   A) 部署脚本默认 env_file 不再写 XRAYC_XRAY_TEST_COMMAND 行（彻底消除该类转义坑）。
#   B) 除 test 命令外，其余命令型 env 值（reload/start/stop/reclaim）字面不含运行时单 $，
#      故经 compose v2 插值也不会被插空/被污染。
#   C) agent 自带默认命令经 sh -c（agent 侧设置 XRAYC_XRAY_CONFIG）展开后 -config 为真实路径、不含 PID。
#   D) 仍守 BUG-C：write_env_line 对任何命令型值不得把 $ double 成 $$。
# 安全：只用临时文件与示例命令，不读私有 env、不打印真实凭据/地址。
# 失败：任一断言不成立即非零退出，纳入 make check 静态门禁防回归。
# 维护：env_escape / write_env_line / 命令注入方式 / agent 默认命令变化时同步本守卫。
# 约束：本文件保持单文件 <550 行硬上限。
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 只取纯函数：common.sh 在 source 时不执行部署动作，安全。
# shellcheck source=/dev/null
source "${ROOT_DIR}/scripts/lib/deploy-access-agent/common.sh"

deploy_script="${ROOT_DIR}/scripts/deploy-access-agent.sh"
agent_config_rs="${ROOT_DIR}/crates/access-agent/src/config.rs"

fail() {
  printf 'agent-env-escape-check: %s\n' "$1" >&2
  exit 1
}

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
fake_config="${tmp_dir}/config.json"
printf '{}\n' > "$fake_config"

# ── 断言 A：部署脚本默认不再无条件注入 XRAYC_XRAY_TEST_COMMAND ──
# 根除 BUG-E：该命令含运行时 $XRAYC_XRAY_CONFIG，经 env_file 在 compose v2 下会被插空
# （v1 保留、v2 插空），无单一转义两侧都对。部署脚本默认不写它，改由 agent 自带默认展开；
# 仅当运维显式设置 XRAYC_XRAY_TEST_COMMAND 时才在 if [[ -n ... ]] 守卫内透传。
# env_file 主块各行用 2 空格缩进；无条件注入即 2 空格缩进的 write_env_line。守卫内透传是 4 空格。
if grep -Eq '^  write_env_line[[:space:]]+XRAYC_XRAY_TEST_COMMAND([[:space:]]|$)' "$deploy_script"; then
  fail "部署脚本默认仍无条件把 XRAYC_XRAY_TEST_COMMAND 写进 env_file（BUG-E：compose v2 会把 \$XRAYC_XRAY_CONFIG 插空）"
fi
# 若仍透传该命令，必须被显式覆盖守卫包裹（缺省路径不得注入）。
if grep -Eq '^[[:space:]]*write_env_line[[:space:]]+XRAYC_XRAY_TEST_COMMAND([[:space:]]|$)' "$deploy_script"; then
  if ! grep -Eq '^[[:space:]]*if[[:space:]]+\[\[[[:space:]]+-n[[:space:]]+"\$\{XRAYC_XRAY_TEST_COMMAND:-\}"' "$deploy_script"; then
    fail "部署脚本透传 XRAYC_XRAY_TEST_COMMAND 时缺少显式覆盖守卫（默认路径不得注入该运行时 \$VAR 命令）"
  fi
fi

# ── 断言 B：其余命令型 env 值字面不含运行时单 $（compose v2 插值也安全）──
# 这些值在部署脚本期已用 ${xray_container} 等展开，不应再含运行时 $VAR。
xray_container="xrayc-xray-examplesuffix"
reclaim_path="/var/lib/xrayc/access-agent/reclaim-xray.sh"
declare -A command_values=(
  [XRAYC_XRAY_RELOAD_COMMAND]="curl --fail --silent --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/restart?t=0' >/dev/null"
  [XRAYC_XRAY_RECLAIM_COMMAND]="sh ${reclaim_path}"
  [XRAYC_XRAY_START_COMMAND]="curl --fail --silent --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/start' >/dev/null"
  [XRAYC_XRAY_STOP_COMMAND]="curl --fail --silent --unix-socket /var/run/docker.sock -X POST 'http://localhost/containers/${xray_container}/stop?t=10' >/dev/null"
)
for key in "${!command_values[@]}"; do
  line="$(write_env_line "$key" "${command_values[$key]}")"
  case "$line" in
    *'$'*) fail "${key} 的 env_file 行含运行时 \$（compose v2 会插值破坏）：${line}" ;;
  esac
done

# ── 断言 C：agent 自带默认命令经 sh -c 展开后 -config 为真实路径、不含 PID ──
# 模拟 config.rs:from_env 在缺省 XRAYC_XRAY_TEST_COMMAND 时的默认，及 run_config_command
# 用 sh -c 执行（agent 侧把 XRAYC_XRAY_CONFIG 设为真实配置路径，由 agent 自己的 shell 展开）。
agent_default_command='/usr/local/bin/xrayc-xray run -test -format json -config "$XRAYC_XRAY_CONFIG"'
got_config="$(XRAYC_XRAY_CONFIG="$fake_config" sh -c \
  "set -- ${agent_default_command}; while [ \$# -gt 0 ]; do if [ \"\$1\" = -config ]; then printf '%s' \"\$2\"; fi; shift; done")"
if [ "$got_config" != "$fake_config" ]; then
  fail "agent 默认 xray-test 的 -config 路径被破坏（期望真实路径，得到 '${got_config}'）"
fi
# 守住 agent 默认本身确实保留单 $（不被源码意外 double 成 $$ → PID）。
if grep -Eq 'config json|format json' "$agent_config_rs"; then
  if ! grep -Fq 'run -test -format json -config \"$XRAYC_XRAY_CONFIG\"' "$agent_config_rs"; then
    fail "agent config.rs 默认 xray-test 命令缺失或被改动（应保留单 \$XRAYC_XRAY_CONFIG 让 agent 自己展开）"
  fi
fi

# ── 断言 D：仍守 BUG-C——write_env_line 对命令型值不得把 $ double 成 $$ ──
# 即便将来有人重新引入含 $ 的命令，env_escape 也不得回退到 $→$$（compose v1 不反转义 → PID）。
double_probe="$(write_env_line XRAYC_PROBE_COMMAND 'echo "$XRAYC_XRAY_CONFIG"')"
case "$double_probe" in
  *'$$'*) fail "write_env_line 把 \$ double 成 \$\$（BUG-C 回归：compose v1 不反转义 → agent sh -c 展开成 PID）：${double_probe}" ;;
esac

echo "agent-env-escape-check: passed"
