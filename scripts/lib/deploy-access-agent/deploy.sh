#!/usr/bin/env bash
# 用途：提供 deploy-access-agent.sh 的下载、启动、等待和回滚函数。
# 该文件只定义部署流程辅助函数，由主部署脚本 source 使用。

wait_container_running() {
  local container="$1"
  local deadline
  local state
  deadline="$(($(date +%s) + DEPLOY_READY_TIMEOUT))"
  while [[ "$(date +%s)" -le "$deadline" ]]; do
    state="$(docker_run inspect -f '{{.State.Running}}' "$container" 2>/dev/null || true)"
    if [[ "$state" == "true" ]]; then
      log "container ${container} is running"
      return 0
    fi
    sleep 2
  done
  die "container ${container} did not become running before timeout"
}

wait_agent_state() {
  if [[ -z "$EXPECTED_LISTEN_PORTS" ]]; then
    log "empty access node has no expected listen ports; accepting running agent without state version"
    return 0
  fi

  local state_file="${INSTALL_DIR}/state/state.json"
  local deadline
  local state_mtime
  deadline="$(($(date +%s) + DEPLOY_READY_TIMEOUT))"
  while [[ "$(date +%s)" -le "$deadline" ]]; do
    state_mtime="$(run_root stat -c %Y "$state_file" 2>/dev/null || printf '0')"
    if run_root test -s "$state_file" \
      && [[ "$state_mtime" =~ ^[0-9]+$ ]] \
      && ((state_mtime >= DEPLOY_START_EPOCH)) \
      && run_root grep -Eq '"config_version"[[:space:]]*:[[:space:]]*"[^"]+"' "$state_file"; then
      log "access-agent state has a desired config version"
      return 0
    fi
    sleep 2
  done
  # 就绪等待超时只 return 1(由调用方降级为告警),绝不 die：容器已 running、鉴权码已输出,
  # agent 会凭心跳继续收敛。若此处 die 会让整个安装在"快装好"时被判失败,触发自伤式重装。
  log "access-agent did not persist a desired config version before timeout; agent will keep converging via heartbeat"
  return 1
}

wait_expected_listen_ports() {
  # 期望入口端口只是“尽力等待”信号，不阻断部署：
  # 新节点刚装上 agent 时还没有任何入口（入口端口只在“有已授权用户+活跃订阅+
  # 物化出口分配”后才生成对应 Xray inbound 去 bind），此时这些端口必然还没监听。
  # 因此端口超时未监听只打告警、继续部署、不退出、不回滚——agent 已上线
  # （wait_agent_state 已保证持久化了 desired config version）即视为部署成功，
  # 具体入口端口等入口物化 + 配置同步后由 agent 自然 bind。
  # 仅对“非法端口值”仍 die（那是真错误，必须拦），不放进告警分支。
  [[ -n "$EXPECTED_LISTEN_PORTS" ]] || return 0
  ensure_ss_tooling
  local port
  local deadline
  for port in $EXPECTED_LISTEN_PORTS; do
    if ! [[ "$port" =~ ^[0-9]+$ ]] || ((port < 1 || port > 65535)); then
      die "XRAYC_EXPECTED_LISTEN_PORTS contains an invalid port"
    fi
    deadline="$(($(date +%s) + DEPLOY_READY_TIMEOUT))"
    while [[ "$(date +%s)" -le "$deadline" ]]; do
      if ss -ltn | awk '{print $4}' | grep -Eq "(^|:)${port}$"; then
        log "expected listen port ${port} is ready"
        break
      fi
      sleep 2
    done
    if ! ss -ltn | awk '{print $4}' | grep -Eq "(^|:)${port}$"; then
      # 非致命：空节点尚无入口属正常情况，端口会在入口配置下发后才监听。
      log "expected listen port ${port} not yet listening; the entry inbound may bind it only after entry config is materialized and synced, continuing without rollback"
    fi
  done
}

download_artifact() {
  local name="$1"
  local url="${DEPLOY_BASE_URL}/api/deploy/artifacts/${name}"
  local output="${ARTIFACT_DIR}/${name}"
  local tmp_output="${output}.tmp"
  local err_file="${tmp_dir}/curl-${name}.err"

  log "downloading ${name}"
  run_root rm -f "$tmp_output"
  if ! run_root curl \
    --config "$CURL_CONFIG" \
    --fail \
    --location \
    --silent \
    --show-error \
    --retry 3 \
    --connect-timeout 20 \
    --max-time "$CURL_TIMEOUT" \
    --output "$tmp_output" \
    "$url" 2> "$err_file"; then
    run_root rm -f "$tmp_output"
    die "artifact download failed for ${name}; curl stderr redacted"
  fi
  if ! run_root test -s "$tmp_output"; then
    run_root rm -f "$tmp_output"
    die "artifact download produced an empty or missing file for ${name}; curl stderr redacted"
  fi
  run_root mv "$tmp_output" "$output"
}

