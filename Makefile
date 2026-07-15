COMPOSE ?= docker compose
ARTIFACT_DIR ?= deploy/artifacts
APP_IMAGE ?= xrayc/rust-app:local
ACCESS_AGENT_IMAGE ?= xrayc/access-agent:local
XRAY_UPSTREAM_IMAGE ?= ghcr.io/xtls/xray-core:26.5.9
XRAY_LOCAL_IMAGE ?= xrayc/xray:local
XRAY_IMAGE_PLATFORM ?= linux/amd64
TEST_FIELD_ENCRYPTION_KEYS ?= new:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ,old:AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM,test:unit-test-field-key

.PHONY: fmt clippy test test-lib test-api test-db frontend-install frontend-build frontend-ci-build check-plan check-plan-run check-affected check-fast check-smoke check-runtime check-docker-runtime check-postgres-integration check-worker-smoke check-running-current check-release check-real-release check-real-release-env check-real-subscription-client-compat check-real-release-gate-profiles real-release-gap-report report-real-release-env-gaps bootstrap-real-release-env prepare-real-e2e-auth-env prepare-real-protocol-matrix-endpoints prepare-real-protocol-matrix-control-plane real-access-third-party-matrix-relay-e2e observe-real-client-env real-auth-ha-uat real-auth-ha-stability-uat runtime-loadtest-smoke runtime-loadtest-large runtime-http-loadtest-smoke runtime-http-loadtest-large runtime-http-loadtest-compose real-runtime-stability-uat ops-mistake-recovery-uat ops-mistake-recovery-large-uat check check-no-legacy check-source-file-length check-real-release-assets-docs check-agent-install-cleanup check-v2-deploy-contract compose-check package-docker-artifacts up rebuild rebuild-backend rebuild-caddy rebuild-frontend restart down

fmt:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" cargo test --workspace

test-lib:
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" cargo test --workspace --lib

test-api:
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" cargo test -p xrayc-api

test-db:
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" cargo test -p xrayc-db

frontend-install:
	cd frontend && npm ci

frontend-build:
	cd frontend && npm run build

frontend-ci-build:
	cd frontend && npm ci && npm run build

check-plan:
	bash scripts/check-plan.sh

check-plan-run:
	bash scripts/check-plan.sh --run

check-affected:
	bash scripts/check-plan.sh --run

check-fast: fmt test-lib
	cd frontend && npm run build

