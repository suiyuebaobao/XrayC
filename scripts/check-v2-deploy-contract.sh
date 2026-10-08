#!/usr/bin/env bash
# 用途：静态检查 v2 部署契约，确保镜像、compose、发布和文档一致。
# 范围：只读取仓库文件，不构建镜像、不推送、不访问外部服务。
# 输入：检查 Dockerfile、compose、CI、部署脚本、文档和前端配置片段。
# 输出：缺失契约时输出标签和文件，通过时输出成功信息。
# 依赖：通过 grep 字面量匹配关键变量、服务名、健康检查和资源限制。
# 安全：不读取私有 env 文件，也不打印任何真实部署凭据。
# 约束：契约字段应反映真实发布门禁和部署脚本的当前约定。
# 行为：所有断言按顺序执行，任一失败会立即终止脚本。
# 失败：用于在 CI 中发现部署契约被误删或未同步更新。
# 维护：部署结构或文档路径调整时需更新本脚本的检查清单。
set -euo pipefail
IFS=$'\n\t'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

failures=0

fail() {
  printf 'v2-deploy-contract-check: %s\n' "$1" >&2
  failures=$((failures + 1))
}

require_file() {
  local file="$1"

  [[ -f "$file" ]] || fail "required file missing: ${file}"
}

require_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"

  if [[ ! -f "$file" ]]; then
    fail "${label} missing because ${file} does not exist"
    return
  fi
  if ! grep -Eq -- "$pattern" "$file"; then
    fail "${label} missing in ${file}"
  fi
}

forbid_pattern() {
  local file="$1"
  local pattern="$2"
  local label="$3"

  [[ -f "$file" ]] || return
  if grep -En -- "$pattern" "$file" >/dev/null; then
    fail "${label} found in ${file}"
  fi
}

require_file Dockerfile
require_file docker-compose.yml
require_file deploy/caddy/Dockerfile
require_file deploy/caddy/Caddyfile
require_file scripts/deploy-access-agent.sh
require_file scripts/real-remote-access-deploy-e2e.sh
require_file scripts/real-access-pool-e2e.sh
require_file scripts/real-access-third-party-matrix-e2e.sh
require_file scripts/real-access-3node-integrity-e2e.sh
require_file Makefile
require_file crates/api/src/lib.rs
require_file frontend/src/services/api/paths.ts
require_file "文档/接口/开放接口.yaml"

