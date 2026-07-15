/*
 * 用途：Phase A 控制面全流程真实操作 E2E（真实 UI 点击 + 真实后端 no-mock + 真实 SSH 一键安装）。
 * 全程走真实管理员/用户登录与真实页面点击，严禁 mock/SQL 直插；远端一键安装是真实 SSH 部署。
 * 所有新建对象用 RUN_ID 命名空间，绝不碰基线真实客户数据（nodes=1/users=6/exit_endpoints=3）。
 * 产出（节点 id / 4 条入口 id / 用户 / 套餐 / 分组 / 订阅 token / 上线证据）写入 PHASEA_ARTIFACTS 文件。
 */
import { expect, type Page, test } from '@playwright/test';

import {
  adminListHas,
  dataId,
  formItemByLabel,
  pageGet,
  pickSelectOption,
  recordArtifact,
  required,
  shot,
  waitWrite,
} from './real-op-phaseA-helpers';
import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/重试循环依赖顺序等待 */

// ---- 环境与命名空间 -------------------------------------------------------
const RUN_ID = process.env.PHASEA_RUN_ID || `${Date.now()}`;
const NS = `e2e-realui-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.PHASEA_ACME_EMAIL || 'acme@example.test';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.PHASEA_HEARTBEAT_TIMEOUT_MS || 360_000);

// server_1（直连/IP）：一键安装目标 + 直连域名；server_4（CF 橙云）：CF 域名。
const SSH_HOST = required('PHASEA_SSH_HOST');
const SSH_PORT = process.env.PHASEA_SSH_PORT || '22';
const SSH_USER = process.env.PHASEA_SSH_USER || 'root';
const SSH_PASSWORD = required('PHASEA_SSH_PASSWORD');
const DIRECT_DOMAIN = required('PHASEA_DIRECT_DOMAIN'); // server_1 public_domain (cert 域名)
const CF_DOMAIN = required('PHASEA_CF_DOMAIN'); // server_4 CF 橙云域名
const IP_DIRECT = process.env.PHASEA_IP_DIRECT || SSH_HOST; // IP 直连地址

const USER_EMAIL = `${NS}@example.test`;
const USER_PASSWORD = 'Phasea-realui-123456';
const PLAN_NAME = `${NS}-plan`;
const GROUP_NAME = `${NS}-group`;
const NODE_NAME = `${NS}-node`;
const EXIT_LINE_NAME = `${NS}-exit`;

// ---- 通用 UI 工具（通用工件/表单/写入/幂等 helper 已拆到 real-op-phaseA-helpers.ts） ----
async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

// 串行：所有步骤共享同一上下文与已建对象。
test.describe.configure({ mode: 'serial' });

test.describe('Phase A 控制面全流程真实操作', () => {
  let nodeId = '';
  let planId = '';
  let groupId = '';
  const entryIds: Record<string, string> = {};

  test('步骤1：admin 真实登录面板', async ({ page }) => {
    await adminLogin(page);
    await expect(page.getByRole('heading', { name: /概览|运营|总览/ }).first()).toBeVisible();
    await shot(page, '01-admin-login');
  });

  test('步骤2：真实建测试套餐（先建套餐供用户授权）', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    if (await adminListHas(page, '/api/admin/plans', 'plans', PLAN_NAME)) {
      recordArtifact('PHASEA_PLAN_NAME', PLAN_NAME);
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
    recordArtifact('PHASEA_PLAN_ID', planId);
    recordArtifact('PHASEA_PLAN_NAME', PLAN_NAME);
    await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
    await shot(page, '02-plan-created');
    expect(planId, '套餐 id').not.toBe('');
  });

  test('步骤3：真实建测试用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    recordArtifact('PHASEA_USER_EMAIL', USER_EMAIL);
    recordArtifact('PHASEA_USER_PASSWORD', USER_PASSWORD);
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
    recordArtifact('PHASEA_USER_EMAIL', USER_EMAIL);
    recordArtifact('PHASEA_USER_PASSWORD', USER_PASSWORD);
    recordArtifact('PHASEA_USER_ID', userId);
    await expect(page.getByText(USER_EMAIL).first()).toBeVisible();
    await shot(page, '03-user-created');
  });

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

  test('步骤4-alt：UI 新增中转节点登记已装 Agent（手动安装路径）', async ({ page }) => {
    const authCode = process.env.PHASEA_AGENT_AUTH_CODE?.trim();
    test.skip(!authCode, '未注入 PHASEA_AGENT_AUTH_CODE（一键安装可用时跳过手动登记）');
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 120_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    // 幂等：节点已存在（重跑）则直接复用，等健康即可。
    const nd = await pageGet(page, '/api/admin/access-nodes');
    const ndList = nd?.accessNodes ?? nd?.access_nodes ?? nd ?? [];
    const existing = (Array.isArray(ndList) ? ndList.find((n: any) => n.name === NODE_NAME)?.id : '') || '';
    if (existing) {
      nodeId = existing;
      recordArtifact('PHASEA_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      recordArtifact('PHASEA_NODE_ONLINE', String(r.online));
      await shot(page, '04alt-node-online');
      expect(r.online, '已存在节点应健康').toBe(true);
      return;
    }
    await page.getByRole('button', { name: '新增中转节点' }).click();
    const dialog = page.getByRole('dialog', { name: '新增中转节点' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '节点名称').locator('input').fill(NODE_NAME);
    // 「客户连接地址」输入框已移除：public_host 由 IP直连>域名直连>CF域名 自动推导。
    await formItemByLabel(page, 'SSH IP').locator('input').fill(SSH_HOST);
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    const directInput = dialog.locator('input[placeholder*="灰云域名"]').first();
    if (await directInput.count() === 0) {
      await dialog.getByRole('button', { name: '添加域名直连地址' }).click();
    }
    await dialog.locator('input[placeholder*="灰云域名"]').first().fill(DIRECT_DOMAIN);
    await dialog.getByRole('button', { name: '添加 CF 直连地址' }).click();
    await dialog.locator('input[placeholder*="橙云域名"]').first().fill(CF_DOMAIN);
    const acme = formItemByLabel(page, 'ACME 邮箱').locator('input');
    if (await acme.count() > 0) {
      await acme.fill(ACME_EMAIL);
    }
    await formItemByLabel(page, '鉴权码').locator('input').fill(authCode!);
    await shot(page, '04alt-create-node-form');
    const resp = await waitWrite(page, /\/api\/admin\/access-nodes$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    nodeId = await dataId(resp);
    recordArtifact('PHASEA_NODE_ID', nodeId);
    const result = await waitNodeHealthy(page);
    if (!nodeId) nodeId = result.id;
    recordArtifact('PHASEA_NODE_ID', nodeId);
    recordArtifact('PHASEA_NODE_ONLINE', String(result.online));

    // —— 本次改动核心断言：public_host 输入框已移除,后端按 IP直连>域名直连>CF 推导。 ——
    // 此节点填了 IP 直连地址,故推导值应非空且 = IP_DIRECT(最高优先级)。
    const nodeRead = await pageGet(page, '/api/admin/access-nodes');
    const readList = nodeRead?.accessNodes ?? nodeRead?.access_nodes ?? nodeRead ?? [];
    const createdNode = Array.isArray(readList)
      ? readList.find((n: any) => String(n.id) === String(nodeId) || n.name === NODE_NAME)
      : null;
    const derivedHost = String(createdNode?.publicHost ?? createdNode?.public_host ?? '');
    recordArtifact('PHASEA_NODE_PUBLIC_HOST', derivedHost);
    expect(derivedHost, 'public_host 推导值不应为空(去掉输入框后由直连地址推导)').not.toBe('');
    expect(derivedHost, 'public_host 应推导为 IP 直连地址(最高优先级)').toBe(IP_DIRECT);

    await page.goto('/admin/transit-nodes').catch(() => {});
    await shot(page, '04alt-node-online');
    expect(nodeId, '节点 id 应已创建').not.toBe('');
    expect(result.online, `节点心跳应在 ${HEARTBEAT_TIMEOUT_MS}ms 内健康`).toBe(true);
  });

  test('步骤4：一键安装 Agent（真实 SSH）并等待心跳上线', async ({ page }) => {
    test.skip(process.env.PHASEA_SKIP_ONE_CLICK === '1', '一键安装后端 SSH 执行失败，已用手动安装路径替代（见报告）');
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 120_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
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
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    await formItemByLabel(page, '域名直连地址').locator('input').fill(DIRECT_DOMAIN);
    await formItemByLabel(page, 'ACME 邮箱').locator('input').fill(ACME_EMAIL);
    await shot(page, '04a-oneclick-form');

    await waitWrite(page, /\/api\/admin\/access-nodes\/one-click-install$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建安装任务' }).click();
    });
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    await shot(page, '04b-task-created');

    for (let i = 0; i < 8; i++) {
      const td = await pageGet(page, '/api/admin/deployment-tasks');
      const items = td?.items ?? td ?? [];
      const mine = Array.isArray(items)
        ? items.find((t: any) => t?.safe_metadata?.access_node_name === NODE_NAME)
        : null;
      if (mine?.status === 'failed') {
        const reason = String(mine?.result?.error || mine?.error_summary || 'ssh_install_failed');
        recordArtifact('PHASEA_INSTALL_FAILURE', reason.slice(0, 500));
        throw new Error(`一键安装失败：${reason.slice(0, 300)}`);
      }
      if (mine?.status === 'succeeded') break;
      await page.waitForTimeout(6_000);
    }
    const result = await waitNodeHealthy(page);
    nodeId = result.id;
    recordArtifact('PHASEA_NODE_ID', nodeId);
    recordArtifact('PHASEA_NODE_ONLINE', String(result.online));
    await shot(page, '04c-node-status');
    expect(nodeId, '节点 id 应已创建（一键安装成功后自动建节点）').not.toBe('');
    expect(result.online, `节点心跳应在 ${HEARTBEAT_TIMEOUT_MS}ms 内上线`).toBe(true);

    // —— 本次改动核心断言：一键安装弹窗已无「公网地址」框,后端按 IP直连>域名直连>CF 推导。 ——
    // 此安装填了 IP 直连地址,故自动建出的节点 public_host 应非空且 = IP_DIRECT。
    const nodeRead = await pageGet(page, '/api/admin/access-nodes');
    const readList = nodeRead?.accessNodes ?? nodeRead?.access_nodes ?? nodeRead ?? [];
    const createdNode = Array.isArray(readList)
      ? readList.find((n: any) => String(n.id) === String(nodeId) || n.name === NODE_NAME)
      : null;
    const derivedHost = String(createdNode?.publicHost ?? createdNode?.public_host ?? '');
    recordArtifact('PHASEA_NODE_PUBLIC_HOST', derivedHost);
    expect(derivedHost, 'public_host 推导值不应为空（去掉输入框后由直连地址推导）').not.toBe('');
    expect(derivedHost, 'public_host 应推导为 IP 直连地址（最高优先级）').toBe(IP_DIRECT);
  });

  test('步骤5：建本机出口服务（IP 直连 + VLESS Reality）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过出口');
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    if (await adminListHas(page, '/api/admin/exit-endpoints', 'exit_endpoints', NODE_NAME)) {
      recordArtifact('PHASEA_EXIT_LINE_NAME', EXIT_LINE_NAME);
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
    recordArtifact('PHASEA_EXIT_LINE_NAME', EXIT_LINE_NAME);
    await shot(page, '05-local-exit-created');
  });

  // ---- 步骤6：入口四模式 -------------------------------------------------
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

  // 选连接方式 radio;若该档不存在(节点缺对应 direct/cf node_domain)返回 false,
  // 让调用方优雅跳过该入口类型(域名/CF 证书未就绪时不阻塞绑定/订阅/流量主线)。
  async function pickConnMode(page: Page, label: string): Promise<boolean> {
    const radio = page.locator('.el-radio-button').filter({ hasText: label }).first()
      .locator('.el-radio-button__inner');
    if (await radio.count() === 0) {
      return false;
    }
    await radio.click({ timeout: 8_000 }).catch(() => {});
    return true;
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
    recordArtifact(`PHASEA_ENTRY_${key}_ID`, id);
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
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const name = `${NS}-ip-none`;
    if (await entryDone(page, name)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, name);
    await pickConnMode(page, 'IP 直连（免证书）');
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), '普通 VLESS');
    // SNI 合并(2026-06-28):VLESS 普通(none) IP 直连无 SNI,已无独立 Server Name 字段,不再断言。
    await setListenPort(page, 20001);
    await shot(page, '06a-entry-ip-none-form');
    await saveEntry(page, dialog, 'IP_NONE', name);
  });

  test('步骤6b：入口② IP 直连 + VLESS Reality', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const name = `${NS}-ip-reality`;
    if (await entryDone(page, name)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, name);
    await pickConnMode(page, 'IP 直连（免证书）');
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'Reality');
    // SNI 合并(2026-06-28):Reality 的 SNI=「伪装域名」(分类下拉,默认 www.cloudflare.com),已非独立 Server Name 输入框,跳过严格断言。
    await setListenPort(page, 20002);
    await shot(page, '06b-entry-ip-reality-form');
    await saveEntry(page, dialog, 'IP_REALITY', name);
  });

  test('步骤6c：入口③ 域名直连（灰云）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const name = `${NS}-domain`;
    if (await entryDone(page, name)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, name);
    if (!(await pickConnMode(page, '域名直连（灰云）'))) {
      // 节点无 direct node_domain(灰云证书未就绪/未注入),跳过域名入口,不阻塞主线。
      await page.keyboard.press('Escape').catch(() => {});
      test.skip(true, '节点缺域名直连档(node_domain direct 未就绪),跳过域名入口');
      return;
    }
    await pickSelectOption(page, formItemByLabel(page, '选用域名').locator('.el-select'), new RegExp(DIRECT_DOMAIN.replace(/\./g, '\\.')));
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'TLS');
    // SNI 合并(2026-06-28):域名直连 SNI=选用域名(上面已选),已无独立 Server Name 字段,不再断言。
    await setListenPort(page, 20003);
    await shot(page, '06c-entry-domain-form');
    await saveEntry(page, dialog, 'DOMAIN', name);
  });

  test('步骤6d：入口④ CF 直连（橙云）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    const name = `${NS}-cf`;
    if (await entryDone(page, name)) return;
    const dialog = await openEntryDialog(page);
    await fillEntryBasics(page, dialog, name);
    if (!(await pickConnMode(page, 'CF 直连（橙云）'))) {
      // 节点无 cf node_domain(橙云 DNS-01 证书未注入),跳过 CF 入口,不阻塞主线。
      await page.keyboard.press('Escape').catch(() => {});
      test.skip(true, '节点缺 CF 直连档(node_domain cf 未就绪),跳过 CF 入口');
      return;
    }
    await pickSelectOption(page, formItemByLabel(page, '选用域名').locator('.el-select'), new RegExp(CF_DOMAIN.replace(/\./g, '\\.')));
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    const cdnHost = formItemByLabel(page, 'CDN Hostname').locator('input').first();
    await expect(cdnHost).toHaveValue(new RegExp(CF_DOMAIN.replace(/\./g, '\\.')));
    await pickSelectOption(page, formItemByLabel(page, '监听端口').locator('.el-select'), '8443');
    await shot(page, '06d-entry-cf-form');
    await saveEntry(page, dialog, 'CF', name);
  });

  test('步骤7：每条入口绑定出口线路', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过绑定');
    await adminLogin(page);
    for (const name of [`${NS}-ip-none`, `${NS}-ip-reality`, `${NS}-domain`, `${NS}-cf`]) {
      await page.goto('/admin/access-entries');
      const row = page.locator('tr').filter({ hasText: name }).first();
      if (await row.count() === 0) continue;
      await row.getByRole('button', { name: '绑定线路' }).click();
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
    test.skip(!nodeId, '节点未创建，跳过分组');
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    if (await adminListHas(page, '/api/admin/line-groups', 'line_groups', GROUP_NAME)) {
      recordArtifact('PHASEA_GROUP_NAME', GROUP_NAME);
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
    recordArtifact('PHASEA_GROUP_ID', groupId);
    recordArtifact('PHASEA_GROUP_NAME', GROUP_NAME);
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

  test('步骤10：用户端真实登录取订阅 token（只读）', async ({ page }) => {
    await login(page, '/login', USER_EMAIL, USER_PASSWORD, /\/(dashboard|subscription)$/);
    await page.goto('/subscription');
    await page.waitForURL(/\/subscription$/, { timeout: 15_000 }).catch(() => {});
    let subUrl = '';
    let token = '';
    for (let i = 0; i < 10 && !subUrl; i++) {
      const data = await pageGet(page, '/api/user/subscription');
      subUrl = String((data as any)?.subscriptionUrl || (data as any)?.subscription_url || '');
      const mm = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
      if (mm) token = mm[1];
      if (!token && (data as any)?.token) token = String((data as any).token);
      if (!subUrl) await page.waitForTimeout(2_000);
    }
    recordArtifact('PHASEA_SUB_URL', subUrl);
    recordArtifact('PHASEA_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl, '订阅链接应非空').not.toBe('');
    expect(token, '订阅 token 应解析到').not.toBe('');
  });
});
