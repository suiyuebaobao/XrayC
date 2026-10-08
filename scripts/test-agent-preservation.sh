#!/usr/bin/env bash
# 用途：以隔离 Docker 容器实测保留与回退，不连接 SSH，不操作现有服务。
# 同 project 的无关容器也必须保持 ID、启动时间和运行状态不变。
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
source "${repo_dir}/scripts/lib/deploy-access-agent/common.sh"
source "${repo_dir}/scripts/lib/deploy-access-agent/preserve.sh"
case_dir="$(mktemp -d /tmp/xrayc-preserve-test.XXXXXX)"
test_ids=()
cleanup() {
  for id in "${test_ids[@]}"; do docker rm -f "$id" >/dev/null 2>&1 || true; done
  rm -rf "$case_dir"
}
trap cleanup EXIT
INSTALL_DIR="${case_dir}/owned"
XRAY_HOST_CONFIG_DIR="${INSTALL_DIR}/xray"
tmp_dir="${case_dir}/scratch"
mkdir -p "$XRAY_HOST_CONFIG_DIR" "$tmp_dir" "${INSTALL_DIR}/state" "${INSTALL_DIR}/logs/xray" "${case_dir}/other"
printf 'original-config\n' > "${XRAY_HOST_CONFIG_DIR}/config.json"
printf 'original-log\n' > "${INSTALL_DIR}/logs/xray/access.log"
printf 'XRAYC_NODE_ID=fixture\n' > "${INSTALL_DIR}/access-agent.env"
printf 'original-compose\n' > "${INSTALL_DIR}/docker-compose.yml"
printf 'original-backlog\n' > "${INSTALL_DIR}/state/traffic-backlog.json"
COMPOSE_PROJECT_NAME="xrayc-preserve-test-$$"
DEPLOY_START=true
DEPLOY_START_EPOCH="$(date +%s)"
agent_container="xrayc-access-agent-preserve-test-$$"
xray_container="xrayc-xray-preserve-test-$$"
image="${XRAYC_PRESERVATION_TEST_IMAGE:-postgres:16-alpine}"
validate_install_ownership
for name in "$xray_container" "$agent_container"; do
  id="$(docker run -d --name "$name" --restart unless-stopped \
    --label "com.docker.compose.project=${COMPOSE_PROJECT_NAME}" \
    -v "${XRAY_HOST_CONFIG_DIR}:/etc/xray:ro" --entrypoint sh "$image" -c 'echo preserved-container-log; exec sleep 86400')"
  test_ids+=("$id")
done
original_xray="${test_ids[0]}"
original_agent="${test_ids[1]}"
unrelated="$(docker run -d --name "unrelated-preserve-test-$$" \
  --label "com.docker.compose.project=${COMPOSE_PROJECT_NAME}" --label com.docker.compose.service=xray \
  -v "${case_dir}/other:/etc/xray:ro" --entrypoint sh "$image" -c 'exec sleep 86400')"
test_ids+=("$unrelated")
unrelated_before="$(docker inspect -f '{{.Id}} {{.State.StartedAt}} {{.State.Running}}' "$unrelated")"

preserve_existing_installation
[[ "$(docker inspect -f '{{.State.Running}}' "$original_xray")" == false ]]
docker logs "$original_xray" | grep -q preserved-container-log
[[ "$(cat "${INSTALL_DIR}/logs/xray/access.log")" == original-log ]]
[[ "$(docker inspect -f '{{.Id}} {{.State.StartedAt}} {{.State.Running}}' "$unrelated")" == "$unrelated_before" ]]

# 模拟只启动一半就失败的新版本，期间已产生的新补送队列同样不能丢失。
printf 'new-config\n' > "${XRAY_HOST_CONFIG_DIR}/config.json"
printf 'new-pending-backlog\n' > "${INSTALL_DIR}/state/traffic-backlog.json"
replacement="$(docker run -d --name "$xray_container" --restart unless-stopped \
  --label "com.docker.compose.project=${COMPOSE_PROJECT_NAME}" \
  -v "${XRAY_HOST_CONFIG_DIR}:/etc/xray:ro" --entrypoint sh "$image" -c 'echo failed-release-log; exec sleep 86400')"
test_ids+=("$replacement")
restore_preserved_installation
[[ "$(docker inspect -f '{{.Id}}' "$xray_container")" == "$original_xray" ]]
[[ "$(docker inspect -f '{{.Id}}' "$agent_container")" == "$original_agent" ]]
[[ "$(docker inspect -f '{{.State.Running}} {{.HostConfig.RestartPolicy.Name}}' "$original_xray")" == 'true unless-stopped' ]]
[[ "$(docker exec "$original_xray" cat /etc/xray/config.json)" == original-config ]]
[[ "$(cat "${INSTALL_DIR}/logs/xray/access.log")" == original-log ]]
[[ "$(cat "${INSTALL_DIR}/state/traffic-backlog.json")" == new-pending-backlog ]]
docker logs "$original_xray" | grep -q preserved-container-log
docker logs "$replacement" | grep -q failed-release-log
[[ "$(docker inspect -f '{{.State.Running}}' "$replacement")" == false ]]
[[ "$(docker inspect -f '{{.Id}} {{.State.StartedAt}} {{.State.Running}}' "$unrelated")" == "$unrelated_before" ]]
printf 'agent-preservation-test: original IDs, configuration, logs, pending traffic and unrelated service preserved\n'