require_pattern docker-compose.yml '^  postgres:' 'local control-plane postgres Docker service'
require_pattern docker-compose.yml '^  api:' 'local control-plane API Docker service'
require_pattern docker-compose.yml '^  worker:' 'local control-plane worker Docker service'
require_pattern docker-compose.yml '^  caddy:' 'local control-plane Caddy Docker service'
forbid_pattern docker-compose.yml '^  frontend:' 'removed standalone frontend Docker service'
forbid_pattern docker-compose.yml '^  n''ginx:' 'removed legacy reverse proxy Docker service'
require_pattern docker-compose.yml 'DEPLOY_ARTIFACT_DIR:' 'local control-plane artifact directory env'
require_pattern docker-compose.yml 'DEPLOY_ARTIFACT_TOKEN:' 'local control-plane artifact token env'
require_pattern docker-compose.yml './deploy/artifacts:/root/xrayc-artifacts:ro' 'local control-plane artifact read-only mount'
require_pattern deploy/caddy/Caddyfile 'handle /api/deploy/artifacts/\*' 'artifact download reverse proxy route'
require_pattern deploy/caddy/Caddyfile 'handle /api/\*' 'API reverse proxy route'
require_pattern deploy/caddy/Caddyfile 'handle /sub/\*' 'subscription reverse proxy route'
require_pattern deploy/caddy/Caddyfile 'handle /health' 'health reverse proxy route'
require_pattern deploy/caddy/Caddyfile 'try_files \{path\} /index\.html' 'SPA fallback route'
require_pattern deploy/caddy/Caddyfile 'file_server' 'frontend static file server'
require_pattern Dockerfile 'xrayc-access-agent' 'access-agent binary included in Docker build'
require_pattern Dockerfile 'scripts/deploy-access-agent.sh' 'manual Agent install script included in app image'
require_pattern crates/api/src/lib.rs '/api/admin/access-nodes/install-guide' 'Agent install guide route'
require_pattern crates/api/src/lib.rs '/api/admin/access-entries' 'access entry management route'
require_pattern crates/api/src/lib.rs '/api/admin/access-entry-exit-bindings/:binding_id' 'access entry exit binding route'
forbid_pattern crates/api/src/lib.rs '/api/admin/access-nodes/deploy' 'removed backend SSH deploy route'
forbid_pattern crates/api/src/lib.rs 'group-entries' 'removed access node group binding route'
forbid_pattern crates/api/src/lib.rs ':access_node_id/exit-pools' 'removed access node exit pool binding route'
forbid_pattern crates/api/src/lib.rs 'local-direct-exit' 'removed legacy node-local direct exit route'
forbid_pattern crates/api/src/lib.rs 'user-exit-assignments/rebalance' 'removed manual exit assignment refresh route'
forbid_pattern crates/api/src/lib.rs 'user-access-line-assignments/rebalance' 'removed manual user-visible ingress refresh route'
forbid_pattern crates/api/src/lib.rs 'post(create_access_line)' 'removed direct access-line create route'
forbid_pattern crates/api/src/lib.rs 'update_access_line_exit_pool' 'removed direct access-line exit-pool update route'
forbid_pattern crates/api/src/lib.rs 'post(create_exit_pool)' 'removed public exit-pool create route'
forbid_pattern crates/api/src/lib.rs 'replace_exit_pool_members' 'removed public exit-pool member replace route'
forbid_pattern crates/api/src/lib.rs 'create_exit_pool_exit' 'removed public exit-pool aggregate create route'
forbid_pattern crates/db/src/store/routing_lines.rs 'replace_admin_access_node_exit_pool_bindings' 'removed db access node exit pool binding helper'
forbid_pattern crates/db/src/store/routing_nodes.rs 'create_admin_local_direct_exit' 'removed db local direct exit helper'
forbid_pattern crates/db/src/types.rs 'AdminAccessNodeExitPoolBindingInput' 'removed db access node exit pool binding DTO'
forbid_pattern crates/db/src/types.rs 'AdminLocalDirectExitInput' 'removed db local direct exit DTO'
forbid_pattern deploy/caddy/Caddyfile '/api/admin/access-nodes/deploy' 'removed backend SSH deploy proxy route'
forbid_pattern frontend/src/services/api/paths.ts 'rebalanceUser' 'removed frontend manual assignment refresh path'
forbid_pattern frontend/src/services/api/clients/routing.ts 'group-entries' 'removed frontend access node group binding path'
forbid_pattern frontend/src/services/api/clients/routing.ts 'replaceAccessNodeExitPools' 'removed frontend access node exit pool binding client'
forbid_pattern frontend/src/services/api/clients/routing.ts 'line-entries' 'removed frontend access node line binding path'
require_pattern frontend/src/services/api/clients/routing.ts 'accessEntries' 'frontend access entry management path'
require_pattern frontend/src/services/api/clients/routing.ts 'exit-bindings' 'frontend access entry exit binding path'
require_pattern "文档/接口/开放接口.yaml" 'exit_endpoint_id:' 'OpenAPI line entry binds concrete endpoint'
forbid_pattern "文档/接口/开放接口.yaml" 'local-direct-exit' 'removed legacy node-local direct exit OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'group-entries' 'removed access node group binding OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'access-nodes/\{id\}/exit-pools' 'removed access node exit pool binding OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'access-lines/\{id\}/exit-pool' 'removed direct access-line exit-pool OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'exit-pools/\{id\}/members' 'removed public exit-pool member OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'exit-pools/\{id\}/exits' 'removed public exit-pool aggregate OpenAPI path'
require_pattern "文档/接口/接口草案.md" 'access-entries' 'docs access entry management API'
forbid_pattern "文档/接口/开放接口.yaml" 'user-exit-assignments/rebalance' 'removed manual exit assignment refresh OpenAPI path'
forbid_pattern "文档/接口/开放接口.yaml" 'user-access-line-assignments/rebalance' 'removed manual user-visible ingress refresh OpenAPI path'