check-smoke: check-running-current
	for script in scripts/real-*.sh scripts/ops-mistake-recovery-uat.sh scripts/ops-mistake-recovery-large-uat.sh scripts/runtime-loadtest.sh scripts/runtime-http-loadtest.sh scripts/runtime-http-loadtest-compose.sh scripts/api-contract-smoke-draft.sh scripts/bootstrap-real-release-env.sh scripts/prepare-real-e2e-auth-env.sh scripts/prepare-real-protocol-matrix-assets.sh scripts/prepare-real-protocol-matrix-endpoints.sh scripts/prepare-real-protocol-matrix-control-plane.sh scripts/check-real-protocol-matrix-env.sh scripts/observe-real-client-env.sh scripts/check-real-release.sh scripts/check-real-release-env.sh scripts/check-real-subscription-client-compat.sh scripts/prepare-real-release-env.sh scripts/validate-real-release-env.sh scripts/check-real-release-assets-docs.sh scripts/check-agent-install-cleanup.sh scripts/check-v2-deploy-contract.sh scripts/verify-no-secrets.sh scripts/deploy-access-agent.sh scripts/test-real-v2-inventory-fallback.sh scripts/test-real-test-server-assets-static.sh scripts/test-real-remote-install-smoke-env.sh scripts/test-real-v2-plan-binding.sh scripts/test-real-multi-user-traffic-uat-static.sh scripts/test-real-subscription-client-compat-static.sh scripts/test-real-access-inbound-matrix-static.sh scripts/test-real-third-party-redaction.sh; do bash -n "$$script"; done
	find scripts/lib -name '*.sh' -print0 | xargs -0 -r -n1 bash -n
	bash scripts/verify-no-secrets.sh
	bash scripts/check-real-release-assets-docs.sh
	bash scripts/check-agent-install-cleanup.sh
	bash scripts/check-v2-deploy-contract.sh
	bash scripts/test-real-multi-user-traffic-uat-static.sh
	bash scripts/test-real-subscription-client-compat-static.sh
	bash scripts/test-real-test-server-assets-static.sh
	bash scripts/test-real-access-inbound-matrix-static.sh
	BASE_URL=$${BASE_URL:-http://127.0.0.1:8080} SMOKE_LOGIN_ACCOUNT=$${SMOKE_LOGIN_ACCOUNT:-demo@example.test} SMOKE_LOGIN_PASSWORD=$${SMOKE_LOGIN_PASSWORD:-demo123456} bash scripts/real-smoke.sh
	STRICT_CONTRACT=1 BASE_URL=$${BASE_URL:-http://127.0.0.1:8080} CONTRACT_USER_ACCOUNT=$${CONTRACT_USER_ACCOUNT:-$${SMOKE_LOGIN_ACCOUNT:-demo@example.test}} CONTRACT_USER_PASSWORD=$${CONTRACT_USER_PASSWORD:-$${SMOKE_LOGIN_PASSWORD:-demo123456}} CONTRACT_ADMIN_ACCOUNT=$${CONTRACT_ADMIN_ACCOUNT:-admin@example.test} CONTRACT_ADMIN_PASSWORD=$${CONTRACT_ADMIN_PASSWORD:-admin123456} bash scripts/api-contract-smoke-draft.sh

check-runtime: check-running-current check-smoke
	cd frontend && E2E_BASE_URL=$${E2E_BASE_URL:-http://127.0.0.1:8080} E2E_USER_ACCOUNT=$${E2E_USER_ACCOUNT:-demo@example.test} E2E_USER_PASSWORD=$${E2E_USER_PASSWORD:-demo123456} E2E_ADMIN_ACCOUNT=$${E2E_ADMIN_ACCOUNT:-admin@example.test} E2E_ADMIN_PASSWORD=$${E2E_ADMIN_PASSWORD:-admin123456} npm run test:e2e

check-docker-runtime:
	@set -eu; \
	project="xrayc_runtime_$$(date +%s)_$$$$"; \
	ports="$$(python3 -c "import socket; ss=[socket.socket(), socket.socket(), socket.socket(), socket.socket()]; [s.bind(('127.0.0.1', 0)) for s in ss]; print(ss[0].getsockname()[1], ss[1].getsockname()[1], ss[2].getsockname()[1], ss[3].getsockname()[1])")"; \
	set -- $$ports; \
	public_http_port="$$1"; \
	http_port="$$2"; \
	https_port="$$3"; \
	postgres_port="$$4"; \
	cleanup() { \
		COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" $(COMPOSE) down -v --remove-orphans >/dev/null 2>&1 || true; \
	}; \
	trap cleanup EXIT INT TERM; \
	echo "compose project: $$project"; \
	echo "http port: $$http_port"; \
	COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" SEED_DEMO_DATA=true XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" $(COMPOSE) up -d --build --remove-orphans; \
	timeout_seconds="$${DOCKER_RUNTIME_HEALTH_TIMEOUT_SECONDS:-240}"; \
	interval_seconds="$${DOCKER_RUNTIME_HEALTH_POLL_INTERVAL_SECONDS:-5}"; \
	elapsed=0; \
	while :; do \
		not_ready=0; \
		for service in $$(COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" $(COMPOSE) config --services); do \
			cid="$$(COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" $(COMPOSE) ps -q "$$service")"; \
			status="$$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$$cid" 2>/dev/null || printf missing)"; \
			case "$$status" in healthy|running) ;; *) not_ready=1; echo "waiting for $$service: $$status";; esac; \
		done; \
		if [ "$$not_ready" -eq 0 ]; then break; fi; \
		if [ "$$elapsed" -ge "$$timeout_seconds" ]; then \
			COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" $(COMPOSE) ps; \
			COMPOSE_PROJECT_NAME="$$project" PUBLIC_HTTP_PORT="127.0.0.1:$$public_http_port" HTTP_PORT="127.0.0.1:$$http_port" HTTPS_PORT="127.0.0.1:$$https_port" POSTGRES_PORT="$$postgres_port" $(COMPOSE) logs --tail=120; \
			exit 1; \
		fi; \
		sleep "$$interval_seconds"; \
		elapsed=$$((elapsed + interval_seconds)); \
	done; \
	base_url="http://127.0.0.1:$$http_port"; \
	user_account="$${DOCKER_RUNTIME_USER_ACCOUNT:-demo@example.test}"; \
	user_password="$${DOCKER_RUNTIME_USER_PASSWORD:-demo123456}"; \
	admin_account="$${DOCKER_RUNTIME_ADMIN_ACCOUNT:-admin@example.test}"; \
	admin_password="$${DOCKER_RUNTIME_ADMIN_PASSWORD:-admin123456}"; \
	BASE_URL="$$base_url" SMOKE_LOGIN_ACCOUNT="$$user_account" SMOKE_LOGIN_PASSWORD="$$user_password" bash scripts/real-smoke.sh; \
	STRICT_CONTRACT=1 BASE_URL="$$base_url" CONTRACT_USER_ACCOUNT="$$user_account" CONTRACT_USER_PASSWORD="$$user_password" CONTRACT_ADMIN_ACCOUNT="$$admin_account" CONTRACT_ADMIN_PASSWORD="$$admin_password" bash scripts/api-contract-smoke-draft.sh; \
	cd frontend && E2E_BASE_URL="$$base_url" E2E_USER_ACCOUNT="$$user_account" E2E_USER_PASSWORD="$$user_password" E2E_ADMIN_ACCOUNT="$$admin_account" E2E_ADMIN_PASSWORD="$$admin_password" npx playwright test e2e/smoke.spec.ts e2e/runtime-no-mock.spec.ts e2e/smoke/admin-users.spec.ts e2e/smoke/admin-traffic-logs.spec.ts

