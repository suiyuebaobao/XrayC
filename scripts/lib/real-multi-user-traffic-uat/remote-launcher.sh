#!/usr/bin/env bash
set -euo pipefail
set +x

remote_tmp="$1"
worker="${remote_tmp}/remote-worker.sh"
pid_file="${remote_tmp}/remote-runner.pid"
status_file="${remote_tmp}/remote-status"

cat >"$worker" <<'WORKER'
#!/usr/bin/env bash
set +e
remote_tmp="$1"
result_file="${remote_tmp}/remote-results.txt"
stderr_file="${remote_tmp}/remote-stderr.txt"
status_file="${remote_tmp}/remote-status"
runner="${remote_tmp}/runner.sh"
env_file="${remote_tmp}/remote.env"
ports_file="${remote_tmp}/ports.tsv"
tarball="${remote_tmp}/configs.tar.gz"

bash "$runner" "$env_file" "$ports_file" "$tarball" >"$result_file" 2>"$stderr_file"
rc="$?"
printf '%s\n' "$rc" >"${status_file}.tmp"
mv "${status_file}.tmp" "$status_file"
exit "$rc"
WORKER
chmod 700 "$worker"
rm -f "$pid_file" "$status_file" "${status_file}.tmp" \
  "${remote_tmp}/remote-results.txt" "${remote_tmp}/remote-stderr.txt"
nohup bash "$worker" "$remote_tmp" >/dev/null 2>&1 </dev/null &
printf '%s\n' "$!" >"$pid_file"