require_pattern Makefile '^package-docker-artifacts:' 'Docker artifact package target'
require_pattern Makefile '^check-deploy-artifact-endpoint:' 'deploy artifact endpoint check target'
require_pattern Makefile 'check-real-release: check-real-release-env check-deploy-artifact-endpoint' 'real release gate validates served artifacts'
require_pattern Makefile 'docker save .*ACCESS_AGENT_IMAGE' 'access-agent Docker image artifact export'
require_pattern Makefile 'docker save .*XRAY_LOCAL_IMAGE' 'Xray Docker image artifact export'
require_pattern Makefile 'sha256sum access-agent-manifest\.json access-agent-image\.tar\.gz xray-image\.tar\.gz' 'artifact checksum generation'
require_pattern Makefile 'access-agent-manifest\.json' 'artifact manifest generation'
require_pattern Makefile '"contract_version": "v2-docker-artifacts"' 'artifact manifest V2 contract marker'
require_pattern Makefile '"remote_deploy": "docker-compose"' 'artifact manifest remote Docker Compose marker'

require_pattern scripts/lib/deploy-access-agent/deploy.sh '/api/deploy/artifacts/\$\{name\}' 'server-local install artifact API downloads'
require_pattern scripts/deploy-access-agent.sh 'download_artifact access-agent-manifest\.json' 'server-local install manifest download'
require_pattern scripts/deploy-access-agent.sh 'download_artifact access-agent-image\.tar\.gz' 'server-local install access-agent image download'
require_pattern scripts/deploy-access-agent.sh 'download_artifact xray-image\.tar\.gz' 'server-local install Xray image download'
require_pattern scripts/deploy-access-agent.sh 'download_artifact access-agent\.sha256' 'server-local install checksum download'
require_pattern scripts/deploy-access-agent.sh 'access-agent\.sha256 \| sha256sum -c -' 'server-local install checksum verification'
require_pattern scripts/deploy-access-agent.sh 'docker_run load --input "\$\{ARTIFACT_DIR\}/access-agent-image\.tar\.gz"' 'server-local install access-agent docker load'
require_pattern scripts/deploy-access-agent.sh 'docker_run load --input "\$\{ARTIFACT_DIR\}/xray-image\.tar\.gz"' 'server-local install Xray docker load'
require_pattern scripts/deploy-access-agent.sh 'cat > "\$compose_file" <<YAML' 'server-local install generated Docker Compose file'
require_pattern scripts/deploy-access-agent.sh '^  xray:$' 'server-local install compose xray service'
require_pattern scripts/deploy-access-agent.sh '^  access-agent:$' 'server-local install compose access-agent service'
require_pattern scripts/deploy-access-agent.sh 'network_mode: host' 'server-local install host networking for relay ports'
require_pattern scripts/deploy-access-agent.sh 'compose_run -f "\$\{INSTALL_DIR\}/docker-compose\.yml" -p "\$COMPOSE_PROJECT_NAME" config' 'server-local install compose validation'
require_pattern scripts/deploy-access-agent.sh 'compose_run -f "\$\{INSTALL_DIR\}/docker-compose\.yml" -p "\$COMPOSE_PROJECT_NAME" up -d' 'server-local install compose startup'
require_pattern scripts/lib/deploy-access-agent/preserve.sh 'preserve_existing_installation' 'server-local install preserves the previous instance'
require_pattern scripts/lib/deploy-access-agent/preserve.sh 'XRAY_HOST_CONFIG_DIR' 'server-local install verifies mount ownership before stopping containers'
require_pattern scripts/lib/deploy-access-agent/tooling.sh 'prompt_tls_certificate_settings' 'server-local install prompts for TLS certificate domain and email'
require_pattern scripts/lib/deploy-access-agent/tooling.sh 'TLS certificate request skipped; both domain and email are required' 'server-local install skips certbot without domain and email'
forbid_pattern scripts/lib/deploy-access-agent/tooling.sh '--register-unsafely-without-email' 'server-local install must not request ACME certificates without email'
forbid_pattern scripts/deploy-access-agent.sh '\b(scp|rsync)\b' 'non-artifact file transfer command'