check-postgres-integration:
	@set -eu; \
	project="xrayc_pgtest_$$(date +%s)_$$$$"; \
	postgres_port="$$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()")"; \
	cleanup() { \
		COMPOSE_PROJECT_NAME="$$project" POSTGRES_PORT="$$postgres_port" $(COMPOSE) down -v --remove-orphans >/dev/null 2>&1 || true; \
	}; \
	trap cleanup EXIT INT TERM; \
	echo "postgres integration project: $$project"; \
	COMPOSE_PROJECT_NAME="$$project" POSTGRES_PORT="$$postgres_port" POSTGRES_DB=xrayc_test $(COMPOSE) up -d postgres; \
	timeout_seconds="$${POSTGRES_INTEGRATION_HEALTH_TIMEOUT_SECONDS:-120}"; \
	interval_seconds="$${POSTGRES_INTEGRATION_HEALTH_POLL_INTERVAL_SECONDS:-3}"; \
	elapsed=0; \
	while :; do \
		cid="$$(COMPOSE_PROJECT_NAME="$$project" POSTGRES_PORT="$$postgres_port" $(COMPOSE) ps -q postgres)"; \
		status="$$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$$cid" 2>/dev/null || printf missing)"; \
		if [ "$$status" = "healthy" ]; then break; fi; \
		if [ "$$elapsed" -ge "$$timeout_seconds" ]; then \
			COMPOSE_PROJECT_NAME="$$project" POSTGRES_PORT="$$postgres_port" $(COMPOSE) ps; \
			COMPOSE_PROJECT_NAME="$$project" POSTGRES_PORT="$$postgres_port" $(COMPOSE) logs --tail=120 postgres; \
			exit 1; \
		fi; \
		echo "waiting for postgres: $$status"; \
		sleep "$$interval_seconds"; \
		elapsed=$$((elapsed + interval_seconds)); \
	done; \
	db_url="postgres://xrayc:change-me@127.0.0.1:$$postgres_port/xrayc_test"; \
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" DATABASE_URL="$$db_url" cargo test -p xrayc-db test_pg_ -- --test-threads=1; \
	XRAYC_FIELD_ENCRYPTION_KEYS="$(TEST_FIELD_ENCRYPTION_KEYS)" DATABASE_URL="$$db_url" cargo test -p xrayc-api test_pg_ -- --test-threads=1

check-worker-smoke: check-docker-runtime
	WORKER_SMOKE_BUILD=0 bash scripts/real-worker-smoke.sh

check-running-current:
	bash scripts/check-running-images-current.sh

check-release: check check-postgres-integration check-worker-smoke compose-check

