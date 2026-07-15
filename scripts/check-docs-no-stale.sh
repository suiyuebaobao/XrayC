#!/usr/bin/env bash
# 用途：扫描公开文档中的旧产品口径，避免专属订阅、二级分组、一键远端部署等已删除设计回流(本地 install.sh 一键部署是现行特性,不在此列)。
# 输出：只报告规则名、相对路径和行号，不打印命中原文。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

  printf 'check-docs-no-stale: %s in %s:%s\n' "$rule" "$rel" "$line" >&2
  failures=$((failures + 1))
}

print_redacted_matches() {
  local rule="$1"
  local rel="$2"

  while IFS=: read -r line _; do
    report "$rule" "$rel" "$line"
  done <"$MATCH_FILE"
}

scan_rule() {
  local file="$1"
  local rel="$2"
  local rule="$3"
  local pattern="$4"

  if grep -Eni -- "$pattern" "$file" >"$MATCH_FILE" 2>/dev/null; then
    print_redacted_matches "$rule" "$rel"
  fi
}

scan_file() {
  local file="$1"
  local rel="${file#$ROOT_DIR/}"

  grep -Iq . "$file" || return 0

  scan_rule "$file" "$rel" "removed subscription/device model" '专属订阅|客户端设备|/api/client(/|[^sA-Za-z0-9_]|$)|client_devices|device-bound|client-specific subscription|profile subscription|设备绑定|设备订阅'
  scan_rule "$file" "$rel" "removed dedicated line wording" '专属线路|单客户线路|客户线路|专用线路|客户转'
  scan_rule "$file" "$rel" "removed local-direct wording" '本机直出|本机直连|local_direct|direct/local_direct|local-direct-exit|outbound_type=direct|ownership=local_direct|历史 direct|旧 direct|legacy direct|direct/remote-deploy|direct 模式|direct is legacy'
  scan_rule "$file" "$rel" "removed line-pool product wording" '线路池'
  scan_rule "$file" "$rel" "removed sing-box/AnyTLS dual-kernel wording" 'sing.box|sing_box|singbox|anytls|any_tls|runtime_core|两个内核|双内核|双 runtime core'
  scan_rule "$file" "$rel" "removed grouping/load wording" '二级分组|二级用户节点|二级组|多层分组|weighted_sticky|粘性会话|会话保持|会话固定|sticky session|secondary group|dedicated client line|load balancing|负载池|负载均衡|权重分摊|自动分摊|自动分散|稳定分散|刷新用户可用入口|刷新出口分配|刷新入口授权|refresh allocation|refresh available|初始入口|初始中转|用户出口 rebalance|订阅入口 rebalance|错误分配[^[:cntrl:]]*rebalance|再 rebalance|不提交删除或 rebalance|remote-deploy'
  scan_rule "$file" "$rel" "removed deploy/frontend wording" 'n''ginx 服务|n''ginx 容器|n''ginx 配置|Caddy[[:space:]]*\+[[:space:]]*n''ginx|独立前端镜像|前端构建服务|docker compose build api frontend|api frontend|5 个服务|5个服务|5 个主服务|API、Worker、Frontend|一键远端部署|one-click remote deploy|后台 SSH 部署|远端 Agent 安装|远程 Agent 安装|remote-agent-install|remote-deploy|remote deployment|remote deploy|具体目标服务器地址'
  scan_rule "$file" "$rel" "bad local Reality wording" 'public_key.*不能为空|Reality public_key.*不能为空'
  scan_rule "$file" "$rel" "removed local-exit limited protocol wording" '本机出口服务[^[:cntrl:]]*(只支持|仅支持|支持选择)[^[:cntrl:]]*SOCKS5/HTTP|可复用的 SOCKS5/HTTP 出口服务|本机 SOCKS5/HTTP inbound|`socks/http` endpoint'
  scan_rule "$file" "$rel" "removed local-exit auto-entry wording" '本机出口服务[^[:cntrl:]]*可选同步创建客户端入口|本机出口服务[^[:cntrl:]]*同步创建客户端入口|本机出口服务[^[:cntrl:]]*自动创建客户端入口'
  scan_rule "$file" "$rel" "removed local-exit runtime-collection wording" '本机出口服务[^[:cntrl:]]*内部运行集合|endpoint 进入出口管理/内部运行集合'
  scan_rule "$file" "$rel" "removed subscription icon wording" 'lineGroupIcon|节点分组由后台套餐授权控制|匹配当前产品文案[^[:cntrl:]]*节点分组|当前产品文案“节点分组”|订阅[^[:cntrl:]]*(应|会|将|可以|支持)[^[:cntrl:]]*(输出|返回|展示)[^[:cntrl:]]*(line_group_icon|proxies\[\]\.icon|proxy-groups\[\]\.icon|图标|emoji|flag)|可用节点[^[:cntrl:]]*(应|会|将|可以|支持)[^[:cntrl:]]*(输出|返回|展示)[^[:cntrl:]]*(line_group_icon|图标|emoji|flag)'
  scan_rule "$file" "$rel" "bad agent auth-code wording" 'agent_token.*可选|服务端生成.*agent_token|agent_token.*generated when omitted|agent_token.*optional'
  scan_rule "$file" "$rel" "removed routing strategy wording" 'hash/stable_hash|stable_hash|weighted_round_robin|round_robin|group-owned exit pool|分组运行兼容集合|分组自动同步运行集合|绑定同一分组生成|出口管理[[:space:]]*->[[:space:]]*运行集合[[:space:]]*->[[:space:]]*中转节点|中转节点[[:space:]]*\+[[:space:]]*出口线路[[:space:]]*\+[[:space:]]*运行集合|绑定运行集合|选择运行集合|运行集合[^[:cntrl:]]*加权分配|运行集合[^[:cntrl:]]*允许新分配|运行集合[^[:cntrl:]]*可重复映射'
  removed_group_entry_pattern="$(printf '%s|%s|%s|%s|%s|%s|%s|%s' \
    "绑定""线路入口" \
    "绑定""分组入口" \
    "中转节点绑定""分组" \
    "按分组生成""入口" \
    "绑定同一分组""生成" \
    "group ""entry ""binding" \
    "line-entries[^[:cntrl:]]*line""_group_id" \
    "line""_group_id[^[:cntrl:]]*line-entries")"
  scan_rule "$file" "$rel" "removed line-endpoint binding wording" "$removed_group_entry_pattern"
  scan_rule "$file" "$rel" "old fixed test-server role wording" '固定为 1 台|1 台私有真实用户客户端|server_1.*客户端|server_2.*中转|server_3/server_4.*出口|(^|[^A-Za-z0-9_])server_N([^A-Za-z0-9_]|$)|固定 4 台|固定4台|四机|四台|4 个节点|4个节点|4 台测试|4台测试|transit-server-1|当日私有清单中的测试服务器|当日私有清单中的真实测试|出口资产机|私有客户端服务器|私有中转服务器|第[[:space:]]*4[[:space:]]*行域名'
  # 限速口径已整体改为「按每用户 Xray mark 区分 + 出口侧 fwmark/connmark+IFB 整形」，拦截旧的「按运行时端口/单用户 runtime 入口」措辞回流（见 开发方案.md §7.7 / §2.9.1）。
  scan_rule "$file" "$rel" "removed per-port rate-limit wording" '运行时端口|运行时入口端口|运行时单用户|runtime 端口|runtime 入口端口|单用户 runtime|runtime 单用户|按端口限速|20000-60999|20000–60999'
}

