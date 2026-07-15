/*
 * 用途：集中 Phase A 真实操作 E2E 的通用工具（工件落盘、el-select/表单定位、写入等待、幂等检查）。
 * 仅为控制单文件行数（≤550）从 real-op-phaseA.spec.ts 原样拆出，函数实现与断言逻辑不变。
 * 本文件不是 spec（不匹配 *.spec.ts），只被 real-op-phaseA.spec.ts 复用，不修改业务实现。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response } from '@playwright/test';

const ARTIFACTS = process.env.PHASEA_ARTIFACTS || '/tmp/phaseA-artifacts.env';

export function required(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) {
    throw new Error(`缺少必填环境变量 ${name}（请在运行 spec 前注入真实 inventory 值）`);
  }
  return v;
}

// ---- 工件落盘（脱敏交给报告层；这里写真实值供主 Agent 接力，文件 0600） -------
const artifacts: Record<string, string> = {};
export function recordArtifact(key: string, value: string) {
  // 单行化，避免多行值（如安装错误日志）破坏 .env 文件格式。
  artifacts[key] = String(value).replace(/[\r\n]+/g, ' ').slice(0, 600);
  const lines: string[] = [];
  const existing = existsSync(ARTIFACTS) ? readFileSync(ARTIFACTS, 'utf8') : '';
  const known = new Set(Object.keys(artifacts));
  for (const line of existing.split('\n')) {
    const m = line.match(/^([A-Z0-9_]+)=/);
    if (m && known.has(m[1])) {
      continue; // 用新值覆盖
    }
    if (line.trim()) {
      lines.push(line);
    }
  }
  for (const [k, v] of Object.entries(artifacts)) {
    lines.push(`${k}=${v}`);
  }
  writeFileSync(ARTIFACTS, `${lines.join('\n')}\n`, { mode: 0o600 });
}

/** 打开 el-select 并按可见文本选项点击。 */
export async function pickSelectOption(page: Page, selectRoot: ReturnType<Page['locator']>, optionText: string | RegExp) {
  await selectRoot.click();
  const option = page.locator('.el-select-dropdown__item:visible').filter({ hasText: optionText }).first();
  await option.click();
}

/** 通过 el-form-item label 文本定位其控件根。 */
export function formItemByLabel(page: Page, label: string) {
  return page.locator('.el-form-item').filter({ has: page.locator('.el-form-item__label', { hasText: label }) }).first();
}

/** 等待某个 admin 写入 API 的 2xx 响应，返回响应体（用于抓取 id）。 */
export async function waitWrite(page: Page, pathPattern: RegExp, method: 'POST' | 'PUT', action: () => Promise<void>) {
  const respPromise = page.waitForResponse(
    (r) => pathPattern.test(new URL(r.url()).pathname) && r.request().method() === method,
    { timeout: 30_000 },
  );
  await action();
  const resp = await respPromise;
  expect(resp.status(), `${method} ${pathPattern} status`).toBeGreaterThanOrEqual(200);
  expect(resp.status(), `${method} ${pathPattern} status`).toBeLessThan(300);
  return resp;
}

export async function dataId(resp: Response): Promise<string> {
  const raw = await resp.json().catch(() => ({}));
  const data = (raw && typeof raw === 'object' && 'data' in raw) ? (raw as any).data : raw;
  const id = data?.id ?? data?.accessNodeId ?? data?.access_node_id;
  return id ? String(id) : '';
}

export async function shot(page: Page, name: string) {
  await page.screenshot({ path: `test-results/phaseA-${name}.png`, fullPage: true }).catch(() => {});
}

// 用页面登录会话的 Bearer 在浏览器内 GET（只读）admin/user 接口，返回 data。
export async function pageGet(page: Page, apiPath: string): Promise<any> {
  return page.evaluate(async (p) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch(p, { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    return j?.data ?? j;
  }, apiPath).catch(() => null);
}

// 幂等检查：判断列表里是否已存在含 needle 的对象（重跑安全）。
export async function adminListHas(page: Page, apiPath: string, listKey: string, needle: string) {
  const data = await pageGet(page, apiPath);
  const arr = (data?.[listKey] ?? data?.items ?? data) as any[];
  // 大小写无关：后端会把邮箱归一为小写,而命名空间含大写(phaseA),
  // 用大小写敏感比较会漏判「已存在」→ 幂等重跑时重复 POST 触发唯一约束 4xx。
  const lowered = needle.toLowerCase();
  return Array.isArray(arr) && arr.some((x) => JSON.stringify(x).toLowerCase().includes(lowered));
}
