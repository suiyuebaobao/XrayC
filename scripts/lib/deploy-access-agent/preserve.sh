#!/usr/bin/env bash
# 用途：只替换本安装目录拥有的 XrayC 容器；保留旧容器和日志，失败时恢复原配置及运行状态。
# 旧容器使用原 ID 保存，采用新的 Compose 实例名避免 Compose 自动回收旧容器。

validate_install_ownership() {
  require_env INSTALL_DIR
  require_env COMPOSE_PROJECT_NAME
  [[ "$INSTALL_DIR" == /* && "$INSTALL_DIR" != *'/../'* && "$INSTALL_DIR" != */.. ]] || die "install directory must be an absolute application directory"
  case "${INSTALL_DIR%/}" in ''|/|/opt|/root|/home|/usr|/etc|/var|/tmp) die "refusing a system directory as install directory" ;; esac
  [[ "$COMPOSE_PROJECT_NAME" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || die "invalid Compose project name"
  if run_root test -d "$INSTALL_DIR" && ! run_root test -f "${INSTALL_DIR}/access-agent.env"; then
    local existing
    existing="$(run_root find "$INSTALL_DIR" -mindepth 1 -maxdepth 1 -print -quit)"
    [[ -z "$existing" ]] || die "install directory contains other data and is not an existing XrayC agent installation"
  fi
}

preserve_existing_installation() {
  [[ "${DEPLOY_PRESERVE_READY:-0}" != "1" ]] || return 0
  if [[ "$DEPLOY_START" != "true" ]] && run_root test -f "${INSTALL_DIR}/access-agent.env"; then
    die "updating an existing installation requires XRAYC_DEPLOY_START=true"
  fi
  DEPLOY_BACKUP_DIR="${INSTALL_DIR}.before-${DEPLOY_START_EPOCH}-$$"
  run_root install -d -m 0700 "$DEPLOY_BACKUP_DIR"
  local candidates="${tmp_dir}/old-container-candidates"
  local validated="${tmp_dir}/old-containers.tsv"
  : > "$candidates"
  : > "$validated"
  docker_run ps -aq --filter "label=com.docker.compose.project=${COMPOSE_PROJECT_NAME}" >> "$candidates"
  docker_run inspect -f '{{.Id}}' "$agent_container" "$xray_container" >> "$candidates" 2>/dev/null || true
  local id name policy retries running mounts service
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    name="$(docker_run inspect -f '{{.Name}}' "$id")"; name="${name#/}"
    case "$name" in *-before-*|*-failed-*) continue ;; esac
    mounts="$(docker_run inspect -f '{{range .Mounts}}{{println .Source}}{{end}}' "$id")"
    service="$(docker_run inspect -f '{{index .Config.Labels "com.docker.compose.service"}}' "$id")"
    if ! printf '%s\n' "$mounts" | grep -Fqx -- "$XRAY_HOST_CONFIG_DIR"; then
      if [[ "$name" == "$agent_container" || "$name" == "$xray_container" ]]; then
        die "container name belongs to another installation; no services changed"
      fi
      continue
    fi
    case "$name:$service" in xrayc-xray-*:*|xrayc-access-agent-*:*|*:xray|*:access-agent) ;; *) continue ;; esac
    policy="$(docker_run inspect -f '{{.HostConfig.RestartPolicy.Name}}' "$id")"
    retries="$(docker_run inspect -f '{{.HostConfig.RestartPolicy.MaximumRetryCount}}' "$id")"
    [[ "$policy" != 'on-failure' || "$retries" == 0 ]] || policy="${policy}:${retries}"
    running="$(docker_run inspect -f '{{.State.Running}}' "$id")"
    # 统一为完整 ID，避免同一容器从名称和 project 两条路径被重复归档。
    id="$(docker_run inspect -f '{{.Id}}' "$id")"
    printf '%s\t%s\t%s\t%s\n' "$id" "$name" "$policy" "$running" >> "$validated"
  done < <(sort -u "$candidates")
  sort -u "$validated" > "${validated}.unique"
  write_file_root 0600 root root "${validated}.unique" "${DEPLOY_BACKUP_DIR}/containers.tsv"
  local item
  for item in access-agent.env docker-compose.yml xray bin; do
    if run_root test -e "${INSTALL_DIR}/${item}"; then
      run_root cp -a "${INSTALL_DIR}/${item}" "${DEPLOY_BACKUP_DIR}/${item}"
    fi
  done
  DEPLOY_PRESERVE_READY=1
  while IFS=$'\t' read -r id name policy running; do
    [[ -n "$id" ]] || continue
    docker_run update --restart=no "$id" >/dev/null
    if [[ "$running" == true ]]; then docker_run stop --time 20 "$id" >/dev/null; fi
    docker_run rename "$id" "${name}-before-${DEPLOY_START_EPOCH}-$$"
  done < "${validated}.unique"
  DEPLOY_NEW_PROJECT_NAME="${COMPOSE_PROJECT_NAME}-r${DEPLOY_START_EPOCH}-$$"
  COMPOSE_PROJECT_NAME="$DEPLOY_NEW_PROJECT_NAME"
  log "previous XrayC containers preserved; preparing a new release instance"
}

restore_preserved_installation() {
  [[ "${DEPLOY_PRESERVE_READY:-0}" == "1" ]] || return 0
  local name id project item policy running actual
  # 只处理本轮新建实例，失败容器同样保留日志；绝不执行 project down/remove-orphans。
  for name in "$agent_container" "$xray_container"; do
    id="$(docker_run inspect -f '{{.Id}}' "$name" 2>/dev/null || true)"
    [[ -n "$id" ]] || continue
    project="$(docker_run inspect -f '{{index .Config.Labels "com.docker.compose.project"}}' "$id")"
    [[ -n "${DEPLOY_NEW_PROJECT_NAME:-}" && "$project" == "$DEPLOY_NEW_PROJECT_NAME" ]] || continue
    docker_run update --restart=no "$id" >/dev/null
    docker_run stop --time 20 "$id" >/dev/null || true
    docker_run rename "$id" "${name}-failed-${DEPLOY_START_EPOCH}-$$"
  done
  for item in access-agent.env docker-compose.yml xray bin; do
    if run_root test -e "${DEPLOY_BACKUP_DIR}/${item}"; then
      run_root rm -rf "${INSTALL_DIR:?}/${item}"
      run_root cp -a "${DEPLOY_BACKUP_DIR}/${item}" "${INSTALL_DIR}/${item}"
    fi
  done
  # state/traffic-backlog 与 logs 持续保留，包括本轮新产生且尚未补送的流量。
  local records="${tmp_dir}/restore-containers.tsv"
  run_root cat "${DEPLOY_BACKUP_DIR}/containers.tsv" > "$records"
  while IFS=$'\t' read -r id name policy running; do
    [[ -n "$id" ]] || continue
    actual="$(docker_run inspect -f '{{.Name}}' "$id")"; actual="${actual#/}"
    if [[ "$actual" != "$name" ]]; then docker_run rename "$id" "$name"; fi
    docker_run update --restart="${policy:-no}" "$id" >/dev/null
    if [[ "$running" == true ]]; then docker_run start "$id" >/dev/null; fi
  done < "$records"
  log "previous installation restored; all previous and failed-container logs retained"
}