check-real-release: check-real-release-env package-docker-artifacts check-deploy-artifact-endpoint
	real_env="$${XRAYC_REAL_RELEASE_ENV_FILE:-.env.real-release}"; \
	case "$$real_env" in /*|*/*) real_env_path="$$real_env" ;; *) real_env_path="./$$real_env" ;; esac; \
	set -a; . "$$real_env_path"; set +a; \
	DATABASE_URL="$${XRAYC_REAL_RELEASE_COMPOSE_DATABASE_URL:-postgres://$${POSTGRES_USER:-xrayc}:$${POSTGRES_PASSWORD:-change-me}@postgres:5432/$${POSTGRES_DB:-xrayc}}" \
	$(COMPOSE) --env-file "$$real_env_path" up -d --no-build api worker caddy --remove-orphans
	bash scripts/check-running-images-current.sh
	bash scripts/check-real-release.sh

check: fmt clippy test frontend-build check-no-legacy check-source-file-length
	for script in scripts/real-*.sh scripts/ops-mistake-recovery-uat.sh scripts/ops-mistake-recovery-large-uat.sh scripts/runtime-loadtest.sh scripts/prepare-runtime-loadtest-seed.sh scripts/runtime-http-loadtest.sh scripts/runtime-http-loadtest-compose.sh scripts/api-contract-smoke-draft.sh scripts/bootstrap-real-release-env.sh scripts/prepare-real-e2e-auth-env.sh scripts/check-real-e2e-auth-accounts.sh scripts/prepare-real-protocol-matrix-assets.sh scripts/prepare-real-protocol-matrix-endpoints.sh scripts/prepare-real-protocol-matrix-control-plane.sh scripts/check-real-protocol-matrix-env.sh scripts/observe-real-client-env.sh scripts/check-deploy-artifact-endpoint.sh scripts/check-real-release.sh scripts/check-real-release-env.sh scripts/check-real-subscription-client-compat.sh scripts/prepare-real-release-env.sh scripts/verify-no-secrets.sh scripts/check-no-legacy.sh scripts/check-source-file-length.sh scripts/validate-real-release-env.sh scripts/check-real-release-assets-docs.sh scripts/check-agent-install-cleanup.sh scripts/check-v2-deploy-contract.sh scripts/deploy-access-agent.sh scripts/test-real-v2-inventory-fallback.sh scripts/test-real-test-server-assets-static.sh scripts/test-real-remote-install-smoke-env.sh scripts/test-real-v2-plan-binding.sh scripts/test-real-multi-user-traffic-uat-static.sh scripts/test-real-subscription-client-compat-static.sh scripts/test-real-access-inbound-matrix-static.sh scripts/test-real-third-party-redaction.sh; do bash -n "$$script"; done
	find scripts/lib -name '*.sh' -print0 | xargs -0 -r -n1 bash -n
	bash scripts/verify-no-secrets.sh
	bash scripts/check-real-release-assets-docs.sh
	bash scripts/check-agent-install-cleanup.sh
	bash scripts/check-v2-deploy-contract.sh
	bash scripts/check-agent-env-escape.sh
	bash scripts/test-v2-mainline-coverage.sh
	bash scripts/test-real-v2-inventory-fallback.sh
	bash scripts/test-real-remote-install-smoke-env.sh
	bash scripts/test-real-v2-plan-binding.sh
	bash scripts/test-real-multi-user-traffic-uat-static.sh
	bash scripts/test-real-subscription-client-compat-static.sh
	bash scripts/test-real-test-server-assets-static.sh
	bash scripts/test-real-access-inbound-matrix-static.sh
	bash scripts/test-real-third-party-redaction.sh

check-real-release-env:
	bash scripts/check-real-release-env.sh

check-real-subscription-client-compat:
	bash scripts/check-real-subscription-client-compat.sh

check-deploy-artifact-endpoint:
	bash scripts/check-deploy-artifact-endpoint.sh

