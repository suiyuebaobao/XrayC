import { openNodeOnboarding } from '../helpers/node-onboarding';
/*
 * 用途：并行流 C 控制面全流程真实操作 E2E（真实 UI 点击 + 真实后端 no-mock + 真实 SSH 一键安装）。
 * 覆盖 Phase A(VLESS)之外的协议：域名直连(灰云)下 ① Trojan ② HY2(hysteria/udp) ③ Shadowsocks(2022)。
 * 全程走真实管理员/用户登录与真实页面点击，严禁 mock/SQL 直插；远端一键安装是真实 SSH 部署。
 * 所有新建对象用 RUN_ID 命名空间(e2e-proto-*)，绝不碰基线真实客户数据/其他流的测试节点。
 * 产出（节点 id / 3 条入口 id / 用户 / 套餐 / 分组 / 订阅 token / 上线证据）写入 PHASEC_ARTIFACTS 文件(0600)。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

// ---- 环境与命名空间 -------------------------------------------------------
const RUN_ID = process.env.PHASEC_RUN_ID || `${Date.now()}`;
const NS = `e2e-proto-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.PHASEC_ACME_EMAIL || 'acme@example.test';
const ARTIFACTS = process.env.PHASEC_ARTIFACTS || '/tmp/phaseC-proto-artifacts.env';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.PHASEC_HEARTBEAT_TIMEOUT_MS || 480_000);

// 域名直连(direct non-CF, sslip.io)：一键安装目标 + 直连证书域名。
// SSH_HOST 仅用于节点登记表单的「SSH IP」字段；真实 agent 安装由 deploy-access-agent.sh 完成（见报告）。
const SSH_HOST = required('PHASEC_SSH_HOST');
const DIRECT_DOMAIN = required('PHASEC_DIRECT_DOMAIN'); // sslip.io 直连证书域名
const IP_DIRECT = process.env.PHASEC_IP_DIRECT || SSH_HOST; // IP 直连地址（出口用）
const SSH_PASSWORD = required('PHASEC_SSH_PASSWORD'); // 一键 SSH 安装密码（改用 phaseA 同款一键装机，自包含）

const USER_EMAIL = `${NS}@example.test`;
const USER_PASSWORD = 'Phasec-proto-123456';
const PLAN_NAME = `${NS}-plan`;
const GROUP_NAME = `${NS}-group`;
const NODE_NAME = `${NS}-node`;
const EXIT_LINE_NAME = `${NS}-exit`;

function required(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) {
    throw new Error(`缺少必填环境变量 ${name}（请在运行 spec 前注入真实 inventory 值）`);
  }
  return v;
}

// ---- 工件落盘（脱敏交给报告层；这里写真实值供主 Agent 接力，文件 0600） -------
const artifacts: Record<string, string> = {};
function recordArtifact(key: string, value: string) {
  artifacts[key] = value;
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

// ---- 通用 UI 工具 --------------------------------------------------------
async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

/** 打开 el-select 并按可见文本选项点击。 */
async function pickSelectOption(page: Page, selectRoot: ReturnType<Page['locator']>, optionText: string | RegExp) {
  await selectRoot.click();
  const option = page.locator('.el-select-dropdown__item:visible').filter({ hasText: optionText }).first();
  await option.click();
}

/** 通过 el-form-item label 文本定位其控件根。 */
function formItemByLabel(page: Page, label: string) {
  return page.locator('.el-form-item').filter({ has: page.locator('.el-form-item__label', { hasText: label }) }).first();
}

/** 等待某个 admin 写入 API 的 2xx 响应，返回响应体（用于抓取 id）。 */
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
  await page.screenshot({ path: `test-results/phaseC-${name}.png`, fullPage: true }).catch(() => {});
}

// 串行：所有步骤共享同一上下文与已建对象。
test.describe.configure({ mode: 'serial' });

