#!/usr/bin/env bash
# 用途：确认当前 Compose 运行容器没有停留在旧镜像版本。
# 范围：比较运行容器 image id 与其声明镜像标签当前指向的 image id。

set -euo pipefail

compose_run() {
  local compose_value="${COMPOSE:-docker compose}"
  local -a compose_parts
  read -r -a compose_parts <<< "$compose_value"
  "${compose_parts[@]}" "$@"
}

short_image_id() {
  local value="$1"
  value="${value#sha256:}"
  printf '%s' "${value:0:12}"
}

failures=0

while IFS= read -r service; do
  [[ -n "$service" ]] || continue

  mapfile -t containers < <(compose_run ps -q "$service")
  if [[ "${#containers[@]}" -eq 0 ]]; then
    echo "stale-check failed: service ${service} has no running container" >&2
    failures=$((failures + 1))
    continue
  fi

  for container_id in "${containers[@]}"; do
    container_name="$(docker inspect --format '{{.Name}}' "$container_id" | sed 's#^/##')"
    image_name="$(docker inspect --format '{{.Config.Image}}' "$container_id")"
    running_image_id="$(docker inspect --format '{{.Image}}' "$container_id")"

    if ! current_image_id="$(docker image inspect --format '{{.Id}}' "$image_name" 2>/dev/null)"; then
      echo "stale-check failed: ${service}/${container_name} image ${image_name} is not present locally" >&2
      failures=$((failures + 1))
      continue
    fi

    if [[ "$running_image_id" != "$current_image_id" ]]; then
      echo "stale-check failed: ${service}/${container_name} is running old ${image_name} image $(short_image_id "$running_image_id"); current tag is $(short_image_id "$current_image_id")" >&2
      failures=$((failures + 1))
      continue
    fi

    echo "stale-check ok: ${service}/${container_name} uses ${image_name}@$(short_image_id "$running_image_id")"
  done
done < <(compose_run config --services)

if [[ "$failures" -gt 0 ]]; then
  exit 1
fi
