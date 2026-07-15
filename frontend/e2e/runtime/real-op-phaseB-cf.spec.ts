/*
 * 用途：并行流 B 控制面全流程真实操作 E2E（真实 UI 点击 + 真实后端 no-mock + 真实 SSH 一键安装）。
 * 专攻 CF 直连（橙云）：节点在 server_4（CF 橙云机），CF 域名 DNS-01 证书，入口 VLESS/WS/TLS 监听 CF HTTPS 端口（8443）。
 * 全程走真实管理员/用户登录与真实页面点击，严禁 mock/SQL 直插；一键安装是真实 SSH 部署（控制面用公网面板地址，bug ①）。
 * 所有新建对象用 RUN_ID 命名空间（e2e-b-*），绝不碰 server_4 上的 AIO 栈/基线/其他流测试数据。
 * 产出（节点 id / CF 入口 id / 用户 / 套餐 / 分组 / 订阅 token / 上线证据）写入 PHASEB_ARTIFACTS 文件（0600）。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/重试循环依赖顺序等待 */

// ---- 环境与命名空间 -------------------------------------------------------
const RUN_ID = process.env.PHASEB_RUN_ID || `${Date.now()}`;
const NS = process.env.PHASEB_NS || `e2e-b-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.PHASEB_ACME_EMAIL || 'acme@example.test';
const ARTIFACTS = process.env.PHASEB_ARTIFACTS || '/tmp/phaseB-cf-artifacts.env';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.PHASEB_HEARTBEAT_TIMEOUT_MS || 600_000);

// server_4（CF 橙云）：一键安装目标 IP + CF 橙云域名（DNS-01 证书）。
const SSH_HOST = required('PHASEB_SSH_HOST');
const SSH_PORT = process.env.PHASEB_SSH_PORT || '22';
const SSH_USER = process.env.PHASEB_SSH_USER || 'root';
const SSH_PASSWORD = required('PHASEB_SSH_PASSWORD');
const CF_DOMAIN = required('PHASEB_CF_DOMAIN'); // server_4 CF 橙云域名
const CF_API_TOKEN = required('PHASEB_CF_API_TOKEN'); // Cloudflare API Token（DNS-01）
const IP_DIRECT = process.env.PHASEB_IP_DIRECT || SSH_HOST; // IP 直连地址（本机出口用）
const CF_LISTEN_PORT = Number(process.env.PHASEB_CF_LISTEN_PORT || 8443);
const INSTALL_DIR = process.env.PHASEB_INSTALL_DIR || '/opt/xrayc/access-agent';
const COMPOSE_PROJECT = process.env.PHASEB_COMPOSE_PROJECT || 'xrayc-access';

const USER_EMAIL = `${NS}@example.test`;
const USER_PASSWORD = 'Phaseb-realui-123456';
const PLAN_NAME = `${NS}-plan`;
const GROUP_NAME = `${NS}-group`;
const NODE_NAME = `${NS}-node`;
const EXIT_LINE_NAME = `${NS}-exit`;
const ENTRY_CF_NAME = `${NS}-cf`;

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
  await page.screenshot({ path: `test-results/phaseB-${name}.png`, fullPage: true }).catch(() => {});
}

// 用页面登录会话的 Bearer 在浏览器内 GET（只读）admin/user 接口，返回 data。
async function pageGet(page: Page, apiPath: string): Promise<any> {
  return page.evaluate(async (p) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch(p, { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    return j?.data ?? j;
  }, apiPath).catch(() => null);
}

// 幂等检查：判断列表里是否已存在含 needle 的对象（重跑安全）。
async function adminListHas(page: Page, apiPath: string, listKey: string, needle: string) {
  const data = await pageGet(page, apiPath);
  const arr = (data?.[listKey] ?? data?.items ?? data) as any[];
  return Array.isArray(arr) && arr.some((x) => JSON.stringify(x).includes(needle));
}

// 串行：所有步骤共享同一上下文与已建对象。
test.describe.configure({ mode: 'serial' });

test.describe('Phase B 控制面全流程真实操作（CF 直连/橙云 server_4）', () => {
  let nodeId = '';
  let planId = '';
  let groupId = '';
  let cfEntryId = '';

  test('步骤1：admin 真实登录面板', async ({ page }) => {
    await adminLogin(page);
    await expect(page.getByRole('heading', { name: /概览|运营|总览/ }).first()).toBeVisible();
    await shot(page, '01-admin-login');
  });

  test('步骤2：真实建测试套餐（先建套餐供用户授权）', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    if (await adminListHas(page, '/api/admin/plans', 'plans', PLAN_NAME)) {
      recordArtifact('PHASEB_PLAN_NAME', PLAN_NAME);
      await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
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
    recordArtifact('PHASEB_PLAN_ID', planId);
    recordArtifact('PHASEB_PLAN_NAME', PLAN_NAME);
    await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
    await shot(page, '02-plan-created');
    expect(planId, '套餐 id').not.toBe('');
  });

  test('步骤3：真实建测试用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    recordArtifact('PHASEB_USER_EMAIL', USER_EMAIL);
    recordArtifact('PHASEB_USER_PASSWORD', USER_PASSWORD);
    await page.goto('/admin/users');
    if (await adminListHas(page, '/api/admin/users', 'users', USER_EMAIL)) {
      await expect(page.getByText(USER_EMAIL).first()).toBeVisible();
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
    recordArtifact('PHASEB_USER_ID', userId);
    await expect(page.getByText(USER_EMAIL).first()).toBeVisible();
    await shot(page, '03-user-created');
  });

  // 等待节点心跳健康：周期读节点列表，找 NODE_NAME 且 healthStatus∈{healthy,syncing} 且有心跳。
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

  test('步骤4：一键安装 Agent（真实 SSH + CF 橙云 DNS-01 证书）并等待心跳上线', async ({ page }) => {
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 180_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    // 幂等：节点已存在（重跑）则复用，等健康即可。
    const nd = await pageGet(page, '/api/admin/access-nodes');
    const ndList = nd?.accessNodes ?? nd?.access_nodes ?? nd ?? [];
    const existing = (Array.isArray(ndList) ? ndList.find((n: any) => n.name === NODE_NAME)?.id : '') || '';
    if (existing) {
      nodeId = existing;
      recordArtifact('PHASEB_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      recordArtifact('PHASEB_NODE_ONLINE', String(r.online));
      await shot(page, '04-node-online');
      expect(r.online, '已存在节点应健康').toBe(true);
      return;
    }
    await expect(page.getByRole('button', { name: '一键安装 Agent' })).toBeVisible();
    await page.getByRole('button', { name: '一键安装 Agent' }).click();
    const dialog = page.getByRole('dialog', { name: '一键安装 Agent' });
    await expect(dialog).toBeVisible();

    await formItemByLabel(page, '节点名称').locator('input').fill(NODE_NAME);
    // 「公网地址」输入框已移除：public_host 由 IP直连>域名直连>CF域名 自动推导。
    await formItemByLabel(page, 'SSH Host（IP）').locator('input').fill(SSH_HOST);
    await formItemByLabel(page, 'SSH Port').locator('input').fill(SSH_PORT);
    await formItemByLabel(page, 'SSH User').locator('input').fill(SSH_USER);
    await formItemByLabel(page, 'SSH 密码').locator('input').fill(SSH_PASSWORD);
    // 安装目录/Compose Project 用命名空间隔离值（与 AIO 栈彻底分开）。
    await formItemByLabel(page, '安装目录').locator('input').fill(INSTALL_DIR);
    await formItemByLabel(page, 'Compose Project').locator('input').fill(COMPOSE_PROJECT);
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    // CF 直连地址 → 启用 CF；触发 ACME 邮箱 + CF API Token 字段出现。
    await formItemByLabel(page, 'CF 直连地址').locator('input').fill(CF_DOMAIN);
    await expect(formItemByLabel(page, 'ACME 邮箱')).toBeVisible();
    await formItemByLabel(page, 'ACME 邮箱').locator('input').fill(ACME_EMAIL);
    await expect(formItemByLabel(page, 'CF API Token')).toBeVisible();
    await formItemByLabel(page, 'CF API Token').locator('input').fill(CF_API_TOKEN);
    await shot(page, '04a-oneclick-form');

    await waitWrite(page, /\/api\/admin\/access-nodes\/one-click-install$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建安装任务' }).click();
    });
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    await shot(page, '04b-task-created');

    // 轮询部署任务状态：失败立刻记原因并抛错；成功后转等心跳。
    let taskTerminal = '';
    const taskDeadline = Date.now() + HEARTBEAT_TIMEOUT_MS;
    while (Date.now() < taskDeadline) {
      const td = await pageGet(page, '/api/admin/deployment-tasks');
      const items = td?.items ?? td ?? [];
      const mine = Array.isArray(items)
        ? items.find((t: any) => t?.safe_metadata?.access_node_name === NODE_NAME)
        : null;
      if (mine?.status === 'failed') {
        const reason = String(mine?.result?.error || mine?.error_summary || 'ssh_install_failed');
        recordArtifact('PHASEB_INSTALL_FAILURE', reason.slice(0, 500));
        taskTerminal = 'failed';
        throw new Error(`一键安装失败：${reason.slice(0, 300)}`);
      }
      if (mine?.status === 'succeeded') {
        taskTerminal = 'succeeded';
        recordArtifact('PHASEB_DEPLOY_TASK_STATUS', 'succeeded');
        break;
      }
      await page.waitForTimeout(8_000);
    }
    const result = await waitNodeHealthy(page);
    nodeId = result.id;
    recordArtifact('PHASEB_NODE_ID', nodeId);
    recordArtifact('PHASEB_NODE_ONLINE', String(result.online));
    recordArtifact('PHASEB_DEPLOY_TASK_STATUS', taskTerminal || 'unknown');
    await page.goto('/admin/transit-nodes').catch(() => {});
    await shot(page, '04c-node-status');
    expect(nodeId, '节点 id 应已创建（一键安装成功后自动建节点）').not.toBe('');
    expect(result.online, `节点心跳应在 ${HEARTBEAT_TIMEOUT_MS}ms 内上线`).toBe(true);
  });

  test('步骤5：建本机出口服务（IP 直连，作内部出口）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过出口');
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    if (await adminListHas(page, '/api/admin/exit-endpoints', 'exit_endpoints', NODE_NAME)) {
      recordArtifact('PHASEB_EXIT_LINE_NAME', EXIT_LINE_NAME);
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
    recordArtifact('PHASEB_EXIT_LINE_NAME', EXIT_LINE_NAME);
    await shot(page, '05-local-exit-created');
  });

  test('步骤6：建 CF 直连（橙云）入口 VLESS/WS/TLS 监听 CF HTTPS 端口', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    if (await adminListHas(page, '/api/admin/access-entries', 'access_entries', ENTRY_CF_NAME)) {
      recordArtifact('PHASEB_CF_LISTEN_PORT', String(CF_LISTEN_PORT));
      await expect(page.getByText(ENTRY_CF_NAME).first()).toBeVisible();
      return;
    }
    await expect(page.getByRole('button', { name: '创建入口' })).toBeVisible();
    await page.getByRole('button', { name: '创建入口' }).click();
    const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: '创建入口' }) });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '入口名称').locator('input').first().fill(ENTRY_CF_NAME);
    await pickSelectOption(page, formItemByLabel(page, '接入节点').locator('.el-select'), new RegExp(NODE_NAME));
    // 选「CF 直连（橙云）」连接方式 → 开 CDN 橙云代理藏中转 IP。
    await page.locator('.el-radio-button').filter({ hasText: 'CF 直连（橙云）' }).first()
      .locator('.el-radio-button__inner').click();
    await pickSelectOption(
      page,
      formItemByLabel(page, '选用域名').locator('.el-select'),
      new RegExp(CF_DOMAIN.replace(/\./g, '\\.')),
    );
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    // 确认 CDN 表单自动出现 + CDN Hostname 默认填 CF 域名（不再有手动「启用 CDN」开关）。
    const cdnHost = formItemByLabel(page, 'CDN Hostname').locator('input').first();
    await expect(cdnHost).toBeVisible();
    await expect(cdnHost).toHaveValue(new RegExp(CF_DOMAIN.replace(/\./g, '\\.')));
    recordArtifact('PHASEB_CF_CDN_HOSTNAME_OK', 'true');
    // 监听端口限 CF 支持的 HTTPS 端口集（443/8443/2053/2083/2087/2096），选 8443。
    await pickSelectOption(page, formItemByLabel(page, '监听端口').locator('.el-select'), String(CF_LISTEN_PORT));
    await shot(page, '06-entry-cf-form');
    const resp = await waitWrite(page, /\/api\/admin\/access-entries$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    cfEntryId = await dataId(resp);
    recordArtifact('PHASEB_ENTRY_CF_ID', cfEntryId);
    recordArtifact('PHASEB_CF_LISTEN_PORT', String(CF_LISTEN_PORT));
    await expect(page.getByText(ENTRY_CF_NAME).first()).toBeVisible();
    expect(cfEntryId, 'CF 入口 id').not.toBe('');
  });

  test('步骤7：CF 入口绑定出口线路', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过绑定');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    // 幂等：已绑定则跳过。
    const bound = await page.evaluate(async (name) => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
      const ej = er.ok ? await er.json() : null;
      const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
      const e = (Array.isArray(entries) ? entries : []).find((x: any) => x.name === name);
      if (!e?.id) return false;
      const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
      const bj = br.ok ? await br.json() : null;
      const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
      return (Array.isArray(binds) ? binds : []).some(
        (b: any) => String(b.access_entry_id ?? b.accessEntryId) === String(e.id),
      );
    }, ENTRY_CF_NAME).catch(() => false);
    if (bound) {
      recordArtifact('PHASEB_BINDING_OK', 'true');
      return;
    }
    const row = page.locator('tr').filter({ hasText: ENTRY_CF_NAME }).first();
    await expect(row).toBeVisible();
    await row.getByRole('button', { name: '绑定线路' }).click();
    const dialog = page.getByRole('dialog', { name: '绑定线路' });
    await expect(dialog).toBeVisible();
    await dialog.locator('.el-select').first().click();
    const option = page.locator('.el-select-dropdown__item:visible').first();
    await expect(option).toBeVisible({ timeout: 10_000 });
    await option.click();
    await page.keyboard.press('Escape').catch(() => {});
    await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    recordArtifact('PHASEB_BINDING_OK', 'true');
    await shot(page, '07-binding');
  });

  test('步骤8：建分组并选绑定节点', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过分组');
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    if (await adminListHas(page, '/api/admin/line-groups', 'line_groups', GROUP_NAME)) {
      recordArtifact('PHASEB_GROUP_NAME', GROUP_NAME);
      return;
    }
    await page.getByRole('button', { name: '创建分组' }).click();
    const dialog = page.getByRole('dialog', { name: '创建分组' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    // 只选本流的节点，避免误绑其他流/基线节点。
    await page.locator('.el-select-dropdown__item:visible').filter({ hasText: new RegExp(NODE_NAME) }).first().click();
    await page.keyboard.press('Escape');
    const resp = await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    groupId = await dataId(resp);
    recordArtifact('PHASEB_GROUP_ID', groupId);
    recordArtifact('PHASEB_GROUP_NAME', GROUP_NAME);
    await shot(page, '08-group-created');
  });

  test('步骤9：套餐授权分组', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过授权');
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
    recordArtifact('PHASEB_PLAN_AUTHORIZED', 'true');
    await shot(page, '09-plan-authorized');
  });

  test('步骤9.5：新功能 UI——内核状态显示 + 重启节点按钮/二次确认弹窗（不真重启）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过新功能 UI 验证');
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');

    // 先从节点列表 API 取该节点真实的内核字段（新 6.8 内核 + act_connmark 可加载 → 正常/不待升级）。
    const nd = await pageGet(page, '/api/admin/access-nodes');
    const ndList = nd?.accessNodes ?? nd?.access_nodes ?? nd ?? [];
    const myNode = (Array.isArray(ndList) ? ndList : []).find((n: any) => n.name === NODE_NAME);
    expect(myNode, '应能在节点列表找到本流节点').toBeTruthy();
    const connmarkAvail = myNode?.kernelConnmarkAvailable ?? myNode?.kernel_connmark_available;
    const upgradePending = myNode?.kernelUpgradePending ?? myNode?.kernel_upgrade_pending;
    recordArtifact('PHASEB_KERNEL_CONNMARK_AVAILABLE', String(connmarkAvail));
    recordArtifact('PHASEB_KERNEL_UPGRADE_PENDING', String(upgradePending));
    // 新内核 6.8 + act_connmark 可加载：connmark 应可用、不应标记待升级。
    expect(connmarkAvail, 'kernel_connmark_available 应为 true（新内核 act_connmark 可加载）').not.toBe(false);
    expect(upgradePending, 'kernel_upgrade_pending 不应为 true（新内核无需升级）').not.toBe(true);

    // 定位本流节点卡片。
    const nodeCard = page.locator('.relay-card').filter({ hasText: NODE_NAME }).first();
    await expect(nodeCard).toBeVisible();

    // 内核状态显示：正常态下「内核待升级·需重启」warning 标签应不存在（渲染条件为 connmark 不可用或待升级）。
    const kernelTag = nodeCard.locator('.el-tag', { hasText: '内核待升级·需重启' });
    const kernelTagCount = await kernelTag.count();
    recordArtifact('PHASEB_KERNEL_UPGRADE_TAG_VISIBLE', String(kernelTagCount > 0));
    expect(kernelTagCount, '新内核正常态不应渲染「内核待升级·需重启」标签').toBe(0);
    await shot(page, '09e-kernel-status-normal');

    // 重启节点按钮存在且可点（danger text 按钮，文案「重启节点」）。
    const rebootBtn = nodeCard.locator('.el-button', { hasText: '重启节点' }).first();
    await expect(rebootBtn, '节点卡片应有「重启节点」按钮').toBeVisible();
    await expect(rebootBtn).toBeEnabled();
    recordArtifact('PHASEB_REBOOT_BUTTON_PRESENT', 'true');

    // 点开二次确认弹窗（ElMessageBox.confirm，type=warning），核对标题/正文/按钮文案——但绝不真重启，点「取消」。
    await rebootBtn.click();
    const msgBox = page.locator('.el-message-box');
    await expect(msgBox, '应弹出重启二次确认框').toBeVisible({ timeout: 10_000 });
    const boxTitle = (await msgBox.locator('.el-message-box__title').innerText().catch(() => '')).trim();
    const boxBody = (await msgBox.locator('.el-message-box__message, .el-message-box__content').innerText().catch(() => '')).trim();
    recordArtifact('PHASEB_REBOOT_CONFIRM_TITLE', boxTitle);
    recordArtifact('PHASEB_REBOOT_CONFIRM_BODY', boxBody.slice(0, 200));
    await expect(msgBox.locator('.el-message-box__title')).toContainText('重启中转节点');
    // 警告文案须含「整机重启」「停止服务」「数分钟」「确认重启」等关键风险提示。
    await expect(msgBox).toContainText('整机重启');
    await expect(msgBox).toContainText('停止服务');
    await expect(msgBox).toContainText('数分钟');
    await expect(msgBox.locator('button', { hasText: '确认重启' })).toBeVisible();
    await expect(msgBox.locator('button', { hasText: '取消' })).toBeVisible();
    // type=warning 校验：弹窗带 warning 图标类。
    const hasWarnIcon = await msgBox.locator('.el-message-box__status.el-icon-warning, .el-message-box__status').count();
    recordArtifact('PHASEB_REBOOT_CONFIRM_WARNING', String(hasWarnIcon > 0));
    await shot(page, '09f-reboot-confirm-dialog');
    // 红线：默认只验弹窗，不实际触发重启——点「取消」关闭。
    await msgBox.locator('button', { hasText: '取消' }).click();
    await expect(msgBox).toBeHidden({ timeout: 5_000 });
    recordArtifact('PHASEB_REBOOT_CANCELLED_NOT_TRIGGERED', 'true');
  });

  test('步骤10：用户端真实登录取订阅 token（只读）', async ({ page }) => {
    await login(page, '/login', USER_EMAIL, USER_PASSWORD, /\/(dashboard|subscription)$/);
    await page.goto('/subscription');
    await page.waitForURL(/\/subscription$/, { timeout: 15_000 }).catch(() => {});
    let subUrl = '';
    let token = '';
    for (let i = 0; i < 12 && !token; i++) {
      const data = await pageGet(page, '/api/user/subscription');
      subUrl = String((data as any)?.subscriptionUrl || (data as any)?.subscription_url || '');
      const mm = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
      if (mm) token = mm[1];
      if (!token && (data as any)?.token) token = String((data as any).token);
      if (!token) await page.waitForTimeout(2_000);
    }
    recordArtifact('PHASEB_SUB_URL', subUrl);
    recordArtifact('PHASEB_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl, '订阅链接应非空').not.toBe('');
    expect(token, '订阅 token 应解析到').not.toBe('');
  });
});
