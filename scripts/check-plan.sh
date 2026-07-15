#!/usr/bin/env bash
# 用途：检查产品计划、实现文件和测试覆盖之间的静态一致性。
# 范围：只扫描仓库文本，不执行服务、不访问网络、不读取私有 env。
# 输入：读取指定源码、前端、脚本和文档文件中的关键契约片段。
# 输出：缺失项以标签和路径报告，全部满足时输出通过信息。
# 依赖：使用 grep 字面量和正则匹配计划要求的 API、字段和用例。
# 安全：只匹配固定字符串，不打印可能包含敏感数据的文件内容。
# 约束：计划中的主线能力变更应同时反映到本检查脚本。
# 行为：任一 require 断言失败都会立即退出并阻断后续检查。
# 失败：用于 CI 或本地快速发现实现与计划文档漂移。
# 维护：新增或删除计划项时同步更新检查清单和标签。
set -euo pipefail

run_mode=false
if [[ "${1:-}" == "--run" ]]; then
  run_mode=true
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo_root"

default_test_field_encryption_keys="new:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ,old:AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM,test:unit-test-field-key"

mapfile -t changed_files < <(
  {
    git -c core.quotepath=false diff --name-only HEAD 2>/dev/null || true
    git -c core.quotepath=false diff --name-only --cached HEAD 2>/dev/null || true
    git -c core.quotepath=false ls-files --others --exclude-standard 2>/dev/null || true
  } | awk 'NF' | sort -u
)

if [[ ${#changed_files[@]} -eq 0 ]]; then
  echo "未发现本地改动。建议只在交付前运行：make check-release"
  exit 0
fi

has_rust=false
has_frontend=false
has_frontend_lock=false
has_e2e=false
has_db=false
has_api=false
has_agent=false
has_docker=false
has_deploy=false
has_docs=false
has_shell=false
has_makefile=false
has_release_risk=false

for file in "${changed_files[@]}"; do
  case "$file" in
    *.rs|Cargo.toml|Cargo.lock|crates/*/Cargo.toml)
      has_rust=true
      ;;
  esac

  case "$file" in
    frontend/package-lock.json|frontend/package.json)
      has_frontend=true
      has_frontend_lock=true
      ;;
    frontend/*|frontend/**/*)
      has_frontend=true
      ;;
  esac

  case "$file" in
    frontend/e2e/*|frontend/e2e/**/*)
      has_e2e=true
      ;;
  esac

  case "$file" in
    migrations/*|crates/db/*|crates/db/**/*)
      has_db=true
      ;;
  esac

  case "$file" in
    crates/api/*|crates/api/**/*)
      has_api=true
      ;;
  esac

  case "$file" in
    crates/access-agent/*|crates/access-agent/**/*|crates/xray-config/*|crates/xray-config/**/*)
      has_agent=true
      ;;
  esac

  case "$file" in
    Dockerfile|docker-compose.yml|deploy/caddy/Dockerfile|deploy/caddy/Caddyfile|frontend/.dockerignore|.dockerignore)
      has_docker=true
      has_release_risk=true
      ;;
  esac

  case "$file" in
    Makefile)
      has_makefile=true
      ;;
  esac

  case "$file" in
    deploy/*|deploy/**/*|scripts/deploy-*.sh)
      has_deploy=true
      has_release_risk=true
      ;;
  esac

  case "$file" in
    Cargo.lock|frontend/package-lock.json|migrations/*)
      has_release_risk=true
      ;;
  esac

  case "$file" in
    文档/*|文档/**/*|docs/*|docs/**/*|README.md|AGENTS.md|CHANGELOG.md|开发方案.md|*.md|*.example|.env.*.example|deploy/*.example|deploy/**/*.example|frontend/src/views/AdminTutorialPage.vue)
      has_docs=true
      ;;
  esac

  case "$file" in
    scripts/*.sh|scripts/**/*.sh)
      has_shell=true
      ;;
  esac
done

commands=()

add_command() {
  local command="$1"
  for existing in "${commands[@]}"; do
    [[ "$existing" == "$command" ]] && return 0
  done
  commands+=("$command")
}

if [[ "$has_rust" == true ]]; then
  add_command "cargo fmt --check"
  if [[ "$has_api" == true ]]; then
    add_command "cargo test -p xrayc-api"
  fi
  if [[ "$has_db" == true ]]; then
    add_command "cargo test -p xrayc-db"
  fi
  if [[ "$has_agent" == true ]]; then
    add_command "cargo test -p xrayc-access-agent"
    add_command "cargo test -p xrayc-xray-config"
  fi
  if [[ "$has_api" != true && "$has_db" != true && "$has_agent" != true ]]; then
    add_command "cargo test --workspace --lib"
  fi
fi

if [[ "$has_frontend" == true ]]; then
  if [[ "$has_frontend_lock" == true ]]; then
    add_command "cd frontend && npm ci"
  fi
  add_command "cd frontend && npm run build"
fi

if [[ "$has_e2e" == true ]]; then
  add_command "cd frontend && E2E_BASE_URL=\${E2E_BASE_URL:-http://127.0.0.1:8080} E2E_USER_ACCOUNT=\${E2E_USER_ACCOUNT:-demo@example.test} E2E_USER_PASSWORD=\${E2E_USER_PASSWORD:-demo123456} E2E_ADMIN_ACCOUNT=\${E2E_ADMIN_ACCOUNT:-admin@example.test} E2E_ADMIN_PASSWORD=\${E2E_ADMIN_PASSWORD:-admin123456} npx playwright test e2e/smoke.spec.ts e2e/runtime-no-mock.spec.ts"
fi

