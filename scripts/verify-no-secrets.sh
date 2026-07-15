#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCAN_DIRS=(
  "$ROOT_DIR/crates"
  "$ROOT_DIR/docs"
  "$ROOT_DIR/文档"
  "$ROOT_DIR/scripts"
  "$ROOT_DIR/deploy"
  "$ROOT_DIR/migrations"
  "$ROOT_DIR/frontend/src"
  "$ROOT_DIR/frontend/e2e"
)
SCAN_FILES=(
  "$ROOT_DIR/开发方案.md"
  "$ROOT_DIR/README.md"
  "$ROOT_DIR/CHANGELOG.md"
  "$ROOT_DIR/Cargo.toml"
  "$ROOT_DIR/Cargo.lock"
  "$ROOT_DIR/Dockerfile"
  "$ROOT_DIR/.dockerignore"
  "$ROOT_DIR/.gitignore"
  "$ROOT_DIR/Makefile"
  "$ROOT_DIR/docker-compose.yml"
  "$ROOT_DIR/.env.example"
  "$ROOT_DIR/.env.real-release.example"
  "$ROOT_DIR/deploy/caddy/Dockerfile"
  "$ROOT_DIR/deploy/caddy/Caddyfile"
  "$ROOT_DIR/frontend/playwright.config.ts"
  "$ROOT_DIR/frontend/vite.config.ts"
  "$ROOT_DIR/frontend/package.json"
  "$ROOT_DIR/frontend/package-lock.json"
)
EXCLUDE_DIRS=(
  "$ROOT_DIR/文档/私有"
  "$ROOT_DIR/deploy/artifacts"
)
REQUIRED_IGNORED_PATHS=(
  "AGENTS.md"
  "env"
  "文档/私有/"
  "服务器账号密码.txt"
)
MATCH_FILE="$(mktemp)"

failures=0

cleanup() {
  rm -f "$MATCH_FILE"
}
trap cleanup EXIT

report() {
  local message="$1"
  echo "verify-no-secrets: ${message}" >&2
  failures=$((failures + 1))
}

print_redacted_matches() {
  awk -F: '{ print "  line " $1 ": <redacted finding>" }' "$MATCH_FILE" >&2
}

check_pattern() {
  local file="$1"
  local rel="$2"
  local message="$3"
  local pattern="$4"

  if grep -En -- "$pattern" "$file" >"$MATCH_FILE" 2>/dev/null; then
    report "${message} in ${rel}"
    print_redacted_matches
  fi
}

