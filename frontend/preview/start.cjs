// 启动独立示例 API 与前端；端口冲突时退出，不接管其他进程，也不接触正式配置。
const { spawn } = require('node:child_process');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
let closing = false;
let web;
const api = spawn(process.execPath, [path.join(__dirname, 'server.cjs')], { cwd: root, stdio: ['ignore', 'pipe', 'inherit'] });
function stop(code = 0) {
  if (closing) return;
  closing = true;
  if (web && web.exitCode === null) web.kill('SIGTERM');
  if (api.exitCode === null) api.kill('SIGTERM');
  process.exitCode = code;
}
api.stdout.on('data', (chunk) => {
  process.stdout.write(chunk);
  if (web || !chunk.toString().includes('listening')) return;
  web = spawn(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '5173', '--strictPort'], {
    cwd: root, stdio: 'inherit', env: { ...process.env, VITE_UI_PREVIEW: 'true' },
  });
  web.on('error', () => stop(1));
  web.on('exit', (code) => stop(code || 0));
});
api.on('error', () => stop(1));
api.on('exit', (code) => stop(code || 0));
process.on('SIGINT', () => stop());
process.on('SIGTERM', () => stop());