if [[ "$has_shell" == true || "$has_deploy" == true ]]; then
  add_command "for script in scripts/real-*.sh scripts/api-contract-smoke-draft.sh scripts/bootstrap-real-release-env.sh scripts/prepare-real-e2e-auth-env.sh scripts/prepare-real-protocol-matrix-assets.sh scripts/check-real-protocol-matrix-env.sh scripts/observe-real-client-env.sh scripts/check-real-release.sh scripts/check-real-release-env.sh scripts/prepare-real-release-env.sh scripts/validate-real-release-env.sh scripts/check-real-release-assets-docs.sh scripts/check-agent-install-cleanup.sh scripts/check-v2-deploy-contract.sh scripts/verify-no-secrets.sh scripts/check-no-legacy.sh scripts/check-source-file-length.sh scripts/deploy-access-agent.sh scripts/test-real-third-party-redaction.sh; do bash -n \"\$script\"; done"
  add_command "find scripts/lib -name '*.sh' -print0 | xargs -0 -r -n1 bash -n"
  add_command "bash scripts/check-real-release-assets-docs.sh"
  add_command "bash scripts/check-agent-install-cleanup.sh"
  add_command "bash scripts/check-v2-deploy-contract.sh"
fi

if [[ "$has_makefile" == true ]]; then
  add_command "make -n check-affected > /dev/null"
fi

if [[ "$has_docs" == true ]]; then
  legacy_agent_term="节点""代理"
  legacy_project_term="Ray""Pilot"
  legacy_line_term="rp""-line"
  legacy_system_term="代理""订阅系统"
  add_command "docs_targets=(README.md AGENTS.md CHANGELOG.md 开发方案.md docs 文档 .env.real-release.example deploy/access-agent/access-agent.env.example); rg --glob '!私有/**' --glob '!**/私有/**' -q \"中转节点强控制|接入/中转节点|控制面只强控制\" \"\${docs_targets[@]}\" && rg --glob '!私有/**' --glob '!**/私有/**' -q \"自建出口|普通自建上游|本机出口服务\" \"\${docs_targets[@]}\" && rg --glob '!私有/**' --glob '!**/私有/**' -q \"第三方出口|第三方上游\" \"\${docs_targets[@]}\" && rg --glob '!私有/**' --glob '!**/私有/**' -q \"用户级平滑限速|rate_limit_bps|tc HTB\" \"\${docs_targets[@]}\" && rg --glob '!私有/**' --glob '!**/私有/**' -q \"Playwright.*real-smoke|real-smoke.*Playwright|真实客户端流量\" \"\${docs_targets[@]}\" && rg --glob '!私有/**' --glob '!**/私有/**' -q \"subagent.*并行|并行.*subagent\" \"\${docs_targets[@]}\""
  add_command "docs_targets=(README.md AGENTS.md CHANGELOG.md 开发方案.md docs 文档 .env.real-release.example deploy/access-agent/access-agent.env.example); ! rg --glob '!私有/**' --glob '!**/私有/**' -n \"${legacy_project_term}|${legacy_line_term}|${legacy_system_term}|${legacy_agent_term}\" \"\${docs_targets[@]}\""
  add_command "docs_targets=(README.md AGENTS.md CHANGELOG.md 开发方案.md docs 文档 .env.real-release.example deploy/access-agent/access-agent.env.example); ! rg --glob '!私有/**' --glob '!**/私有/**' -n \"最终选择.*订阅.*直接暴露.*出口|采用.*订阅.*直接暴露.*出口|订阅直暴出口回流\" \"\${docs_targets[@]}\""
  add_command "rg -q \"已否决/非采用方案|已否决|非采用方案\" 文档/架构决策 && rg -q \"usage_ledgers\\.exit_endpoint_id|exit_endpoint_id.*账本\" 开发方案.md"
  add_command "bash scripts/check-docs-no-stale.sh"
fi

if [[ "$has_docker" == true || "$has_deploy" == true ]]; then
  add_command "docker compose config --services"
  add_command "bash scripts/check-v2-deploy-contract.sh"
  add_command "make check-docker-runtime"
fi

if [[ "$has_docs" == true || "$has_shell" == true || "$has_deploy" == true || "$has_docker" == true ]]; then
  add_command "bash scripts/check-no-legacy.sh"
  add_command "bash scripts/verify-no-secrets.sh"
fi

add_command "bash scripts/check-source-file-length.sh"
add_command "bash scripts/test-v2-mainline-coverage.sh"
add_command "bash scripts/test-real-third-party-redaction.sh"
add_command "git diff --check"

echo "改动文件："
printf '  %s\n' "${changed_files[@]}"
echo
echo "建议本轮最小验证："
printf '  %s\n' "${commands[@]}"

if [[ "$has_docker" == true || "$has_deploy" == true || "$has_frontend_lock" == true ]]; then
  echo
  echo "提示：本轮涉及 Docker、部署或依赖变更，交付前再运行 make check-release；日常开发不要反复 make rebuild。"
fi

echo
if [[ "$has_release_risk" == true ]]; then
  echo "测试档位：发布风险改动。开发中仍先执行以上最小验证，最终交付前再执行 make check-release；只有 Dockerfile、Compose、依赖或部署产物确实变化时才执行 make rebuild。"
else
  echo "测试档位：普通改动。优先执行 make check-affected；不需要 Docker rebuild，不需要 npm ci，不需要 make package-docker-artifacts。"
fi

if [[ "$run_mode" != true ]]; then
  exit 0
fi

echo
echo "开始执行建议验证..."
export XRAYC_FIELD_ENCRYPTION_KEYS="${XRAYC_FIELD_ENCRYPTION_KEYS:-$default_test_field_encryption_keys}"
for command in "${commands[@]}"; do
  echo "+ $command"
  bash -lc "$command"
done