check-real-release-gate-profiles:
	@UAT_VALIDATE_ONLY=1 UAT_PROFILE=24h UAT_REQUIRE_AGENT_API=1 BASE_URL=https://control.example.test DATABASE_URL=postgres://xrayc@db.example.test/xrayc SUB_TOKEN=private-sub-token CLIENT_PROXY_URL=socks5h://127.0.0.1:1080 EXPECTED_EXIT_IP=203.0.113.10 ACCESS_NODE_ID=00000000-0000-0000-0000-000000000101 ACCESS_LINE_ID=00000000-0000-0000-0000-000000000201 EXIT_ENDPOINT_ID=00000000-0000-0000-0000-000000000301 USER_ACCESS_TOKEN=private-user-token AGENT_TOKEN=private-agent-token UAT_ACCESS_NODE_IDS=00000000-0000-0000-0000-000000000101,00000000-0000-0000-0000-000000000102 UAT_ACCESS_LINE_IDS=00000000-0000-0000-0000-000000000201,00000000-0000-0000-0000-000000000202 UAT_EXIT_ENDPOINT_IDS=00000000-0000-0000-0000-000000000301,00000000-0000-0000-0000-000000000302 UAT_CLIENT_PROXY_URLS=socks5h://127.0.0.1:1080,socks5h://127.0.0.1:1081 UAT_EXPECTED_EXIT_IPS=203.0.113.10,203.0.113.11 UAT_USER_ACCESS_TOKENS=private-user-token-a,private-user-token-b UAT_AGENT_TOKENS=private-agent-token-a,private-agent-token-b bash scripts/real-runtime-stability-uat.sh
	@RUNTIME_LOADTEST_VALIDATE_ONLY=1 RUNTIME_LOADTEST_PROFILE=large DATABASE_URL=postgres://xrayc@loadtest.example.test/xrayc RUNTIME_LOADTEST_FORBID_DATABASE_URL=postgres://xrayc@real.example.test/xrayc bash scripts/runtime-loadtest.sh
	@RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY=1 RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=0 PROFILE=large BASE_URL=https://control.example.test USER_ACCESS_TOKEN=private-user-token ADMIN_ACCESS_TOKEN=private-admin-token AGENT_TOKEN=private-agent-token ACCESS_NODE_ID=00000000-0000-0000-0000-000000000101 ACCESS_LINE_ID=00000000-0000-0000-0000-000000000201 EXIT_ENDPOINT_ID=00000000-0000-0000-0000-000000000301 XRAY_USER_KEY=runtime-http-loadtest@example.test bash scripts/runtime-http-loadtest.sh
	@UAT_VALIDATE_ONLY=1 UAT_COMPOSE_START=1 UAT_MIN_API_REPLICAS=2 ADMIN_LOGIN_ACCOUNT=admin@example.test ADMIN_LOGIN_PASSWORD=admin123456 bash scripts/real-auth-ha-uat.sh
	@AUTH_HA_STABILITY_VALIDATE_ONLY=1 UAT_COMPOSE_START=1 UAT_MIN_API_REPLICAS=2 ADMIN_LOGIN_ACCOUNT=admin@example.test ADMIN_LOGIN_PASSWORD=admin123456 bash scripts/real-auth-ha-stability-uat.sh
	@OPS_MISTAKE_RECOVERY_UAT_VALIDATE_ONLY=1 BASE_URL=https://control.example.test DATABASE_URL=postgres://xrayc@db.example.test/xrayc ADMIN_LOGIN_ACCOUNT=admin@example.test ADMIN_LOGIN_PASSWORD=admin123456 bash scripts/ops-mistake-recovery-uat.sh
	@OPS_MISTAKE_RECOVERY_LARGE_UAT_VALIDATE_ONLY=1 OPS_MISTAKE_RECOVERY_LARGE_DATABASE_URL=postgres://xrayc@loadtest.example.test/xrayc RUNTIME_LOADTEST_FORBID_DATABASE_URL=postgres://xrayc@real.example.test/xrayc bash scripts/ops-mistake-recovery-large-uat.sh
	@bash scripts/test-real-multi-user-traffic-uat-static.sh

real-auth-ha-uat:
	UAT_COMPOSE_START=$${UAT_COMPOSE_START:-1} UAT_COMPOSE_NO_BUILD=$${UAT_COMPOSE_NO_BUILD:-1} ADMIN_LOGIN_ACCOUNT=$${ADMIN_LOGIN_ACCOUNT:-$${E2E_ADMIN_ACCOUNT:-admin@example.test}} ADMIN_LOGIN_PASSWORD=$${ADMIN_LOGIN_PASSWORD:-$${E2E_ADMIN_PASSWORD:-admin123456}} bash scripts/real-auth-ha-uat.sh