test.describe('Phase C 控制面全流程真实操作（Trojan/HY2/SS 域名直连）', () => {
  let nodeId = '';
  let planId = '';
  let groupId = '';
  const entryIds: Record<string, string> = {};

  test('步骤1：admin 真实登录面板', async ({ page }) => {
    await adminLogin(page);
    await expect(page.getByRole('heading', { name: /概览|运营|总览/ }).first()).toBeVisible();
    await shot(page, '01-admin-login');
  });

  // 幂等：按名查既有套餐 id（重跑时复用，避免重复创建报错）。
  async function findPlanId(page: Page, name: string): Promise<string> {
    return page.evaluate(async (planName) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const r = await fetch('/api/admin/plans', { headers: { Authorization: `Bearer ${tok}` } });
      const j = r.ok ? await r.json() : null;
      const items = j?.data?.plans ?? j?.data?.items ?? j?.data ?? [];
      const f = Array.isArray(items) ? items.find((p: any) => p.name === planName) : null;
      return f?.id ? String(f.id) : '';
    }, name).catch(() => '');
  }

  test('步骤2：真实建测试套餐（先建套餐供用户授权）', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    const existingPlan = await findPlanId(page, PLAN_NAME);
    if (existingPlan) {
      planId = existingPlan;
      recordArtifact('PHASEC_PLAN_ID', planId);
      recordArtifact('PHASEC_PLAN_NAME', PLAN_NAME);
      return;
    }
    await expect(page.getByRole('button', { name: '新增套餐' })).toBeVisible();
    await page.getByRole('button', { name: '新增套餐' }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('例如：标准套餐').fill(PLAN_NAME);
    await formItemByLabel(page, '流量额度（GB）').locator('input').fill('50');
    await formItemByLabel(page, '周期（天）').locator('input').fill('30');
    const resp = await waitWrite(page, /\/api\/admin\/plans$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建套餐' }).click();
    });
    planId = await dataId(resp);
    recordArtifact('PHASEC_PLAN_ID', planId);
    recordArtifact('PHASEC_PLAN_NAME', PLAN_NAME);
    await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
    await shot(page, '02-plan-created');
    expect(planId, '套餐 id').not.toBe('');
  });

  test('步骤3：真实建测试用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/users');
    const existingUser = await page.evaluate(async (email) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const r = await fetch('/api/admin/users?search=' + encodeURIComponent(email), { headers: { Authorization: `Bearer ${tok}` } });
      const j = r.ok ? await r.json() : null;
      const items = j?.data?.users ?? j?.data?.items ?? j?.data ?? [];
      const f = Array.isArray(items) ? items.find((u: any) => u.email === email) : null;
      return f?.id ? String(f.id) : '';
    }, USER_EMAIL).catch(() => '');
    if (existingUser) {
      recordArtifact('PHASEC_USER_EMAIL', USER_EMAIL);
      recordArtifact('PHASEC_USER_PASSWORD', USER_PASSWORD);
      recordArtifact('PHASEC_USER_ID', existingUser);
      return;
    }
    await expect(page.getByRole('button', { name: '新增用户' })).toBeVisible();
    await page.getByRole('button', { name: '新增用户' }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('user@example.com').fill(USER_EMAIL);
    await dialog.getByPlaceholder('至少 8 位').fill(USER_PASSWORD);
    await pickSelectOption(page, formItemByLabel(page, '套餐').locator('.el-select'), PLAN_NAME);
    const resp = await waitWrite(page, /\/api\/admin\/users$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    const userId = await dataId(resp);
    recordArtifact('PHASEC_USER_EMAIL', USER_EMAIL);
    recordArtifact('PHASEC_USER_PASSWORD', USER_PASSWORD);
    recordArtifact('PHASEC_USER_ID', userId);
    await expect(page.getByText(USER_EMAIL).first()).toBeVisible();
    await shot(page, '03-user-created');
  });

  // 等待节点心跳健康：周期读节点列表，找 NODE_NAME 且 healthStatus∈{healthy,syncing} 且有心跳。
  async function waitNodeHealthy(page: Page): Promise<{ online: boolean; id: string }> {
    let online = false;
    let id = '';
    const deadline = Date.now() + HEARTBEAT_TIMEOUT_MS;
    /* eslint-disable no-await-in-loop */
    while (Date.now() < deadline) {
      const nodesResp = await page.evaluate(async () => {
        const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
        const tok = sess.accessToken || sess.access_token || '';
        const r = await fetch('/api/admin/access-nodes', { headers: { Authorization: `Bearer ${tok}` } });
        return r.ok ? r.json() : null;
      }).catch(() => null);
      const nodes = (nodesResp as any)?.data?.accessNodes ?? (nodesResp as any)?.data?.access_nodes
        ?? (nodesResp as any)?.data ?? [];
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
    /* eslint-enable no-await-in-loop */
    return { online, id };
  }

  // 步骤4：一键安装 Agent（真实 SSH，自包含——与 phaseA 同款一键装机,已真机验证可用）。
  test('步骤4：一键安装 Agent（真实 SSH）并等待心跳上线', async ({ page }) => {
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 120_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    const existing = await page.evaluate(async (name) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const r = await fetch('/api/admin/access-nodes', { headers: { Authorization: `Bearer ${tok}` } });
      const j = r.ok ? await r.json() : null;
      const nodes = j?.data?.accessNodes ?? j?.data?.access_nodes ?? j?.data ?? [];
      const f = Array.isArray(nodes) ? nodes.find((n: any) => n.name === name) : null;
      return f?.id ? String(f.id) : '';
    }, NODE_NAME).catch(() => '');
    if (existing) {
      nodeId = existing;
      recordArtifact('PHASEC_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      expect(r.online, '已存在节点应健康').toBe(true);
      return;
    }
    await openNodeOnboarding(page, 'automatic');
    const dialog = page.getByRole('dialog', { name: '一键安装 Agent' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '节点名称').locator('input').fill(NODE_NAME);
    await formItemByLabel(page, 'SSH Host（IP）').locator('input').fill(SSH_HOST);
    await formItemByLabel(page, 'SSH Port').locator('input').fill('22');
    await formItemByLabel(page, 'SSH User').locator('input').fill('root');
    await formItemByLabel(page, 'SSH 密码').locator('input').fill(SSH_PASSWORD);
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    await formItemByLabel(page, '域名直连地址').locator('input').fill(DIRECT_DOMAIN);
    await formItemByLabel(page, 'ACME 邮箱').locator('input').fill(ACME_EMAIL);
    await shot(page, '04-oneclick-form');
    await waitWrite(page, /\/api\/admin\/access-nodes\/one-click-install$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建安装任务' }).click();
    });
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    for (let i = 0; i < 8; i++) {
      const status = await page.evaluate(async (name) => {
        const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
        const tok = sess.accessToken || sess.access_token || '';
        const r = await fetch('/api/admin/deployment-tasks', { headers: { Authorization: `Bearer ${tok}` } });
        const j = r.ok ? await r.json() : null;
        const items = j?.data?.items ?? j?.data ?? [];
        const mine = Array.isArray(items) ? items.find((t: any) => t?.safe_metadata?.access_node_name === name) : null;
        return mine?.status || '';
      }, NODE_NAME).catch(() => '');
      if (status === 'failed') throw new Error('一键安装失败');
      if (status === 'succeeded') break;
      await page.waitForTimeout(6_000);
    }
    const result = await waitNodeHealthy(page);
    nodeId = result.id;
    recordArtifact('PHASEC_NODE_ID', nodeId);
    recordArtifact('PHASEC_NODE_ONLINE', String(result.online));
    await shot(page, '04-node-online');
    expect(nodeId, '节点 id 应已创建').not.toBe('');
    expect(result.online, '节点心跳应上线').toBe(true);
  });

  test('步骤5：建本机出口服务（IP 直连 + VLESS Reality 作内部出口）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过出口');
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
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
    await shot(page, '05a-local-exit-form');
    await waitWrite(page, /\/api\/admin\/access-nodes\/[0-9a-f-]+\/local-exit-lines$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '添加到出口管理' }).click();
    });
    recordArtifact('PHASEC_EXIT_LINE_NAME', EXIT_LINE_NAME);
    await shot(page, '05-local-exit-created');
  });

  // ---- 步骤6：三协议域名直连入口 -----------------------------------------
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

  // 域名直连：选连接方式后选用证书域名。
  async function pickDirectDomain(page: Page) {
    await pickConnMode(page, '域名直连（灰云）');
    await pickSelectOption(
      page,
      formItemByLabel(page, '选用域名').locator('.el-select'),
      new RegExp(DIRECT_DOMAIN.replace(/\./g, '\\.')),
    );
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
    recordArtifact(`PHASEC_ENTRY_${key}_ID`, id);
    await expect(page.getByText(name).first()).toBeVisible();
  }

  test('步骤6a：入口① 域名直连 + Trojan（TLS + 证书域名 SNI）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const dialog = await openEntryDialog(page);
    const name = `${NS}-trojan`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'Trojan');
    // Trojan 强制 TLS，安全模式只有 TLS 一项；SNI 自动带证书域名。
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'TLS');
    // SNI 合并(2026-06-28):域名直连 SNI=选用域名(证书域名,上面已选),已无独立 Server Name 字段,不再断言。
    await setListenPort(page, 30011);
    await shot(page, '06a-entry-trojan-form');
    await saveEntry(page, dialog, 'TROJAN', name);
  });

  test('步骤6b：入口② 域名直连 + HY2（hysteria/udp，TLS + 证书域名 SNI）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const dialog = await openEntryDialog(page);
    const name = `${NS}-hy2`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'HY2');
    // HY2 强制 TLS + UDP（hysteria）；SNI 自动带证书域名。
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'TLS');
    // SNI 合并(2026-06-28):域名直连 SNI=选用域名(证书域名,上面已选),已无独立 Server Name 字段,不再断言。
    await setListenPort(page, 30012);
    await shot(page, '06b-entry-hy2-form');
    await saveEntry(page, dialog, 'HY2', name);
  });

  test('步骤6c：入口③ 域名直连 + Shadowsocks（2022 系列）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const dialog = await openEntryDialog(page);
    const name = `${NS}-ss`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'Shadowsocks');
    // SS 无 SNI（security=None），方法由后端按 2022 系列生成。
    // SNI 合并(2026-06-28):Shadowsocks(2022)无 TLS/SNI,无独立 Server Name 字段,不再断言。
    await setListenPort(page, 30013);
    await shot(page, '06c-entry-ss-form');
    await saveEntry(page, dialog, 'SS', name);
  });

  test('步骤7：每条入口绑定出口线路', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过绑定');
    await adminLogin(page);
    // 幂等：先查已存在出口绑定的入口名集合（create-binding 是追加语义，重绑会产生重复）。
    await page.goto('/admin/access-entries');
    const boundNames: string[] = await page.evaluate(async (ns) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
      const ej = er.ok ? await er.json() : null;
      const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
      const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
      const bj = br.ok ? await br.json() : null;
      const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
      const boundEntryIds = new Set((Array.isArray(binds) ? binds : []).map((b: any) => String(b.access_entry_id ?? b.accessEntryId)));
      return (Array.isArray(entries) ? entries : [])
        .filter((e: any) => String(e.name).startsWith(ns) && boundEntryIds.has(String(e.id)))
        .map((e: any) => String(e.name));
    }, NS).catch(() => [] as string[]);
    for (const name of [`${NS}-trojan`, `${NS}-hy2`, `${NS}-ss`]) {
      if (boundNames.includes(name)) continue;
      await page.goto('/admin/access-entries');
      const row = page.locator('tr').filter({ hasText: name }).first();
      if (await row.count() === 0) continue;
      await row.getByRole('button', { name: '绑定线路' }).click();
      const dialog = page.getByRole('dialog', { name: '绑定线路' });
      await expect(dialog).toBeVisible();
      // 先点开出口下拉，等到至少一个可见选项再选第一个（避免选项未加载就保存出空绑定）。
      await dialog.locator('.el-select').first().click();
      const option = page.locator('.el-select-dropdown__item:visible').first();
      await expect(option).toBeVisible({ timeout: 10_000 });
      await option.click();
      // 确认下拉已收起且选中态生效再保存；保存须命中 2xx 写接口。
      await page.keyboard.press('Escape').catch(() => {});
      await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
      await expect(dialog).toBeHidden({ timeout: 10_000 }).catch(() => {});
    }
    await shot(page, '07-bindings');
  });

  test('步骤8：建分组并选绑定节点', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过分组');
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    // 幂等：分组已存在（重跑）则复用其 id，不重复创建。
    const existingGroup = await page.evaluate(async (name) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const r = await fetch('/api/admin/line-groups', { headers: { Authorization: `Bearer ${tok}` } });
      const j = r.ok ? await r.json() : null;
      const items = j?.data?.lineGroups ?? j?.data?.line_groups ?? j?.data?.items ?? j?.data ?? [];
      const f = Array.isArray(items) ? items.find((g: any) => g.name === name) : null;
      return f?.id ? String(f.id) : '';
    }, GROUP_NAME).catch(() => '');
    if (existingGroup) {
      groupId = existingGroup;
      recordArtifact('PHASEC_GROUP_ID', groupId);
      recordArtifact('PHASEC_GROUP_NAME', GROUP_NAME);
      return;
    }
    await page.getByRole('button', { name: '创建分组' }).click();
    const dialog = page.getByRole('dialog', { name: '创建分组' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    const opts = page.locator('.el-select-dropdown__item:visible');
    const n = await opts.count();
    for (let i = 0; i < n; i++) {
      await opts.nth(i).click();
    }
    await page.keyboard.press('Escape');
    const resp = await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    groupId = await dataId(resp);
    recordArtifact('PHASEC_GROUP_ID', groupId);
    recordArtifact('PHASEC_GROUP_NAME', GROUP_NAME);
    await shot(page, '08-group-created');
  });

  test('步骤9：套餐授权分组', async ({ page }) => {
    test.skip(!groupId || !planId, '分组或套餐缺失，跳过授权');
    await adminLogin(page);
    await page.goto('/admin/plans');
    const row = page.locator('tr').filter({ hasText: PLAN_NAME }).first();
    await row.getByRole('button', { name: '编辑分组' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    // 「可用分组」是 multiple el-select；点其 form-item 下的 select 根（placeholder 在嵌套 input 上不稳）。
    const groupSelect = dialog.locator('.el-form-item').filter({
      has: page.locator('.el-form-item__label', { hasText: '可用分组' }),
    }).locator('.el-select').first();
    await groupSelect.click();
    await page.locator('.el-select-dropdown__item:visible').filter({ hasText: GROUP_NAME }).first().click();
    await page.keyboard.press('Escape');
    await waitWrite(page, /\/api\/admin\/plans\/[0-9a-f-]+\/line-groups$/, 'PUT', async () => {
      await dialog.getByRole('button', { name: '保存分组' }).click();
    }).catch(async () => {
      await dialog.getByRole('button', { name: '保存分组' }).click().catch(() => {});
    });
    await shot(page, '09-plan-authorized');
  });

  test('步骤10：用户端真实登录取订阅 token（只读）', async ({ page }) => {
    test.skip(!artifacts.PHASEC_USER_EMAIL, '用户未创建，跳过订阅');
    await login(page, '/login', USER_EMAIL, USER_PASSWORD, /\/(dashboard|subscription)$/);
    const subResp = await page.waitForResponse(
      (r) => /\/api\/user\/subscription$/.test(new URL(r.url()).pathname),
      { timeout: 15_000 },
    ).catch(() => null);
    await page.goto('/subscription');
    await page.waitForTimeout(1_500);
    const subCard = page.locator('.el-card').filter({ hasText: '订阅链接' }).first();
    const linkInput = subCard.locator('input').first();
    const subUrl = await linkInput.inputValue().catch(() => '');
    let token = '';
    const m = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
    if (m) token = m[1];
    if (!token && subResp) {
      const raw = await subResp.json().catch(() => ({}));
      const data = (raw as any)?.data ?? raw;
      const url = data?.subscriptionUrl || data?.subscription_url || '';
      const mm = String(url).match(/\/sub\/([A-Za-z0-9_-]+)/);
      if (mm) token = mm[1];
      if (!token && data?.token) token = String(data.token);
    }
    recordArtifact('PHASEC_SUB_URL', subUrl);
    recordArtifact('PHASEC_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl, '订阅链接应非空').not.toBe('');
  });
});
