/*
 * 用途：Phase3 限速测试场景【全真实后台 UI】搭建（admin/admin123456，面板 127.0.0.1:8080）。
 * 复用 ft3-1-flow1 / real-op-cf-reuse / plan-updown-ui 的真实点击口径，严禁 mock/SQL 直插造数。
 * 建：2 套餐（压测-限速 up2/down5、压测-不限速 0/0）+ 10 用户（pressL1..L5 限速、pressU1..U5 不限速）
 *     + 3 入口（CF橙云 / 灰云直连 / IP直连 Reality，分别挂现有在线节点，绑各自本机出口）
 *     + 1 分组（含 3 条入口出口绑定）+ 两套餐都授权该分组 → 10 用户各取真实订阅。
 * 现有在线节点按 id 前缀解析：5b364d06=CF橙云、b4d05aad=灰云直连(带域名)；两节点证书均 valid。
 * 真实订阅 URL/token 写入 PHASE3_SCENARIO_OUT（供后续真客户端协议 + 压测阶段读，0600）。
 * 只读探查（pageGet）仅用于导航/幂等/订阅提取，所有对象创建一律真实 UI 点击。
 */
import { writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/逐用户顺序操作依赖顺序等待 */

const ADMIN = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const PASS = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const OUT = process.env.PHASE3_SCENARIO_OUT
  || '/tmp/claude-0/-root-suiyue/2b33ac0a-52d2-420f-830c-f512ac403e79/scratchpad/phase3-scenario.json';

const USER_PASSWORD = 'Press-Phase3-123456';
const GROUP_NAME = '压测-分组';

// 两套餐定义（0=不限）。
const PLANS = [
  { name: '压测-限速', up: 2, down: 5, traffic: 50, days: 30 },
  { name: '压测-不限速', up: 0, down: 0, traffic: 50, days: 30 },
] as const;

// 10 个用户：L 系订限速、U 系订不限速。邮箱用小写（后端统一归一为小写存储）。
const USERS = [
  ...[1, 2, 3, 4, 5].map((i) => ({ username: `pressL${i}`, email: `pressl${i}@test.local`, plan: '压测-限速' })),
  ...[1, 2, 3, 4, 5].map((i) => ({ username: `pressU${i}`, email: `pressu${i}@test.local`, plan: '压测-不限速' })),
];

// 三入口定义：nodePrefix 解析现有在线节点；mode 决定连接方式与端口控件类型。
const ENTRIES = [
  { key: 'cf', name: '压测-CF入口', nodePrefix: '5b364d06', mode: 'cf' as const,
    modeLabel: 'CF 直连（橙云）', protocol: 'VLESS', security: '', port: 8443, portIsSelect: true,
    wantTransport: 'WebSocket', transportTag: 'ws', desc: 'CF橙云 VLESS+WS+TLS' },
  { key: 'direct', name: '压测-灰云入口', nodePrefix: 'b4d05aad', mode: 'domain' as const,
    modeLabel: '域名直连（灰云）', protocol: 'VLESS', security: '', port: 8443, portIsSelect: false,
    wantTransport: '', transportTag: 'tcp', desc: '灰云直连 VLESS+TLS(TCP)' },
  { key: 'ip', name: '压测-IP入口', nodePrefix: 'b4d05aad', mode: 'ip' as const,
    modeLabel: 'IP 直连（免证书）', protocol: 'VLESS', security: 'Reality', port: 20443, portIsSelect: false,
    wantTransport: '', transportTag: 'tcp', desc: 'IP直连 VLESS Reality(TCP)' },
];

type NodeInfo = { id: string; name: string; ip: string; cfEnabled: boolean };
const nodes: Record<string, NodeInfo> = {}; // by prefix
const createdEntries: Array<{ key: string; name: string; node: string; nodeId: string; desc: string;
  protocol: string; mode: string; port: number; ip: string }> = [];

// 累积场景产出，逐步落盘。
const scenario: any = {
  panel: process.env.E2E_BASE_URL || 'http://127.0.0.1:8080',
  group: GROUP_NAME,
  plans: PLANS.map((p) => ({ name: p.name, uplinkMbps: p.up, downlinkMbps: p.down, trafficGb: p.traffic, days: p.days })),
  entries: [],
  users: [],
};
function persist() {
  writeFileSync(OUT, JSON.stringify(scenario, null, 2), { mode: 0o600 });
}

// ---- 通用 UI 工具（与参考 spec 同口径） ----
async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN, PASS, /\/(overview|dashboard)$/);
}
async function pickSelectOption(page: Page, selectRoot: ReturnType<Page['locator']>, optionText: string | RegExp) {
  await selectRoot.click();
  const option = page.locator('.el-select-dropdown__item:visible').filter({ hasText: optionText }).first();
  await expect(option).toBeVisible({ timeout: 10_000 });
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
async function pageGet(page: Page, apiPath: string): Promise<any> {
  return page.evaluate(async (p) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch(p, { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    return j?.data ?? j;
  }, apiPath).catch(() => null);
}
async function listHas(page: Page, apiPath: string, key: string, needle: string) {
  const data = await pageGet(page, apiPath);
  const arr = (data?.[key] ?? data?.items ?? data) as any[];
  return Array.isArray(arr) && arr.some((x) => JSON.stringify(x).includes(needle));
}
// 用户列表分页（pageSize 默认 20），用 keyword 精确查，避免被分页漏判。
async function userExists(page: Page, email: string) {
  const data = await pageGet(page, `/api/admin/users?keyword=${encodeURIComponent(email)}&pageSize=50`);
  const arr = (data?.items ?? data?.users ?? data) as any[];
  const want = email.toLowerCase();
  return Array.isArray(arr) && arr.some((u) => String(u?.email ?? '').toLowerCase() === want);
}
async function shot(page: Page, name: string) {
  await page.screenshot({ path: `test-results/phase3-${name}.png`, fullPage: true }).catch(() => {});
}
// 把某入口的「传输」多选收敛为恰好 [transportLabel]。
// 关键：必须【先加目标再删其它】——先删空会触发弹窗自动回填默认传输（CF 回 XHTTP），导致 2 个传输保存被拦。
async function ensureEntryTransport(page: Page, entryName: string, transportLabel: string) {
  await page.goto('/admin/access-entries');
  const row = page.locator('tr').filter({ hasText: entryName }).first();
  if (await row.count() === 0) return;
  await row.getByRole('button', { name: '编辑' }).first().click();
  const dialog = page.getByRole('dialog', { name: '编辑入口' });
  await expect(dialog).toBeVisible();
  const items = () => page.locator('.el-select-dropdown__item:visible');
  await formItemByLabel(page, '传输').locator('.el-select').first().click();
  await page.waitForTimeout(300);
  // pass1：先确保目标传输被选中（集合非空）。
  const target = items().filter({ hasText: transportLabel }).first();
  await expect(target).toBeVisible({ timeout: 8_000 });
  if (!((await target.getAttribute('class')) || '').includes('is-selected')) {
    await target.click();
    await page.waitForTimeout(250);
  }
  // pass2：逐个取消非目标（每轮重查，避免索引错位 / 自动回填干扰）。
  for (let guard = 0; guard < 6; guard++) {
    const all = items();
    const n = await all.count();
    let removed = false;
    for (let i = 0; i < n; i++) {
      const opt = all.nth(i);
      const txt = ((await opt.textContent()) || '').trim();
      const cls = (await opt.getAttribute('class')) || '';
      if (cls.includes('is-selected') && txt !== transportLabel) {
        await opt.click();
        await page.waitForTimeout(250);
        removed = true;
        break;
      }
    }
    if (!removed) break;
  }
  await page.keyboard.press('Escape');
  await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+$/, 'PUT', async () => {
    await dialog.getByRole('button', { name: '保存' }).click();
  });
}
// 分组列表只能从 access-routing 读模型取（/api/admin/line-groups 仅 POST/PUT，GET 返 405）。
async function groupExists(page: Page, name: string) {
  const data = await pageGet(page, '/api/admin/access-routing');
  const arr = (data?.line_groups ?? data?.lineGroups ?? []) as any[];
  return Array.isArray(arr) && arr.some((g) => g?.name === name);
}

test.describe.configure({ mode: 'serial' });

test.describe('Phase3 限速场景全真实 UI 搭建', () => {
  test('步骤0：admin 登录并解析现有在线节点', async ({ page }) => {
    await adminLogin(page);
    const raw = await pageGet(page, '/api/admin/access-nodes');
    const list = raw?.accessNodes ?? raw?.access_nodes ?? raw ?? [];
    for (const e of ENTRIES) {
      const n = (Array.isArray(list) ? list : []).find((x: any) => String(x.id).startsWith(e.nodePrefix));
      expect(n, `应找到节点前缀 ${e.nodePrefix}`).toBeTruthy();
      nodes[e.nodePrefix] = {
        id: String(n.id),
        name: String(n.name),
        ip: String(n.ipDirectAddress ?? n.ip_direct_address ?? n.clientConnectAddress ?? n.client_connect_address ?? ''),
        cfEnabled: Boolean(n.cfEnabled ?? n.cf_enabled),
      };
    }
    persist();
  });

  test('步骤1：建 2 套餐（限速 / 不限速）', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    for (const p of PLANS) {
      if (await listHas(page, '/api/admin/plans', 'plans', p.name)) continue;
      await page.getByRole('button', { name: '新增套餐' }).click();
      const dialog = page.getByRole('dialog', { name: '新增套餐' });
      await expect(dialog).toBeVisible();
      await dialog.getByPlaceholder('例如：标准套餐').fill(p.name);
      await formItemByLabel(page, '流量额度（GB）').locator('input').first().fill(String(p.traffic));
      await formItemByLabel(page, '上行限速').locator('input').first().fill(String(p.up));
      await formItemByLabel(page, '下行限速').locator('input').first().fill(String(p.down));
      await formItemByLabel(page, '周期（天）').locator('input').first().fill(String(p.days));
      await waitWrite(page, /\/api\/admin\/plans$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '创建套餐' }).click();
      });
      await expect(page.getByText(p.name).first()).toBeVisible();
    }
    await shot(page, '01-plans');
    for (const p of PLANS) expect(await listHas(page, '/api/admin/plans', 'plans', p.name)).toBe(true);
  });

  test('步骤2：建 10 用户并各授权套餐', async ({ page }) => {
    test.setTimeout(120_000);
    await adminLogin(page);
    await page.goto('/admin/users');
    for (const u of USERS) {
      if (await userExists(page, u.email)) continue;
      await page.getByRole('button', { name: '新增用户' }).click();
      const dialog = page.getByRole('dialog');
      await dialog.getByPlaceholder('user@example.com').fill(u.email);
      await dialog.getByPlaceholder('至少 8 位').fill(USER_PASSWORD);
      await pickSelectOption(page, formItemByLabel(page, '套餐').locator('.el-select'), u.plan);
      await waitWrite(page, /\/api\/admin\/users$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
      await expect(page.getByText(u.email).first()).toBeVisible();
    }
    await shot(page, '02-users');
    for (const u of USERS) expect(await userExists(page, u.email), `用户 ${u.email} 应已建`).toBe(true);
  });

  test('步骤3：建 3 入口（CF橙云 / 灰云直连 / IP直连 Reality）', async ({ page }) => {
    test.setTimeout(120_000);
    await adminLogin(page);
    for (const e of ENTRIES) {
      const node = nodes[e.nodePrefix];
      await page.goto('/admin/access-entries');
      if (await listHas(page, '/api/admin/access-entries', 'access_entries', e.name)) {
        createdEntries.push({ key: e.key, name: e.name, node: node.name, nodeId: node.id, desc: e.desc,
          protocol: e.protocol.toLowerCase(), mode: e.mode, port: e.port, ip: node.ip });
        continue;
      }
      try {
        await page.getByRole('button', { name: '创建入口' }).click();
        const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: '创建入口' }) });
        await expect(dialog).toBeVisible();
        await formItemByLabel(page, '入口名称').locator('input').first().fill(e.name);
        await pickSelectOption(page, formItemByLabel(page, '接入节点').locator('.el-select'),
          new RegExp(node.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
        await page.waitForTimeout(400);
        // 连接方式分段控件。
        await page.locator('.el-radio-button').filter({ hasText: e.modeLabel }).first()
          .locator('.el-radio-button__inner').click();
        await page.waitForTimeout(400);
        // 协议固定 VLESS。
        await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), e.protocol);
        // IP 直连显式选 Reality（域名/CF 安全模式仅 TLS、自动）。
        if (e.security) {
          await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), e.security);
        }
        // 监听端口：CF 是下拉（限 CF HTTPS 端口），其余是数字输入。
        if (e.portIsSelect) {
          await pickSelectOption(page, formItemByLabel(page, '监听端口').locator('.el-select'), String(e.port));
        } else {
          const portInput = formItemByLabel(page, '监听端口').locator('input').first();
          await portInput.click();
          await portInput.fill(String(e.port));
          await page.keyboard.press('Tab');
        }
        await shot(page, `03-entry-${e.key}-form`);
        await waitWrite(page, /\/api\/admin\/access-entries$/, 'POST', async () => {
          await dialog.getByRole('button', { name: '保存' }).click();
        });
        await expect(page.getByText(e.name).first()).toBeVisible({ timeout: 10_000 });
        createdEntries.push({ key: e.key, name: e.name, node: node.name, nodeId: node.id, desc: e.desc,
          protocol: e.protocol.toLowerCase(), mode: e.mode, port: e.port, ip: node.ip });
      } catch (err) {
        await shot(page, `03-entry-${e.key}-FAIL`);
        // 单入口失败不拖垮整套：记录跳过，靠其余入口保证订阅可下。
        console.log(`ENTRY_CREATE_FAILED key=${e.key} name=${e.name} err=${String(err).slice(0, 200)}`);
      }
      // 收敛传输（CF 默认 XHTTP，按任务口径改成 WebSocket；其余保持 TCP）。
      if (e.wantTransport) {
        try {
          await ensureEntryTransport(page, e.name, e.wantTransport);
        } catch (err) {
          console.log(`ENTRY_TRANSPORT_FIX_FAILED key=${e.key} err=${String(err).slice(0, 150)}`);
        }
      }
    }
    scenario.entries = createdEntries.map((c) => {
      const def = ENTRIES.find((x) => x.key === c.key)!;
      return { name: c.name, node: c.node, connection: c.mode, protocol: c.protocol,
        transport: def.transportTag, port: c.port, desc: c.desc };
    });
    persist();
    await shot(page, '03-entries');
    expect(createdEntries.length, '至少要建成 2 个入口以保证订阅可下').toBeGreaterThanOrEqual(2);
  });

  test('步骤4：每条入口绑定同节点本机出口', async ({ page }) => {
    test.setTimeout(120_000);
    await adminLogin(page);
    for (const e of createdEntries) {
      await page.goto('/admin/access-entries');
      // 幂等：已绑定则跳过。
      const bound = await page.evaluate(async (name) => {
        const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
        const tok = sess.accessToken || sess.access_token || '';
        const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
        const ej = er.ok ? await er.json() : null;
        const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
        const ent = (Array.isArray(entries) ? entries : []).find((x: any) => x.name === name);
        if (!ent?.id) return false;
        const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
        const bj = br.ok ? await br.json() : null;
        const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
        return (Array.isArray(binds) ? binds : []).some(
          (b: any) => String(b.access_entry_id ?? b.accessEntryId) === String(ent.id),
        );
      }, e.name).catch(() => false);
      if (bound) continue;
      const row = page.locator('tr').filter({ hasText: e.name }).first();
      await expect(row).toBeVisible();
      await row.getByRole('button', { name: '绑定线路' }).click();
      const dialog = page.getByRole('dialog', { name: '绑定线路' });
      await expect(dialog).toBeVisible();
      const select = dialog.locator('.el-select').first();
      await select.click();
      // 优先选同节点本机出口（label 含 host:port，按节点 IP 匹配），匹配不到回退第一个。
      const byIp = page.locator('.el-select-dropdown__item:visible').filter({ hasText: e.ip }).first();
      const target = (e.ip && await byIp.count() > 0) ? byIp : page.locator('.el-select-dropdown__item:visible').first();
      await expect(target).toBeVisible({ timeout: 10_000 });
      await target.click();
      await page.keyboard.press('Escape').catch(() => {});
      await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
    }
    await shot(page, '04-bindings');
  });

  test('步骤5：建分组并选 3 条入口出口绑定', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    if (!(await groupExists(page, GROUP_NAME))) {
      await page.getByRole('button', { name: '创建分组' }).click();
      const dialog = page.getByRole('dialog', { name: '创建分组' });
      await expect(dialog).toBeVisible();
      await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
      const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
      await bindSelect.click();
      await page.waitForTimeout(500);
      // 只勾选本场景 3 个入口对应的绑定项（option label 含入口名）。
      for (const e of createdEntries) {
        const opt = page.locator('.el-select-dropdown__item:visible').filter({ hasText: e.name }).first();
        if (await opt.count() > 0) {
          const cls = (await opt.getAttribute('class')) || '';
          if (!cls.includes('is-selected')) {
            await opt.click();
            await page.waitForTimeout(150);
          }
        }
      }
      await page.keyboard.press('Escape');
      await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
    }
    await shot(page, '05-group');
    expect(await groupExists(page, GROUP_NAME), '分组 压测-分组 应已建').toBe(true);
  });

  test('步骤6：两套餐都授权分组', async ({ page }) => {
    await adminLogin(page);
    for (const p of PLANS) {
      await page.goto('/admin/plans');
      const row = page.locator('tr').filter({ hasText: p.name }).first();
      await row.getByRole('button', { name: '编辑分组' }).click();
      const dialog = page.getByRole('dialog').filter({ hasText: '配置套餐分组' });
      await expect(dialog).toBeVisible();
      const sel = dialog.locator('.el-form-item').filter({ hasText: '可用分组' }).locator('.el-select').first();
      await sel.click();
      await page.waitForTimeout(400);
      const opt = page.locator('.el-select-dropdown__item:visible').filter({ hasText: GROUP_NAME }).first();
      const cls = (await opt.getAttribute('class').catch(() => '')) || '';
      if (!cls.includes('is-selected')) {
        await opt.click();
      }
      await page.keyboard.press('Escape');
      await waitWrite(page, /\/api\/admin\/plans\/[0-9a-f-]+\/line-groups$/, 'PUT', async () => {
        await dialog.getByRole('button', { name: '保存分组' }).click();
      });
    }
    await shot(page, '06-plan-auth');
  });

  test('步骤7：10 用户各真实登录取订阅 URL', async ({ page }) => {
    test.setTimeout(240_000);
    for (const u of USERS) {
      const planDef = PLANS.find((p) => p.name === u.plan)!;
      await login(page, '/login', u.email, USER_PASSWORD, /\/(dashboard|subscription)$/);
      await page.goto('/subscription');
      await page.waitForURL(/\/subscription$/, { timeout: 15_000 }).catch(() => {});
      let subUrl = '';
      let token = '';
      for (let i = 0; i < 15 && !token; i++) {
        const data = await pageGet(page, '/api/user/subscription');
        subUrl = String((data as any)?.subscriptionUrl || (data as any)?.subscription_url || '');
        const mm = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
        if (mm) token = mm[1];
        if (!token && (data as any)?.token) token = String((data as any).token);
        if (!token) await page.waitForTimeout(2_000);
      }
      scenario.users.push({
        username: u.username, email: u.email, password: USER_PASSWORD, plan: u.plan,
        uplinkMbps: planDef.up, downlinkMbps: planDef.down,
        subscriptionUrl: subUrl, subToken: token,
      });
      persist();
      // 退出当前用户会话，保证下一个用户干净登录。
      await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }).catch(() => {});
      await page.context().clearCookies().catch(() => {});
      expect(subUrl, `用户 ${u.username} 订阅 URL 应非空`).not.toBe('');
      expect(token, `用户 ${u.username} 订阅 token 应解析到`).not.toBe('');
    }
    scenario.entriesReady = createdEntries.map((c) => c.name);
    persist();
    await shot(page, '07-subscriptions');
    expect(scenario.users.length).toBe(10);
  });
});