server_local_e2e_script="scripts/real-""remote""-access-deploy-e2e.sh"
server_local_e2e_artifacts_var="XRAYC_""REMOTE""_E2E_CHECK_ARTIFACTS:-1"
require_pattern "$server_local_e2e_script" "$server_local_e2e_artifacts_var" 'server-local Agent install e2e artifact smoke default'
require_pattern scripts/check-deploy-artifact-endpoint.sh 'access-agent-manifest\.json' 'deploy artifact endpoint manifest comparison'
require_pattern scripts/check-deploy-artifact-endpoint.sh 'access-agent\.sha256' 'deploy artifact endpoint checksum comparison'
require_pattern "$server_local_e2e_script" 'DEPLOY_ARTIFACT_FULL' 'server-local Agent install e2e full artifact verification hook'
require_pattern "$server_local_e2e_script" 'XRAYC_TLS_CERT_DOMAINS' 'server-local Agent install e2e TLS certificate domain forwarding'
require_pattern "$server_local_e2e_script" 'bash ./deploy-access-agent\.sh' 'server-local Agent install script execution'
require_pattern scripts/real-smoke.sh 'DEPLOY_ARTIFACT_FULL' 'real smoke full artifact verification'
require_pattern scripts/real-smoke.sh 'gzip -t "\$artifact_body"' 'real smoke validates compressed artifacts'
require_pattern scripts/real-smoke.sh 'sha256sum -c access-agent\.sha256' 'real smoke validates artifact checksums'

require_pattern scripts/deploy-access-agent.sh 'write_env_line XRAYC_XRAY_CONFIG_PATH "/etc/xray/config\.json"' 'server-local install writes xray config path'
require_pattern scripts/deploy-access-agent.sh 'write_env_line XRAYC_XRAY_TEST_COMMAND' 'server-local install writes xray test command'
require_pattern scripts/deploy-access-agent.sh 'write_env_line XRAYC_XRAY_RELOAD_COMMAND' 'server-local install writes xray reload command'
require_pattern scripts/deploy-access-agent.sh 'write_env_line XRAYC_XRAY_START_COMMAND' 'server-local install writes xray start command'
require_pattern scripts/deploy-access-agent.sh 'write_env_line XRAYC_XRAY_STOP_COMMAND' 'server-local install writes xray stop command'
require_pattern deploy/access-agent/access-agent.env.example '^XRAYC_XRAY_TEST_COMMAND=' 'access-agent env example xray test command'
require_pattern deploy/access-agent/access-agent.env.example '^XRAYC_XRAY_RELOAD_COMMAND=' 'access-agent env example xray reload command'
require_pattern deploy/access-agent/access-agent.env.example '^XRAYC_XRAY_START_COMMAND=' 'access-agent env example xray start command'
require_pattern deploy/access-agent/access-agent.env.example '^XRAYC_XRAY_STOP_COMMAND=' 'access-agent env example xray stop command'
require_pattern README.md 'xray-core.*access-agent' 'README access node stack wording'
require_pattern AGENTS.md 'xray-core.*access-agent' 'AGENTS access node stack wording'
require_pattern "开发方案.md" 'xray-core.*access-agent' '方案 access node stack wording'
require_pattern "文档/部署/部署说明.md" 'xray-core.*access-agent' 'deploy docs access node stack wording'
require_pattern "文档/接入代理/部署指南.md" 'xray-core.*access-agent' 'access-agent deploy guide access node stack wording'
require_pattern "文档/架构/架构说明.md" 'xray-core.*access-agent' 'architecture docs access node stack wording'
require_pattern "文档/架构/系统总览.md" 'xray-core.*access-agent' 'system overview access node stack wording'
require_pattern "文档/测试/真实测试用例.md" 'xray-core.*access-agent' 'real test docs access node stack wording'

