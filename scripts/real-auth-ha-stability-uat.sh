#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# 认证多副本长稳 UAT 调度脚本。
# 该脚本复用 real-auth-ha-uat.sh 的真实多副本认证闭环，并按持续时长
# 重复执行，验证验证码、Refresh Token、登录锁和共享频控在长时间内稳定。

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$BASE_DIR"

usage() {
  cat <<'USAGE'
usage: bash scripts/real-auth-ha-stability-uat.sh

Runs repeated multi-replica auth-security UAT for a long stability window.
The script never prints passwords, tokens, cookies, response bodies, URLs, IPs,
or database connection strings.

Environment:
  AUTH_HA_STABILITY_PROFILE              10m, 6h or 24h. Default: 10m.
  AUTH_HA_STABILITY_DURATION_SECONDS     Default: 600 for 10m, 21600 for 6h, 86400 for 24h.
  AUTH_HA_STABILITY_INTERVAL_SECONDS     Delay after each completed round. Default: 120 for 10m, 900 otherwise.
  AUTH_HA_STABILITY_MIN_ROUNDS           Minimum completed rounds. Default: 2.
  AUTH_HA_STABILITY_VALIDATE_ONLY=1      Validate profile and auth UAT env only.

All variables accepted by scripts/real-auth-ha-uat.sh are forwarded, including:
  UAT_COMPOSE_START, UAT_COMPOSE_NO_BUILD, UAT_MIN_API_REPLICAS,
  BASE_URL, ADMIN_ACCESS_TOKEN, ADMIN_LOGIN_ACCOUNT, ADMIN_LOGIN_PASSWORD,
  E2E_ADMIN_ACCOUNT, E2E_ADMIN_PASSWORD, DATABASE_URL, UAT_PSQL_DATABASE_URL.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

REAL_RELEASE_ENV_FILE="${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"
if [[ -f "$REAL_RELEASE_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$REAL_RELEASE_ENV_FILE"
  set +a
fi

if [[ -n "${XRAYC_AUTH_HA_FORCE_PROFILE:-}" ]]; then
  AUTH_HA_STABILITY_PROFILE="$XRAYC_AUTH_HA_FORCE_PROFILE"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_DURATION_SECONDS:-}" ]]; then
  AUTH_HA_STABILITY_DURATION_SECONDS="$XRAYC_AUTH_HA_FORCE_DURATION_SECONDS"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_INTERVAL_SECONDS:-}" ]]; then
  AUTH_HA_STABILITY_INTERVAL_SECONDS="$XRAYC_AUTH_HA_FORCE_INTERVAL_SECONDS"
fi
if [[ -n "${XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY:-}" ]]; then
  AUTH_HA_STABILITY_VALIDATE_ONLY="$XRAYC_AUTH_HA_FORCE_VALIDATE_ONLY"
fi

bool_is_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

positive_int() {
  [[ "${1:-}" =~ ^[1-9][0-9]*$ ]]
}

profile="${AUTH_HA_STABILITY_PROFILE:-10m}"
case "$profile" in
  10m)
    default_duration=600
    default_interval=120
    ;;
  6h)
    default_duration=21600
    default_interval=900
    ;;
  24h)
    default_duration=86400
    default_interval=900
    ;;
  *)
    echo "AUTH_HA_STABILITY_PROFILE must be 10m, 6h or 24h" >&2
    exit 2
    ;;
esac

duration="${AUTH_HA_STABILITY_DURATION_SECONDS:-$default_duration}"
interval="${AUTH_HA_STABILITY_INTERVAL_SECONDS:-$default_interval}"
min_rounds="${AUTH_HA_STABILITY_MIN_ROUNDS:-2}"
validate_only="${AUTH_HA_STABILITY_VALIDATE_ONLY:-0}"

positive_int "$duration" || { echo "AUTH_HA_STABILITY_DURATION_SECONDS must be a positive integer" >&2; exit 2; }
positive_int "$interval" || { echo "AUTH_HA_STABILITY_INTERVAL_SECONDS must be a positive integer" >&2; exit 2; }
positive_int "$min_rounds" || { echo "AUTH_HA_STABILITY_MIN_ROUNDS must be a positive integer" >&2; exit 2; }

if [[ "$profile" == "10m" && "$duration" -lt 600 ]]; then
  echo "10m auth HA stability UAT requires duration >= 600" >&2
  exit 2
fi
if [[ "$profile" == "6h" && "$duration" -lt 21600 ]]; then
  echo "6h auth HA stability UAT requires duration >= 21600" >&2
  exit 2
fi
if [[ "$profile" == "24h" && "$duration" -lt 86400 ]]; then
  echo "24h auth HA stability UAT requires duration >= 86400" >&2
  exit 2
fi

UAT_VALIDATE_ONLY=1 bash scripts/real-auth-ha-uat.sh >/dev/null
if bool_is_true "$validate_only"; then
  echo "auth HA stability UAT validation passed"
  exit 0
fi

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
deadline=$((SECONDS + duration))
round=0
failures=0

echo "auth HA stability UAT started profile=${profile} duration_seconds=${duration} started_at=${started_at}"
while [[ "$SECONDS" -lt "$deadline" || "$round" -lt "$min_rounds" ]]; do
  round=$((round + 1))
  round_started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if UAT_VALIDATE_ONLY=0 bash scripts/real-auth-ha-uat.sh >/dev/null; then
    echo "auth HA stability round ${round}: passed at ${round_started_at}"
  else
    failures=$((failures + 1))
    echo "auth HA stability round ${round}: failed at ${round_started_at}" >&2
  fi
  [[ "$SECONDS" -ge "$deadline" && "$round" -ge "$min_rounds" ]] && break
  sleep "$interval"
done

if [[ "$failures" -gt 0 ]]; then
  echo "auth HA stability UAT failed; failure_count=${failures}" >&2
  exit 1
fi

echo "auth HA stability UAT completed rounds=${round} finished_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
