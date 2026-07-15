#!/usr/bin/env bash
# 用途：提供 runtime-http-loadtest.sh 的帮助文本，避免主脚本超过行数限制。
# 本文件由主脚本 source 加载，只定义帮助输出函数。

runtime_http_loadtest_usage() {
  cat <<'USAGE'
usage: BASE_URL="https://control.example.test" bash scripts/runtime-http-loadtest.sh

Runs a lightweight concurrent HTTP/API runtime load test. Output is limited to
stage names, request counts, HTTP outcome counts, and latency summaries.

Required:
  BASE_URL

Optional:
  PROFILE                               smoke, standard, large, or stress. Default standard.
  RUNTIME_HTTP_LOADTEST_PROFILE         Alias for PROFILE.
  RUNTIME_HTTP_LOADTEST_CONCURRENCY     Override profile concurrency.
  RUNTIME_HTTP_LOADTEST_REQUESTS        Override requests per stage.
  RUNTIME_HTTP_LOADTEST_AGENT_CONCURRENCY Override agent write-stage concurrency.
  RUNTIME_HTTP_LOADTEST_AGENT_REQUESTS  Override agent write-stage requests.
  RUNTIME_HTTP_LOADTEST_AGENT_BATCH_SIZE Override agent payload item count per request.
  RUNTIME_HTTP_LOADTEST_REQUIRE_FULL=1  Fail when protected stages are skipped.
  RUNTIME_HTTP_LOADTEST_MAX_AVG_MS      Average latency budget. Default 2000.
  RUNTIME_HTTP_LOADTEST_MAX_MAX_MS      Max latency budget. Default 10000.
  RUNTIME_HTTP_LOADTEST_AGENT_TRAFFIC_MAX_AVG_MS
                                        Average latency budget for batched traffic writes. Default 3000.
  RUNTIME_HTTP_LOADTEST_VALIDATE_ONLY=1 Validate configuration only.
  USER_ACCESS_TOKEN                     Enables user subscription/usage stages.
  USER_ACCESS_TOKENS                    Comma-separated user tokens for multi-user stages.
  ADMIN_ACCESS_TOKEN                    Enables admin runtime read stages.
  AGENT_TOKEN                           Enables agent stages when IDs are present.
  ACCESS_NODE_ID                        Required for agent stages.
  ACCESS_NODE_IDS                       Comma-separated access node IDs for admin runtime reads.
  ACCESS_LINE_ID                        Required for agent metrics/sessions/probes.
  ACCESS_LINE_IDS                       Comma-separated access line IDs for admin runtime reads.
  EXIT_ENDPOINT_ID                      Enables exit probe payload in probes stage.
  EXIT_ENDPOINT_IDS                     Comma-separated exit endpoint IDs for admin runtime reads.
  XRAY_USER_KEY                         Enables agent traffic/Xray Stats replay stage.
USAGE
}
