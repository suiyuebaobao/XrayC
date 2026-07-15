#!/usr/bin/env bash
# 用途：验证真实 V2 主链路把临时分组授权给测试用户当前套餐。
# 范围：静态检查脚本 SQL，不访问真实数据库或私有环境。
# 失败：绑定默认套餐会导致非默认套餐测试用户拉不到刚创建的线路。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

script="scripts/real-v2-relay-pool-e2e.sh"
grep -Fq 'SELECT plan_id::text FROM user_subscriptions WHERE user_id' "$script"
grep -Fq -- "-v plan_id=\"\$user_plan_id\"" "$script"
grep -Fq '"name": f"real-e2e-default-{rid}"' "$script"
grep -Fq '"sort_weight": -100000' "$script"
grep -Fq 'api_json POST "/api/admin/line-groups"' "$script"
grep -Fq '</dev/tcp/${H2}/${CLIENT_PORT}' "$script"
grep -Fq '</dev/tcp/${PH2}/${CLIENT_PORT}' "$script"
grep -Fq 'XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS=false' "$script"
if grep -Fq 'WHERE is_default = TRUE' "$script"; then
  echo "real-v2-plan-binding: must not bind temporary group to default plan" >&2
  exit 1
fi
echo "real-v2-plan-binding: passed"