doc_paths=(
  "$ROOT_DIR/README.md"
  "$ROOT_DIR/AGENTS.md"
  "$ROOT_DIR/CHANGELOG.md"
  "$ROOT_DIR/开发方案.md"
  "$ROOT_DIR/.env.real-release.example"
  "$ROOT_DIR/deploy/access-agent/access-agent.env.example"
  "$ROOT_DIR/frontend/src/views/AdminTutorialPage.vue"
)

for path in "${doc_paths[@]}"; do
  [[ -f "$path" ]] && scan_file "$path"
done

while IFS= read -r -d '' file; do
  scan_file "$file"
done < <(
  find "$ROOT_DIR/docs" "$ROOT_DIR/文档" \
    -path "$ROOT_DIR/文档/私有" -prune -o \
    -path "$ROOT_DIR/文档/私有/*" -prune -o \
    -path "$ROOT_DIR/文档/每日记录" -prune -o \
    -path "$ROOT_DIR/文档/每日记录/*" -prune -o \
    -path "$ROOT_DIR/docs/superpowers" -prune -o \
    -path "$ROOT_DIR/docs/superpowers/*" -prune -o \
    -type f -print0
)

while IFS= read -r -d '' file; do
  rel="${file#$ROOT_DIR/}"
  grep -Iq . "$file" || continue
  scan_rule "$file" "$rel" "removed group-entry binding wording" '绑定分组入口|中转节点绑定分组|按分组生成入口|group entry binding|line-entries[^[:cntrl:]]*line_group_id|line_group_id[^[:cntrl:]]*line-entries'
done < <(
  find "$ROOT_DIR/crates/api/src" "$ROOT_DIR/crates/db/src/store" \
    -type f \( -name '*.rs' -o -name '*.sql' \) -print0
)

while IFS= read -r -d '' file; do
  [[ "$file" == "$ROOT_DIR/scripts/check-docs-no-stale.sh" ]] && continue
  rel="${file#$ROOT_DIR/}"
  grep -Iq . "$file" || continue
  scan_rule "$file" "$rel" "removed runtime strategy implementation" 'COALESCE\(l\.line_group_id|weighted_round_robin|exit_pool_strategy=hash|reapply_round_robin_exit_assignments|ensure_hash_exit_pool_strategy'
done < <(
  find "$ROOT_DIR/scripts" -type f \( -name '*.sh' -o -name '*.bash' \) -print0
)

if [[ "$failures" -gt 0 ]]; then
  printf 'check-docs-no-stale: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi

echo "check-docs-no-stale: passed"