extract_binary() {
  local image="$1"
  local dest="$2"
  shift 2
  local candidates=("$@")
  local cid=""
  local extracted=""

  if ! cid="$(docker_run create "$image" 2> "${tmp_dir}/docker-create-image.err")"; then
    die "could not create temporary container from Docker image; stderr redacted"
  fi

  for path in "${candidates[@]}"; do
    if docker_run cp "${cid}:${path}" "$dest" >/dev/null 2>&1; then
      extracted="$path"
      break
    fi
  done

  docker_run rm "$cid" >/dev/null 2>&1 || true

  [[ -n "$extracted" ]] || die "could not extract binary from Docker image"
  log "extracted $(basename "$dest") from Docker image"
}

cleanup_existing_compose_project_for_force_reinstall() {
  [[ "${OVERWRITE_RUNTIME_CONFIG:-false}" == "true" ]] || return 0

  log "cleaning existing Docker Compose project before forced reinstall"
  if [[ -f "${INSTALL_DIR}/docker-compose.yml" ]]; then
    compose_run -f "${INSTALL_DIR}/docker-compose.yml" -p "$COMPOSE_PROJECT_NAME" down --remove-orphans >/dev/null 2>&1 || true
  fi

  local stale_containers
  stale_containers="$(docker_run ps -aq --filter "label=com.docker.compose.project=${COMPOSE_PROJECT_NAME}" 2>/dev/null || true)"
  if [[ -n "$stale_containers" ]]; then
    # 同一 compose project 多次换节点 ID 时，容器名会跟随节点 ID 变化。
    # 按 compose project 标签清理，避免旧 Xray 继续占用 stats API 或入口端口。
    # shellcheck disable=SC2086
    docker_run rm -f $stale_containers >/dev/null 2>&1 || true
  fi
}

cleanup_existing_install_files_for_force_reinstall() {
  [[ "${OVERWRITE_RUNTIME_CONFIG:-false}" == "true" ]] || return 0

  log "removing existing runtime files before forced reinstall"
  run_root rm -rf \
    "$ARTIFACT_DIR" \
    "${INSTALL_DIR}/bin" \
    "$XRAY_HOST_CONFIG_DIR" \
    "$XRAY_HOST_LOG_DIR" \
    "${INSTALL_DIR}/state" \
    "${INSTALL_DIR}/access-agent.env" \
    "${INSTALL_DIR}/docker-compose.yml" \
    >/dev/null 2>&1 || true
}

cleanup_legacy_compose_projects() {
  [[ "${CLEAN_LEGACY_COMPOSE_PROJECTS:-true}" == "true" ]] || return 0

  local legacy_project="xrayc-access"
  if [[ "$COMPOSE_PROJECT_NAME" == "$legacy_project" ]]; then
    return 0
  fi

  local legacy_dir="/opt/xrayc/access-agent"
  if [[ -f "${legacy_dir}/docker-compose.yml" ]]; then
    log "cleaning legacy Docker Compose project"
    compose_run -f "${legacy_dir}/docker-compose.yml" -p "$legacy_project" down --remove-orphans >/dev/null 2>&1 || true
  fi

  local legacy_containers
  legacy_containers="$(docker_run ps -aq --filter "label=com.docker.compose.project=xrayc-access" 2>/dev/null || true)"
  if [[ -n "$legacy_containers" ]]; then
    log "removing legacy Docker Compose containers"
    # shellcheck disable=SC2086
    docker_run rm -f $legacy_containers >/dev/null 2>&1 || true
  fi
}

rollback_failed_deploy() {
  [[ "$DEPLOY_ROLLBACK_ON_FAILURE" == "true" ]] || return 0

  log "deployment failed; rolling back files and containers created by this run"
  if [[ "$DEPLOY_COMPOSE_UP_ATTEMPTED" == "1" && -n "${COMPOSE_PROJECT_NAME:-}" && -f "${INSTALL_DIR}/docker-compose.yml" ]]; then
    compose_run -f "${INSTALL_DIR}/docker-compose.yml" -p "$COMPOSE_PROJECT_NAME" down --remove-orphans >/dev/null 2>&1 || true
  fi
  if [[ "$DEPLOY_CONTAINER_NAMES_TOUCHED" == "1" && ( -n "${agent_container:-}" || -n "${xray_container:-}" ) ]]; then
    docker_run rm -f ${agent_container:+"$agent_container"} ${xray_container:+"$xray_container"} >/dev/null 2>&1 || true
  fi
  if [[ "$DEPLOY_CREATED_INSTALL_DIR" == "1" || "$DEPLOY_ROLLBACK_REMOVE_EXISTING" == "true" ]]; then
    run_root rm -rf "$INSTALL_DIR" >/dev/null 2>&1 || true
  else
    log "install directory existed before deploy; leaving files in place unless XRAYC_DEPLOY_ROLLBACK_REMOVE_EXISTING=true"
  fi
}

cleanup_all() {
  local status="${1:-$?}"
  if [[ "$status" -ne 0 ]]; then
    rollback_failed_deploy
  fi
  rm -rf "$tmp_dir"
  exit "$status"
}