filter_placeholder_secret_matches() {
  grep -Eiv -- '[:=][[:space:]]*["'\'']?(demo-agent-token|admin123456|demo123456|change-me|change_me|replace-me|replace_me|example[^"'\''[:space:],;]*|sample[^"'\''[:space:],;]*|test[^"'\''[:space:],;]*|dummy[^"'\''[:space:],;]*|placeholder[^"'\''[:space:],;]*|fake[^"'\''[:space:],;]*|mock[^"'\''[:space:],;]*|local[^"'\''[:space:],;]*|redacted|worker-smoke[^"'\''[:space:],;]*|xrayc-node-test-token|trojan-secret|admin-smoke-token|secret-agent-token|super-secret-password|full-token|<set-in-env>|<password>|<token>|<secret>|your-secret|your-token|your-password)(["'\'',;[:space:]]|$)' \
    | grep -Eiv -- '[:=][[:space:]]*["'\'']?(replace-with-[^"'\''[:space:],;]+|replace_with_[^"'\''[:space:],;]+|your-[^"'\''[:space:],;]+|your_[^"'\''[:space:],;]+)(["'\'',;[:space:]]|$)' \
    | grep -Eiv -- '[:=][[:space:]]*["'\'']?[A-Za-z0-9_./:@+=,-]*(change[-_]?me|replace[-_]?me|replace[-_]?with|placeholder|redacted)[A-Za-z0-9_./:@+=,-]*(["'\'',;[:space:]]|$)' \
    | grep -Eiv -- '[?&](token|access_?token|refresh_?token|api_?key|secret|password|passwd)=((demo|example|sample|test|dummy|placeholder|fake|mock|redacted|change[-_]?me|replace[-_]?me|your[-_][^&[:space:]]+)[^&[:space:]]*|<[^>]+>)(&|[[:space:]]|$)' \
    | grep -Eiv -- '(default_test_field_encryption_keys=|password_hash|token_hash|target_hash)' \
    | grep -Eiv -- '[:=][[:space:]]*["'\'']?(md|cf|blank|unit)(-[a-z0-9]+)*-(token|secret-marker)["'\'']?(["'\'',;[:space:])]|\.(into|to_string|to_owned)|$)' \
    | grep -Ev -- '(密码|口令|密钥|令牌|凭据|账号密码)(泄露|管理|仅[^[:space:]]*使用|[^[:space:]]*）)' \
    | grep -Ev -- '[:：][[:space:]]*[a-z_]+[a-z0-9_]*=[A-Za-z0-9_./-]+([[:space:],;）)]|$)'
}

filter_placeholder_url_matches() {
  # 先放过显式占位词，再放过 shell/compose 的 ${VAR} 插值与 <angle> 占位（如 ${POSTGRES_PASSWORD}/${pw}/<pw>/<DB_IP>）。
  # 这些只是变量引用或文档占位，真实口令由部署期注入，绝非硬编码凭据；不放过任何普通字面口令。
  grep -Eiv -- '(change[-_]?me|replace[-_]?me|replace[-_]?with|example|sample|test|dummy|placeholder|fake|mock|redacted|<set-in-env>|<password>|<token>|<secret>|your-secret|your-token|your-password)' \
    | grep -Ev -- '://[^[:space:]:@/]+:(\$\{[A-Za-z_][A-Za-z0-9_]*\}|<[^>@/[:space:]]+>)@'
}

check_required_ignored_paths() {
  if ! git -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    return
  fi

  for path in "${REQUIRED_IGNORED_PATHS[@]}"; do
    if ! git -C "$ROOT_DIR" check-ignore --no-index -q -- "$path"; then
      report "required private path is not covered by .gitignore: ${path}"
    fi
    if ! grep -Fxq "$path" "$ROOT_DIR/.dockerignore"; then
      report "required private path is not covered by .dockerignore: ${path}"
    fi
  done
}

check_tracked_secret_paths() {
  if ! git -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    return
  fi

  local path base
  while IFS= read -r -d '' path; do
    base="${path##*/}"
    case "$base" in
      .env.example|.env.real-release.example)
        continue
        ;;
    esac
    case "$path" in
      AGENTS.md|AGENTS.local.md|.env|.env.*|env|.envrc|*/.env|*/.env.*|*/env|*/.envrc|real-e2e-inventory.*|*.inventory.private.*|*.private.md|*.secrets.md|*账号密码*.txt|secrets/*|*/secrets/*|文档/私有/*)
        report "tracked private/env path is present in Git index"
        ;;
    esac
  done < <(git -C "$ROOT_DIR" ls-files -z)
}

scan_file() {
  local file="$1"
  local rel="${file#$ROOT_DIR/}"

  if grep -Iq . "$file"; then
    if {
      grep -EIn -- '([A-Za-z0-9_.-]*?(password|passwd|secret|token|private[_-]?key|access[_-]?key|api[_-]?key|auth[_-]?key|client[_-]?secret)[A-Za-z0-9_.-]*?)[[:space:]]*[:=][[:space:]]*["'\''][A-Za-z0-9_./:@+=,-]{12,}["'\'']' "$file" 2>/dev/null || true
      grep -EIn -- '^[[:space:]]*(export[[:space:]]+)?[A-Z][A-Z0-9_]*?(PASSWORD|PASSWD|SECRET|TOKEN|PRIVATE_KEY|ACCESS_KEY|API_KEY|AUTH_KEY|CLIENT_SECRET)[A-Z0-9_]*[[:space:]]*=[[:space:]]*[A-Za-z0-9_./:@+=,-]{16,}([[:space:]#]|$)' "$file" 2>/dev/null || true
      grep -EIn -- '^[[:space:]]*[A-Za-z0-9_.-]*?(password|passwd|secret|token|private[_-]?key|access[_-]?key|api[_-]?key|auth[_-]?key|client[_-]?secret)[A-Za-z0-9_.-]*?[[:space:]]*:[[:space:]]*[A-Za-z0-9_+/@=-]{16,}([[:space:]#,}]|$)' "$file" 2>/dev/null || true
      grep -EIn -- '(密码|口令|密钥|令牌|凭据|账号密码)[^[:space:]]{0,24}[[:space:]]*[:：=][[:space:]]*["'\'']?[A-Za-z0-9_./:@+=,-]{8,}["'\'']?' "$file" 2>/dev/null || true
      grep -EIn -- '[?&](access_?token|refresh_?token|token|api_?key|secret|password|passwd)=[A-Za-z0-9._~+/=-]{12,}' "$file" 2>/dev/null || true
      grep -EIn -- 'Authorization[[:space:]]*[:=][[:space:]]*["'\'']?(Bearer|Basic)[[:space:]]+[A-Za-z0-9._~+/=-]{12,}' "$file" 2>/dev/null || true
    } \
      | filter_placeholder_secret_matches >"$MATCH_FILE" 2>/dev/null; then
      report "possible assigned secret in ${rel}"
      print_redacted_matches
    fi
    check_pattern "$file" "$rel" "private key block marker" '-----BEGIN [A-Z ]*PRIVATE KEY-----'
    if grep -En -- 'postgres(ql)?://[^[:space:]]+:[^[:space:]@]+@' "$file" | filter_placeholder_url_matches >"$MATCH_FILE" 2>/dev/null; then
      report "database URL with inline password in ${rel}"
      print_redacted_matches
    fi
    if grep -En -- 'https?://[^[:space:]/:]+:[^[:space:]@]+@' "$file" | filter_placeholder_url_matches >"$MATCH_FILE" 2>/dev/null; then
      report "URL with inline credentials in ${rel}"
      print_redacted_matches
    fi
    if grep -En -- 'Bearer[[:space:]]+[A-Za-z0-9._~+/=-]{20,}' "$file" | filter_placeholder_url_matches >"$MATCH_FILE" 2>/dev/null; then
      report "bearer token literal in ${rel}"
      print_redacted_matches
    fi
    check_pattern "$file" "$rel" "OpenAI-style API key literal" 'sk-[A-Za-z0-9_-]{20,}'
    check_pattern "$file" "$rel" "JWT literal" 'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'

    case "$rel" in
      scripts/*.sh)
        check_pattern "$file" "$rel" "unsafe psql DATABASE_URL command argument" 'psql[[:space:]]+"\$(DATABASE_URL|database_url)"'
        check_pattern "$file" "$rel" "unsafe curl request body command argument" '--data([[:space:]]|=)["'\'']?\$[A-Za-z_]*(PASSWORD|TOKEN|SECRET|DATABASE_URL|body|json_body)'
        check_pattern "$file" "$rel" "unsafe Python sensitive command argument" 'python3[[:space:]]+-[[:space:]].*"\$[A-Za-z_]*(PASSWORD|password|DATABASE_URL|database_url)"'
        ;;
    esac

    # 公共解析器与文档示例 IP：8.8.8.8/8.8.4.4(Google)、1.1.1.1/1.0.0.1(Cloudflare DNS)、1.2.3.4(文档占位)、
    # 223.5.5.5/223.6.6.6(阿里)、119.29.29.29(腾讯)、114.114.114.114/114.114.115.115(114DNS)：订阅内置 dns 段用的公开解析器。
    # 这些是公开常量，用于 dig 解析、单测断言、订阅 dns 配置或示例命令，绝非私有主机地址，逐条确认后放行。
    local public_dns_or_doc_ip='(^|[^0-9])(8\.8\.8\.8|8\.8\.4\.4|1\.1\.1\.1|1\.0\.0\.1|1\.2\.3\.4|223\.5\.5\.5|223\.6\.6\.6|119\.29\.29\.29|114\.114\.114\.114|114\.114\.115\.115)([^0-9]|$)'
    # fake-ip 池与保留段：198.18.0.0/15(RFC 2544 基准测试，mihomo fake-ip-range)、240.0.0.0/4(RFC 1112 保留，fallback-filter ipcidr)，均为 Clash 配置常量、非真实主机。
    local clash_reserved_ip='(^|[^0-9])(198\.18\.[0-9]{1,3}\.[0-9]{1,3}|198\.19\.[0-9]{1,3}\.[0-9]{1,3}|240\.0\.0\.0)([^0-9]|$)'
    # Cloudflare 官方公开 IPv4 回源段(https://www.cloudflare.com/ips/)：仅用于 CDN 段判定的单测/CIDR 文档，是公开常量。
    local cloudflare_public_cidr_or_host='(173\.245\.48\.|103\.21\.244\.|103\.22\.200\.|103\.31\.4\.|141\.101\.6[4-9]\.|141\.101\.[7-9][0-9]\.|141\.101\.1[01][0-9]\.|141\.101\.12[0-7]\.|108\.162\.(19[2-9]|2[0-4][0-9]|25[0-5])\.|108\.162\.1[6-8][0-9]\.|190\.93\.24[0-9]\.|190\.93\.25[0-5]\.|188\.114\.9[6-9]\.|188\.114\.1[01][0-9]\.|197\.234\.24[0-3]\.|198\.41\.(12[8-9]|1[3-9][0-9]|2[0-4][0-9]|25[0-5])\.|162\.158\.|162\.159\.|104\.1[6-9]\.|104\.2[0-7]\.|172\.6[4-9]\.|172\.7[0-1]\.|131\.0\.7[2-5]\.)'
    if grep -En '([0-9]{1,3}\.){3}[0-9]{1,3}' "$file" \
      | grep -Ev '(^|[^0-9])(127\.0\.0\.1|127\.0\.0\.11|0\.0\.0\.0|192\.0\.2\.[0-9]{1,3}|198\.51\.100\.[0-9]{1,3}|203\.0\.113\.[0-9]{1,3})([^0-9]|$)' \
      | grep -Ev 'IP-CIDR,(127\.0\.0\.0/8|10\.0\.0\.0/8|172\.16\.0\.0/12|192\.168\.0\.0/16|100\.64\.0\.0/10|224\.0\.0\.0/4),' \
      | grep -Ev -- "$public_dns_or_doc_ip" \
      | grep -Ev -- "$clash_reserved_ip" \
      | grep -Ev -- "$cloudflare_public_cidr_or_host" >"$MATCH_FILE" 2>/dev/null; then
      report "possible real IPv4 address in ${rel}"
      print_redacted_matches
    fi
  fi
}

check_required_ignored_paths
check_tracked_secret_paths

for file in "${SCAN_FILES[@]}"; do
  if [[ -f "$file" ]]; then
    scan_file "$file"
  fi
done

while IFS= read -r -d '' file; do
  scan_file "$file"
done < <(
  find "${SCAN_DIRS[@]}" \
    \( -path "${EXCLUDE_DIRS[0]}" -o -path "${EXCLUDE_DIRS[1]}" \) -prune -o \
    -type f -print0
)

if [[ "$failures" -gt 0 ]]; then
  echo "verify-no-secrets: failed with ${failures} finding(s)" >&2
  exit 1
fi

echo "verify-no-secrets: passed"
