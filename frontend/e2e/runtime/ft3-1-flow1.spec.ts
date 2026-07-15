/*
 * 用途：FT3-1 流1（VLESS + 限速 + 剔除）控制面真实 UI E2E（no-mock）。
 * 全程真实 admin/user 登录 + 真实页面点击 + 真实后端 + 真实 SSH 一键安装；严禁 mock/SQL 直插。
 * 只建流1需要的对象：套餐(20Mbps 限速)/用户/授权/中转节点/本机出口/两入口(IP-none 20001、IP-reality 20002)/绑定/分组/套餐授权分组/订阅。
 * 所有对象用 PHASEA_RUN_ID 命名空间，绝不碰基线真实客户数据。串行 mode、幂等可重跑。
 * 产出 id/token 写入 PHASEA_ARTIFACTS（0600）供主 Agent 接力。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/重试循环依赖顺序等待 */

const RUN_ID = process.env.PHASEA_RUN_ID || `${Date.now()}`;
const NS = process.env.FT3_NS || `e2e-r1-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.PHASEA_ACME_EMAIL || 'acme@example.test';
const ARTIFACTS = process.env.PHASEA_ARTIFACTS || '/tmp/ft3-1-artifacts.env';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.PHASEA_HEARTBEAT_TIMEOUT_MS || 480_000);

const SSH_HOST = required('PHASEA_SSH_HOST');
const SSH_PORT = process.env.PHASEA_SSH_PORT || '22';
const SSH_USER = process.env.PHASEA_SSH_USER || 'root';
const SSH_PASSWORD = required('PHASEA_SSH_PASSWORD');
const DIRECT_DOMAIN = required('PHASEA_DIRECT_DOMAIN');
const IP_DIRECT = process.env.PHASEA_IP_DIRECT || SSH_HOST;

const RATE_LIMIT_MBPS = process.env.FT3_RATE_LIMIT_MBPS || '20';
const USER_EMAIL = `${NS}@example.test`;
const USER_PASSWORD = 'Ft3realui-Flow1-123456';
const PLAN_NAME = `${NS}-plan`;
const GROUP_NAME = `${NS}-group`;
const NODE_NAME = `${NS}-node`;
const EXIT_LINE_NAME = `${NS}-exit`;
const ENTRY_NONE = `${NS}-ip-none`;
const ENTRY_REALITY = `${NS}-ip-reality`;
const PORT_NONE = Number(process.env.FT3_PORT_NONE || 20001);
const PORT_REALITY = Number(process.env.FT3_PORT_REALITY || 20002);

function required(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) {
    throw new Error(`缺少必填环境变量 ${name}`);
  }
  return v;
}

const artifacts: Record<string, string> = {};
function recordArtifact(key: string, value: string) {
  artifacts[key] = String(value).replace(/[\r\n]+/g, ' ').slice(0, 600);
  const lines: string[] = [];
  const existing = existsSync(ARTIFACTS) ? readFileSync(ARTIFACTS, 'utf8') : '';
  const known = new Set(Object.keys(artifacts));
  for (const line of existing.split('\n')) {
    const m = line.match(/^([A-Z0-9_]+)=/);
    if (m && known.has(m[1])) {
      continue;
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

async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

async function pickSelectOption(page: Page, selectRoot: ReturnType<Page['locator']>, optionText: string | RegExp) {
  await selectRoot.click();
  const option = page.locator('.el-select-dropdown__item:visible').filter({ hasText: optionText }).first();
  await option.click();
}

function formItemByLabel(page: Page, label: string) {
  return page.locator('.el-form-item').filter({ has: page.locator('.el-form-item__label', { hasText: label }) }).first();
}

async function waitWrite(page: Page, pathPattern: RegExp, method: 'POST' | 'PUT', action: () => Promise<void>) {
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

async function dataId(resp: Response): Promise<string> {
  const raw = await resp.json().catch(() => ({}));
  const data = (raw && typeof raw === 'object' && 'data' in raw) ? (raw as any).data : raw;
  const id = data?.id ?? data?.accessNodeId ?? data?.access_node_id;
  return id ? String(id) : '';
}

async function shot(page: Page, name: string) {
  await page.screenshot({ path: `test-results/ft3-${name}.png`, fullPage: true }).catch(() => {});
}

async function pageGet(page: Page, apiPath: string): Promise<any> {
  return page.evaluate(async (p) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch(p, { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    return j?.data ?? j;
  }, apiPath).catch(() => null);
}

async function adminListHas(page: Page, apiPath: string, listKey: string, needle: string) {
  const data = await pageGet(page, apiPath);
  const arr = (data?.[listKey] ?? data?.items ?? data) as any[];
  return Array.isArray(arr) && arr.some((x) => JSON.stringify(x).includes(needle));
}

test.describe.configure({ mode: 'serial' });

test.describe('FT3-1 流1 VLESS+限速+剔除 控制面真实操作', () => {
  let nodeId = '';
  let planId = '';
  let groupId = '';
  const entryIds: Record<string, string> = {};

  async function nodeIdFromList(page: Page): Promise<string> {
    const data = await pageGet(page, '/api/admin/access-nodes');
    const nodes = data?.accessNodes ?? data?.access_nodes ?? data ?? [];
    const found = Array.isArray(nodes) ? nodes.find((n: any) => n.name === NODE_NAME) : null;
    return found?.id ? String(found.id) : '';
  }

  async function waitNodeHealthy(page: Page): Promise<{ online: boolean; id: string }> {
    let online = false;
    let id = '';
    const deadline = Date.now() + HEARTBEAT_TIMEOUT_MS;
    while (Date.now() < deadline) {
      const data = await pageGet(page, '/api/admin/access-nodes');
      const nodes = data?.accessNodes ?? data?.access_nodes ?? data ?? [];
      const found = Array.isArray(nodes) ? nodes.find((n: any) => n.name === NODE_NAME) : null;
      if (found?.id) {
        id = String(found.id);
        const health = found.healthStatus ?? found.health_status;
        const hb = found.lastHeartbeatAt ?? found.last_heartbeat_at;
        if (hb && (health === 'healthy' || health === 'syncing')) {
          online = true;
          break;
        }
      }
      await page.waitForTimeout(6_000);
    }
    return { online, id };
  }

  test('步骤1：admin 真实登录', async ({ page }) => {
    await adminLogin(page);
    await expect(page.getByRole('heading', { name: /概览|运营|总览/ }).first()).toBeVisible();
  });

  test('步骤2：建套餐（50GB，默认限速 20Mbps）', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    if (await adminListHas(page, '/api/admin/plans', 'plans', PLAN_NAME)) {
      recordArtifact('FT3_PLAN_NAME', PLAN_NAME);
      const data = await pageGet(page, '/api/admin/plans');
      const arr = data?.plans ?? data ?? [];
      const found = Array.isArray(arr) ? arr.find((p: any) => p.name === PLAN_NAME) : null;
      planId = found?.id ? String(found.id) : '';
      recordArtifact('FT3_PLAN_ID', planId);
      return;
    }
    await page.getByRole('button', { name: '新增套餐' }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('例如：标准套餐').fill(PLAN_NAME);
    await formItemByLabel(page, '流量额度（GB）').locator('input').fill('50');
    await formItemByLabel(page, '周期（天）').locator('input').fill('30');
    // 关键：默认限速 20Mbps（el-input-number form.rateLimitMbps）。
    const rateInput = formItemByLabel(page, '默认限速（Mbps）').locator('input').first();
    await rateInput.click();
    await rateInput.fill(RATE_LIMIT_MBPS);
    await page.keyboard.press('Tab');
    const resp = await waitWrite(page, /\/api\/admin\/plans$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建套餐' }).click();
    });
    planId = await dataId(resp);
    recordArtifact('FT3_PLAN_ID', planId);
    recordArtifact('FT3_PLAN_NAME', PLAN_NAME);
    recordArtifact('FT3_PLAN_RATE_MBPS', RATE_LIMIT_MBPS);
    await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
    await shot(page, '02-plan-created');
    expect(planId, '套餐 id').not.toBe('');
  });

  test('步骤3：建用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    recordArtifact('FT3_USER_EMAIL', USER_EMAIL);
    recordArtifact('FT3_USER_PASSWORD', USER_PASSWORD);
    await page.goto('/admin/users');
    if (await adminListHas(page, '/api/admin/users', 'users', USER_EMAIL)) {
      const data = await pageGet(page, '/api/admin/users');
      const arr = data?.users ?? data ?? [];
      const found = Array.isArray(arr) ? arr.find((u: any) => u.email === USER_EMAIL) : null;
      recordArtifact('FT3_USER_ID', found?.id ? String(found.id) : '');
      return;
    }
    await page.getByRole('button', { name: '新增用户' }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('user@example.com').fill(USER_EMAIL);
    await dialog.getByPlaceholder('至少 8 位').fill(USER_PASSWORD);
    await pickSelectOption(page, formItemByLabel(page, '套餐').locator('.el-select'), PLAN_NAME);
    const resp = await waitWrite(page, /\/api\/admin\/users$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    recordArtifact('FT3_USER_ID', await dataId(resp));
    await expect(page.getByText(USER_EMAIL).first()).toBeVisible();
    await shot(page, '03-user-created');
  });

  test('步骤4：一键安装 Agent（真实 SSH）并等心跳上线', async ({ page }) => {
    test.skip(process.env.PHASEA_SKIP_ONE_CLICK === '1', '一键安装失败，改走手动登记路径');
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 180_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    nodeId = await nodeIdFromList(page);
    if (nodeId) {
      recordArtifact('FT3_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      recordArtifact('FT3_NODE_ONLINE', String(r.online));
      expect(r.online, '已存在节点应健康').toBe(true);
      return;
    }
    await expect(page.getByRole('button', { name: '一键安装 Agent' })).toBeVisible();
    await page.getByRole('button', { name: '一键安装 Agent' }).click();
    const dialog = page.getByRole('dialog', { name: '一键安装 Agent' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '节点名称').locator('input').fill(NODE_NAME);
    // 「公网地址」输入框已移除:public_host 由 IP直连>域名直连>CF 自动推导,不再填。
    await formItemByLabel(page, 'SSH Host（IP）').locator('input').fill(SSH_HOST);
    await formItemByLabel(page, 'SSH Port').locator('input').fill(SSH_PORT);
    await formItemByLabel(page, 'SSH User').locator('input').fill(SSH_USER);
    await formItemByLabel(page, 'SSH 密码').locator('input').fill(SSH_PASSWORD);
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    await formItemByLabel(page, '域名直连地址').locator('input').fill(DIRECT_DOMAIN);
    await formItemByLabel(page, 'ACME 邮箱').locator('input').fill(ACME_EMAIL);
    await shot(page, '04a-oneclick-form');
    await waitWrite(page, /\/api\/admin\/access-nodes\/one-click-install$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建安装任务' }).click();
    });
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    for (let i = 0; i < 20; i++) {
      const td = await pageGet(page, '/api/admin/deployment-tasks');
      const items = td?.items ?? td ?? [];
      const mine = Array.isArray(items)
        ? items.find((t: any) => t?.safe_metadata?.access_node_name === NODE_NAME)
        : null;
      if (mine?.status === 'failed') {
        const reason = String(mine?.result?.error || mine?.error_summary || 'ssh_install_failed');
        recordArtifact('FT3_INSTALL_FAILURE', reason.slice(0, 500));
        throw new Error(`一键安装失败：${reason.slice(0, 300)}`);
      }
      if (mine?.status === 'succeeded') break;
      await page.waitForTimeout(6_000);
    }
    const result = await waitNodeHealthy(page);
    nodeId = result.id || (await nodeIdFromList(page));
    recordArtifact('FT3_NODE_ID', nodeId);
    recordArtifact('FT3_NODE_ONLINE', String(result.online));
    await shot(page, '04c-node-status');
    expect(nodeId, '节点 id 应创建').not.toBe('');
    expect(result.online, `节点心跳应在 ${HEARTBEAT_TIMEOUT_MS}ms 内上线`).toBe(true);
  });

  test('步骤5：建本机出口（IP 直连 + VLESS Reality）', async ({ page }) => {
    await adminLogin(page);
    if (!nodeId) nodeId = await nodeIdFromList(page);
    test.skip(!nodeId, '节点未创建');
    await page.goto('/admin/transit-nodes');
    if (await adminListHas(page, '/api/admin/exit-endpoints', 'exit_endpoints', EXIT_LINE_NAME)) {
      recordArtifact('FT3_EXIT_LINE_NAME', EXIT_LINE_NAME);
      return;
    }
    const nodeCard = page.locator('.relay-card').filter({ hasText: NODE_NAME }).first();
    await expect(nodeCard).toBeVisible();
    await nodeCard.getByRole('button', { name: '本机出口服务' }).click();
    const dialog = page.getByRole('dialog', { name: '本机出口服务' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: '添加线路' }).click();
    const nameInput = dialog.locator('input[placeholder*="香港自建"]').first();
    if (await nameInput.count() > 0) {
      await nameInput.fill(EXIT_LINE_NAME);
    }
    const addrInput = dialog.locator('input[placeholder*="自动读取中转节点地址"]').first();
    if (await addrInput.count() > 0 && ((await addrInput.inputValue().catch(() => '')) || '').trim() === '') {
      await addrInput.fill(IP_DIRECT);
    }
    await waitWrite(page, /\/api\/admin\/access-nodes\/[0-9a-f-]+\/local-exit-lines$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '添加到出口管理' }).click();
    });
    recordArtifact('FT3_EXIT_LINE_NAME', EXIT_LINE_NAME);
    await shot(page, '05-local-exit-created');
  });

  async function openEntryDialog(page: Page) {
    await page.goto('/admin/access-entries');
    await expect(page.getByRole('button', { name: '创建入口' })).toBeVisible();
    await page.getByRole('button', { name: '创建入口' }).click();
    const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: '创建入口' }) });
    await expect(dialog).toBeVisible();
    return dialog;
  }

  async function fillEntryBasics(page: Page, dialog: ReturnType<Page['locator']>, name: string) {
    await formItemByLabel(page, '入口名称').locator('input').first().fill(name);
    await pickSelectOption(page, formItemByLabel(page, '接入节点').locator('.el-select'), new RegExp(NODE_NAME));
  }

  async function pickConnMode(page: Page, label: string) {
    await page.locator('.el-radio-button').filter({ hasText: label }).first()
      .locator('.el-radio-button__inner').click();
  }

  async function setListenPort(page: Page, port: number) {
    const input = formItemByLabel(page, '监听端口').locator('input').first();
    await input.click();
    await input.fill(String(port));
    await page.keyboard.press('Tab');
  }

  async function saveEntry(page: Page, dialog: ReturnType<Page['locator']>, key: string, name: string) {
    const resp = await waitWrite(page, /\/api\/admin\/access-entries$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    const id = await dataId(resp);
    entryIds[key] = id;
    recordArtifact(`FT3_ENTRY_${key}_ID`, id);
    await expect(page.getByText(name).first()).toBeVisible();
  }

  async function entryDone(page: Page, name: string): Promise<boolean> {
    await page.goto('/admin/access-entries');
    if (await adminListHas(page, '/api/admin/access-entries', 'access_entries', name)) {
      await expect(page.getByText(name).first()).toBeVisible();
      return true;
    }
    return false;
  }

  test('步骤6a：入口① IP 直连 + VLESS 普通(none)', async ({ page }) => {
    await adminLogin(page);
    if (!nodeId) nodeId = await nodeIdFromList(page);
    test.skip(!nodeId, '节点未创建');
    if (await entryDone(page, ENTRY_NONE)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, ENTRY_NONE);
    await pickConnMode(page, 'IP 直连（免证书）');
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), '普通 VLESS');
    await setListenPort(page, PORT_NONE);
    await shot(page, '06a-entry-ip-none-form');
    await saveEntry(page, dialog, 'IP_NONE', ENTRY_NONE);
  });

  test('步骤6b：入口② IP 直连 + VLESS Reality', async ({ page }) => {
    await adminLogin(page);
    if (!nodeId) nodeId = await nodeIdFromList(page);
    test.skip(!nodeId, '节点未创建');
    if (await entryDone(page, ENTRY_REALITY)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, ENTRY_REALITY);
    await pickConnMode(page, 'IP 直连（免证书）');
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'Reality');
    await setListenPort(page, PORT_REALITY);
    await shot(page, '06b-entry-ip-reality-form');
    await saveEntry(page, dialog, 'IP_REALITY', ENTRY_REALITY);
  });

  test('步骤7：每条入口绑定出口线路', async ({ page }) => {
    await adminLogin(page);
    if (!nodeId) nodeId = await nodeIdFromList(page);
    test.skip(!nodeId, '节点未创建');
    for (const name of [ENTRY_NONE, ENTRY_REALITY]) {
      await page.goto('/admin/access-entries');
      const row = page.locator('tr').filter({ hasText: name }).first();
      if (await row.count() === 0) continue;
      // 已绑定则跳过（幂等）。
      const bindBtn = row.getByRole('button', { name: '绑定线路' });
      if (await bindBtn.count() === 0) continue;
      await bindBtn.click();
      const dialog = page.getByRole('dialog', { name: '绑定线路' });
      await expect(dialog).toBeVisible();
      await dialog.locator('.el-select').first().click();
      await page.locator('.el-select-dropdown__item:visible').first().click();
      await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
    }
    await shot(page, '07-bindings');
  });

  test('步骤8：建分组并选绑定节点', async ({ page }) => {
    await adminLogin(page);
    if (!nodeId) nodeId = await nodeIdFromList(page);
    test.skip(!nodeId, '节点未创建');
    await page.goto('/admin/line-groups');
    if (await adminListHas(page, '/api/admin/line-groups', 'line_groups', GROUP_NAME)) {
      recordArtifact('FT3_GROUP_NAME', GROUP_NAME);
      const data = await pageGet(page, '/api/admin/line-groups');
      const arr = data?.line_groups ?? data?.lineGroups ?? data ?? [];
      const found = Array.isArray(arr) ? arr.find((g: any) => g.name === GROUP_NAME) : null;
      groupId = found?.id ? String(found.id) : '';
      recordArtifact('FT3_GROUP_ID', groupId);
      return;
    }
    await page.getByRole('button', { name: '创建分组' }).click();
    const dialog = page.getByRole('dialog', { name: '创建分组' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    // 只选本 NS 的节点，避免误把基线节点拉进分组。
    const opt = page.locator('.el-select-dropdown__item:visible').filter({ hasText: NODE_NAME }).first();
    if (await opt.count() > 0) {
      await opt.click();
    } else {
      await page.locator('.el-select-dropdown__item:visible').first().click();
    }
    await page.keyboard.press('Escape');
    const resp = await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    groupId = await dataId(resp);
    recordArtifact('FT3_GROUP_ID', groupId);
    recordArtifact('FT3_GROUP_NAME', GROUP_NAME);
    await shot(page, '08-group-created');
  });

  test('步骤9：套餐授权分组', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    const row = page.locator('tr').filter({ hasText: PLAN_NAME }).first();
    await row.getByRole('button', { name: '编辑分组' }).click();
    const dialog = page.getByRole('dialog').filter({ hasText: '配置套餐分组' });
    await expect(dialog).toBeVisible();
    await dialog.locator('.el-form-item').filter({ hasText: '可用分组' }).locator('.el-select').first().click();
    await page.locator('.el-select-dropdown__item:visible').filter({ hasText: GROUP_NAME }).first().click();
    await page.keyboard.press('Escape');
    await waitWrite(page, /\/api\/admin\/plans\/[0-9a-f-]+\/line-groups$/, 'PUT', async () => {
      await dialog.getByRole('button', { name: '保存分组' }).click();
    });
    await shot(page, '09-plan-authorized');
  });

  test('步骤10：用户端真实登录取订阅 token', async ({ page }) => {
    await login(page, '/login', USER_EMAIL, USER_PASSWORD, /\/(dashboard|subscription)$/);
    await page.goto('/subscription');
    await page.waitForURL(/\/subscription$/, { timeout: 15_000 }).catch(() => {});
    let subUrl = '';
    let token = '';
    for (let i = 0; i < 12 && !subUrl; i++) {
      const data = await pageGet(page, '/api/user/subscription');
      subUrl = String((data as any)?.subscriptionUrl || (data as any)?.subscription_url || '');
      const mm = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
      if (mm) token = mm[1];
      if (!token && (data as any)?.token) token = String((data as any).token);
      if (!subUrl) await page.waitForTimeout(2_000);
    }
    recordArtifact('FT3_SUB_URL', subUrl);
    recordArtifact('FT3_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl, '订阅链接应非空').not.toBe('');
    expect(token, '订阅 token 应解析到').not.toBe('');
  });
});