real-auth-ha-stability-uat:
	UAT_COMPOSE_START=$${UAT_COMPOSE_START:-1} UAT_COMPOSE_NO_BUILD=$${UAT_COMPOSE_NO_BUILD:-1} AUTH_HA_STABILITY_PROFILE=$${AUTH_HA_STABILITY_PROFILE:-6h} bash scripts/real-auth-ha-stability-uat.sh

runtime-loadtest-seed:
	test -n "$${RUNTIME_LOADTEST_DATABASE_URL:-}" || { echo "RUNTIME_LOADTEST_DATABASE_URL is required"; exit 2; }
	real_release_database_url="$${DATABASE_URL:-}"; DATABASE_URL="$${RUNTIME_LOADTEST_DATABASE_URL}" RUNTIME_LOADTEST_FORBID_DATABASE_URL="$$real_release_database_url" XRAYC_ENV=loadtest bash scripts/prepare-runtime-loadtest-seed.sh

runtime-loadtest-smoke:
	test -n "$${RUNTIME_LOADTEST_DATABASE_URL:-}" || { echo "RUNTIME_LOADTEST_DATABASE_URL is required"; exit 2; }
	real_release_database_url="$${DATABASE_URL:-}"; DATABASE_URL="$${RUNTIME_LOADTEST_DATABASE_URL}" RUNTIME_LOADTEST_FORBID_DATABASE_URL="$$real_release_database_url" RUNTIME_LOADTEST_PROFILE=smoke XRAYC_ENV=loadtest bash scripts/runtime-loadtest.sh

runtime-loadtest-large: runtime-loadtest-seed
	real_release_database_url="$${DATABASE_URL:-}"; DATABASE_URL="$${RUNTIME_LOADTEST_DATABASE_URL}" RUNTIME_LOADTEST_FORBID_DATABASE_URL="$$real_release_database_url" RUNTIME_LOADTEST_PROFILE=large RUNTIME_LOADTEST_REQUIRE_LARGE=1 XRAYC_ENV=loadtest bash scripts/runtime-loadtest.sh

