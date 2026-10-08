#!/usr/bin/env python3
"""只用 GET 核对现有部署的 API/前端/Worker 版本，不部署、不重启、不改业务数据。"""
import json
import os
from pathlib import Path
import sys
from urllib.request import Request, urlopen


def main():
    base = os.environ.get("BASE_URL", "").rstrip("/")
    expected = os.environ.get("EXPECTED_RELEASE_ID", "")
    token_file = os.environ.get("RELEASE_ADMIN_TOKEN_FILE", "")
    if not base.startswith(("http://", "https://")) or not expected:
        raise ValueError("BASE_URL and EXPECTED_RELEASE_ID are required")

    def read(path, token=None):
        headers = {"Accept": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        with urlopen(Request(base + path, headers=headers, method="GET"), timeout=20) as response:
            return json.load(response)

    health = read("/health")
    frontend = read("/release.json")
    if health.get("success") is not True or health.get("release_id") != expected:
        raise ValueError("API health/release mismatch")
    if frontend.get("release_id") != expected:
        raise ValueError("frontend release mismatch")
    print("release-status: API and frontend match expected release")
    if not token_file:
        print("release-status: Worker not checked (RELEASE_ADMIN_TOKEN_FILE missing)")
        return 2
    info = read("/api/admin/system-info", Path(token_file).read_text().strip()).get("data", {})
    workers = [row for row in info.get("workers", []) if row.get("fresh") is True]
    if not workers or any(row.get("release_id") != expected for row in workers):
        raise ValueError("Worker heartbeat/release mismatch")
    print(f"release-status: {len(workers)} active Worker instance(s) match expected release")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        # 不输出带 token/地址的请求对象、响应体或环境变量。
        print("release-status: failed: " + type(error).__name__, file=sys.stderr)
        sys.exit(1)
