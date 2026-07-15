#!/usr/bin/env bash
# 用途：提供 runtime HTTP 压测的敏感请求 helper。
# 这里把 Bearer token 写入临时 curl config，避免出现在进程参数中。
# 文件只定义函数，不主动发起 HTTP 请求。
# 调用方负责创建 tmp_dir 并在退出时删除。
# token 会被校验为原始值或 Bearer 值，不允许传入完整 header。
# 配置文件权限固定为 600，避免其他用户读取。
# 新增请求 helper 时继续保持不打印 token、URL 密钥或响应体。
# 本文件服务真实发布压测，不能依赖 mock 数据。
# 修改时同步 runtime-http-loadtest.sh 的 bash 语法检查。
# 本头部满足前十行中文注释约束。

write_curl_bearer_config() {
  local token="${1:-}"
  local config_file="$2"
  case "$token" in
    *$'\n'*|*$'\r'*)
      die "authorization token must not contain newlines"
      ;;
    [Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]:*)
      die "authorization token must be a raw token or Bearer token, not a full header"
      ;;
    [Bb][Ee][Aa][Rr][Ee][Rr]\ *)
      token="${token#* }"
      ;;
  esac
  [[ -n "$token" ]] || die "authorization token must not be empty"
  TOKEN_VALUE="$token" python3 - "$config_file" <<'PY'
import os
import sys

token = os.environ["TOKEN_VALUE"].replace("\\", "\\\\").replace('"', '\\"')
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write(f'header = "Authorization: Bearer {token}"\n')
PY
  chmod 600 "$config_file"
}