runtime-http-loadtest-smoke:
	PROFILE=smoke BASE_URL=$${BASE_URL:-http://127.0.0.1:8080} bash scripts/runtime-http-loadtest.sh

runtime-http-loadtest-large:
	RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=$${RUNTIME_HTTP_LOADTEST_REQUIRE_FULL:-1} PROFILE=large BASE_URL=$${BASE_URL:?BASE_URL is required for runtime-http-loadtest-large} bash scripts/runtime-http-loadtest.sh

real-multi-user-traffic-uat:
	XRAYC_REAL_MULTI_USER_COUNT=$${XRAYC_REAL_MULTI_USER_COUNT:-10} XRAYC_REAL_MULTI_USER_DURATION_SECONDS=$${XRAYC_REAL_MULTI_USER_DURATION_SECONDS:-2400} XRAYC_REAL_MULTI_USER_ROUNDS=$${XRAYC_REAL_MULTI_USER_ROUNDS:-2} XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES=$${XRAYC_REAL_MULTI_USER_ALLOWED_FAILURES:-3} bash scripts/real-multi-user-traffic-uat.sh

runtime-http-loadtest-compose:
	RUNTIME_HTTP_LOADTEST_COMPOSE_NO_BUILD=1 bash scripts/runtime-http-loadtest-compose.sh

real-runtime-stability-uat:
	bash scripts/real-runtime-stability-uat.sh

ops-mistake-recovery-uat:
	RUN_OPS_MISTAKE_RECOVERY_UAT=1 bash scripts/ops-mistake-recovery-uat.sh

ops-mistake-recovery-large-uat:
	RUN_OPS_MISTAKE_RECOVERY_LARGE_UAT=1 bash scripts/ops-mistake-recovery-large-uat.sh

real-release-gap-report:
	bash scripts/real-release-gap-report.sh

report-real-release-env-gaps: real-release-gap-report

bootstrap-real-release-env:
	@printf '%s\n' "bootstrap-real-release-env: creates a private env draft; DATABASE_URL enables read-only endpoint discovery."
	bash scripts/bootstrap-real-release-env.sh

prepare-real-e2e-auth-env:
	bash scripts/prepare-real-e2e-auth-env.sh

prepare-real-protocol-matrix-endpoints:
	bash scripts/prepare-real-protocol-matrix-endpoints.sh

prepare-real-protocol-matrix-control-plane:
	bash scripts/prepare-real-protocol-matrix-control-plane.sh

real-access-third-party-matrix-relay-e2e:
	bash scripts/real-access-third-party-matrix-relay-e2e.sh

observe-real-client-env:
	bash scripts/observe-real-client-env.sh

check-no-legacy:
	bash scripts/check-no-legacy.sh

check-source-file-length:
	bash scripts/check-source-file-length.sh

check-real-release-assets-docs:
	bash scripts/check-real-release-assets-docs.sh

check-agent-install-cleanup:
	bash scripts/check-agent-install-cleanup.sh

check-v2-deploy-contract:
	bash scripts/check-v2-deploy-contract.sh

compose-check:
	$(COMPOSE) config --services

package-docker-artifacts:
	mkdir -p $(ARTIFACT_DIR)
	$(COMPOSE) build api caddy
	docker tag $(APP_IMAGE) $(ACCESS_AGENT_IMAGE)
	tmp_dir="$$(mktemp -d)"; trap 'rm -rf "$$tmp_dir"' EXIT; docker pull --platform $(XRAY_IMAGE_PLATFORM) $(XRAY_UPSTREAM_IMAGE) || true; cid="$$(docker create --platform $(XRAY_IMAGE_PLATFORM) $(XRAY_UPSTREAM_IMAGE))"; found=0; for path in /usr/local/bin/xray /usr/bin/xray /xray /app/xray; do if docker cp "$$cid:$$path" "$$tmp_dir/xray" >/dev/null 2>&1; then found=1; break; fi; done; docker rm "$$cid" >/dev/null; test "$$found" = "1"; cp /etc/ssl/certs/ca-certificates.crt "$$tmp_dir/ca-certificates.crt"; printf 'FROM scratch\nCOPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt\nCOPY xray /usr/local/bin/xray\nENTRYPOINT ["/usr/local/bin/xray"]\nCMD ["-confdir", "/usr/local/etc/xray/"]\n' > "$$tmp_dir/Dockerfile"; docker build -t $(XRAY_LOCAL_IMAGE) "$$tmp_dir"
	rm -f $(ARTIFACT_DIR)/access-agent-image.tar $(ARTIFACT_DIR)/xray-image.tar
	docker save --platform $(XRAY_IMAGE_PLATFORM) -o $(ARTIFACT_DIR)/access-agent-image.tar $(ACCESS_AGENT_IMAGE)
	gzip -f $(ARTIFACT_DIR)/access-agent-image.tar
	docker save -o $(ARTIFACT_DIR)/xray-image.tar $(XRAY_LOCAL_IMAGE)
	gzip -f $(ARTIFACT_DIR)/xray-image.tar
	printf '{\n  "contract_version": "v2-docker-artifacts",\n  "remote_deploy": "docker-compose",\n  "runtime_cores": ["xray"],\n  "access_agent_image": "%s",\n  "xray_image": "%s",\n  "xray_upstream_image": "%s",\n  "git_commit": "%s",\n  "built_at_unix": "%s",\n  "artifacts": ["access-agent-manifest.json", "access-agent-image.tar.gz", "xray-image.tar.gz", "access-agent.sha256"]\n}\n' "$(ACCESS_AGENT_IMAGE)" "$(XRAY_LOCAL_IMAGE)" "$(XRAY_UPSTREAM_IMAGE)" "$$(git rev-parse --short=12 HEAD 2>/dev/null || printf unknown)" "$$(date +%s)" > $(ARTIFACT_DIR)/access-agent-manifest.json
	( cd $(ARTIFACT_DIR) && sha256sum access-agent-manifest.json access-agent-image.tar.gz xray-image.tar.gz > access-agent.sha256 )

up:
	$(COMPOSE) up -d --remove-orphans
	bash scripts/check-running-images-current.sh

rebuild:
	$(COMPOSE) up -d --build --remove-orphans
	bash scripts/check-running-images-current.sh

rebuild-backend:
	$(COMPOSE) up -d --build api worker caddy --remove-orphans
	bash scripts/check-running-images-current.sh

rebuild-caddy:
	$(COMPOSE) up -d --build caddy --remove-orphans
	bash scripts/check-running-images-current.sh

rebuild-frontend: rebuild-caddy

restart:
	$(COMPOSE) up -d --no-build --remove-orphans
	bash scripts/check-running-images-current.sh

down:
	$(COMPOSE) down --remove-orphans
