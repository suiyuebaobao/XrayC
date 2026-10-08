#!/usr/bin/env bash
# 用途：执行真实安装入口，故障注入中心/清单下载失败，验证既有配置、状态和日志完全保留。
# 外部 Docker、curl 与系统管理命令被隔离桩替代，不连接服务器、不操作真实容器。
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
case_dir="$(mktemp -d)"
trap 'rm -rf "$case_dir"' EXIT
mkdir -p "$case_dir/bin" "$case_dir/existing/logs/xray" "$case_dir/existing/state" "$case_dir/existing/xray"
printf 'existing-log\n' > "$case_dir/existing/logs/xray/access.log"
printf 'existing-state\n' > "$case_dir/existing/state/state.json"
printf 'existing-config\n' > "$case_dir/existing/xray/config.json"
printf 'XRAYC_NODE_ID=00000000-0000-0000-0000-000000000099\nXRAYC_NODE_TOKEN=fixture-agent\n' > "$case_dir/existing/access-agent.env"
cat > "$case_dir/bin/docker" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$XRAYC_TEST_DOCKER_CALLS"
exit 0
EOF
cat > "$case_dir/bin/id" <<'EOF'
#!/usr/bin/env bash
printf '0\n'
EOF
cat > "$case_dir/bin/uname" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == '-s' ]]; then printf 'Linux\n'; else printf 'x86_64\n'; fi
EOF
cat > "$case_dir/bin/curl" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == *'/health'* && "$XRAYC_TEST_FAIL_STAGE" != 'health' ]]; then
  printf '{"success":true,"service":"xrayc-api"}\n'
  exit 0
fi
exit 22
EOF
for tool in systemctl service sha256sum; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$case_dir/bin/$tool"
done
chmod +x "$case_dir/bin/"*
for stage in health manifest; do
  : > "$case_dir/docker-calls"
  if env PATH="$case_dir/bin:$PATH" \
    XRAYC_TEST_DOCKER_CALLS="$case_dir/docker-calls" XRAYC_TEST_FAIL_STAGE="$stage" \
    XRAYC_CONTROL_PLANE_URL=http://control.example.invalid \
    XRAYC_DEPLOY_ARTIFACT_TOKEN=fixture-token XRAYC_NODE_TOKEN=fixture-agent \
    XRAYC_NODE_ID=00000000-0000-0000-0000-000000000099 \
    XRAYC_INSTALL_DIR="$case_dir/existing" XRAYC_RATE_LIMITER_INTERFACE=eth0 \
    XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG=true XRAYC_DEPLOY_ROLLBACK_REMOVE_EXISTING=true \
    XRAYC_CLEAN_LEGACY_COMPOSE_PROJECTS=true \
    bash "$repo_dir/scripts/deploy-access-agent.sh" > "$case_dir/$stage.log" 2>&1; then
    printf 'preflight-test: expected %s failure\n' "$stage" >&2; exit 1
  fi
  [[ "$(cat "$case_dir/existing/logs/xray/access.log")" == existing-log ]]
  [[ "$(cat "$case_dir/existing/state/state.json")" == existing-state ]]
  [[ "$(cat "$case_dir/existing/xray/config.json")" == existing-config ]]
  ! grep -Eq '(^| )(rm|stop|down|kill)( |$)' "$case_dir/docker-calls"
  grep -q 'preflight failed before runtime changes' "$case_dir/$stage.log"
  if [[ "$stage" == health ]]; then
    grep -q 'center health preflight failed' "$case_dir/$stage.log"
  else
    grep -q 'artifact download failed for access-agent-manifest.json' "$case_dir/$stage.log"
  fi
  printf 'preflight-test: %s failure preserves existing services, state and logs\n' "$stage"
done