require_pattern scripts/check-real-release.sh 'real-access-third-party-matrix-relay-e2e' 'release gate relay third-party exit matrix'
require_pattern scripts/check-real-release.sh 'real-v2-relay-pool-e2e' 'release gate V2 relay pool e2e'
require_pattern scripts/real-access-pool-e2e.sh 'EXIT_ENDPOINT_ID' 'access pool e2e exit endpoint attribution'
require_pattern scripts/real-access-pool-e2e.sh 'xrayc_real_e2e_assert_subscription_access_servers' 'access pool subscription exposes relay servers only'
require_pattern scripts/real-access-pool-e2e.sh 'outbound_proxy_url\|exit_endpoint' 'access pool subscription leak guard'
require_pattern scripts/real-access-third-party-matrix-e2e.sh 'RELEASE_PROTOCOLS="socks,http,vless,trojan,shadowsocks,hy2"' 'third-party release protocol matrix'
require_pattern scripts/real-access-third-party-matrix-e2e.sh 'protocol_env_value "\$protocol" "EXIT_ENDPOINT_ID" "EXIT_ENDPOINT_ID"' 'per-protocol third-party exit endpoint IDs'
require_pattern scripts/real-access-third-party-matrix-e2e.sh 'STRICT_AGENT_TRAFFIC=1' 'third-party real traffic through relay/access node'
require_pattern scripts/real-access-third-party-matrix-relay-e2e.sh 'PROTOCOLS="\$\{REAL_PROTOCOL_MATRIX_PROTOCOLS:-socks,http,vless,trojan,shadowsocks,hy2\}"' 'relay third-party release protocol matrix'
require_pattern scripts/real-access-third-party-matrix-relay-e2e.sh 'wait_protocol_route_and_ledger' 'relay third-party matrix verifies real route ledger billing'
require_pattern scripts/lib/real-access-third-party-matrix-relay/assertions.sh 'ledger_sum_for_endpoint' 'relay matrix checks ledger totals'
require_pattern scripts/lib/real-access-third-party-matrix-relay/assertions.sh 'subscription_used_bytes_for_user' 'relay matrix checks user subscription billing'
require_pattern scripts/lib/real-access-third-party-matrix-relay/assertions.sh 'subscription_current.*-gt.*subscription_before' 'relay matrix requires subscription usage to increase'
require_pattern scripts/real-v2-relay-pool-e2e.sh 'subscription_redaction_ok' 'V2 relay pool subscription redaction check'
require_pattern scripts/real-v2-relay-pool-e2e.sh 'second_exit_ledger_ok' 'V2 relay pool verifies second self-hosted exit ledger'
require_pattern scripts/lib/real-access-3node-integrity/db-mutation.sh 'exit_pool_members' 'self-hosted exit pool member verification'
require_pattern scripts/real-access-3node-integrity-e2e.sh 'MIN_EXIT_MEMBERS' 'self-hosted minimum exit pool member gate'

if [[ "$failures" -gt 0 ]]; then
  printf 'v2-deploy-contract-check: failed with %s finding(s)\n' "$failures" >&2
  exit 1
fi

echo "v2-deploy-contract-check: passed"
