import { openNodeOnboarding } from '../helpers/node-onboarding';
/*
 * 用途：server_4（Cloudflare 橙云）CF 真实流量 E2E —— 真实 UI 点击 + 真实后端 no-mock + 真实 SSH 一键安装。
 * 关键：无 CF API Token 时，agent 经 CF 橙云 :80 反代做 HTTP-01，给 CF 域名各签【自有】证书（per-domain，绝不复用直连灰云证书；用户 2026-06-29 定）。
 * 节点配两类对外地址：
 *   - 域名直连（kind=direct，灰云）= 节点IP.sslip.io → HTTP-01 签自有证书（与 CF 域名各签各的、互不复用）。
 *   - CF 直连（kind=cf，橙云）= server_4 的橙云域名 → CF 入口对外、藏中转真实 IP。
 * CF 入口 = VLESS + WS + TLS，监听 CF 支持端口 443。
 * 全程真实管理员/用户登录与真实页面点击，严禁 mock/SQL 直插。所有对象用 RUN_ID 命名空间（e2e-cf-*）。
 * 产出写 CF_ARTIFACTS（0600）供主 Agent 接力做真实流量验证与脱敏报告。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/重试循环依赖顺序等待 */

const RUN_ID = process.env.CF_RUN_ID || `${Date.now()}`;
const NS = process.env.CF_NS || `e2e-cf-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.CF_ACME_EMAIL || 'acme@example.test';
const ARTIFACTS = process.env.CF_ARTIFACTS || '/tmp/cf-artifacts.env';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.CF_HEARTBEAT_TIMEOUT_MS || 600_000);

const SSH_HOST = required('CF_SSH_HOST');
const SSH_PORT = process.env.CF_SSH_PORT || '22';
const SSH_USER = process.env.CF_SSH_USER || 'root';
const SSH_PASSWORD = required('CF_SSH_PASSWORD');
const DIRECT_DOMAIN = required('CF_DIRECT_DOMAIN'); // 灰云直连域名（sslip），HTTP-01 签证书
const CF_DOMAIN = required('CF_CF_DOMAIN'); // 橙云 CF 域名
const IP_DIRECT = process.env.CF_IP_DIRECT || SSH_HOST;
const CF_LISTEN_PORT = Number(process.env.CF_LISTEN_PORT || 443);
const INSTALL_DIR = process.env.CF_INSTALL_DIR || '/opt/xrayc-access-cf/agent';
const COMPOSE_PROJECT = process.env.CF_COMPOSE_PROJECT || 'xrayc-access';
const CERT_TIMEOUT_MS = Number(process.env.CF_CERT_TIMEOUT_MS || 300_000);

const USER_EMAIL = `${NS}@example.test`;
const USER_PASSWORD = 'Cfreuse-realui-123456';
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

const artifacts: Record<string, string> = {};
function recordArtifact(key: string, value: string) {
  artifacts[key] = String(value).replace(/[\r\n]+/g, ' ').slice(0, 600);
  const lines: string[] = [];
  const existing = existsSync(ARTIFACTS) ? readFileSync(ARTIFACTS, 'utf8') : '';
  const known = new Set(Object.keys(artifacts));
  for (const line of existing.split('\n')) {
    const m = line.match(/^([A-Z0-9_]+)=/);
    if (m && known.has(m[1])) continue;
    if (line.trim()) lines.push(line);
  }
  for (const [k, v] of Object.entries(artifacts)) lines.push(`${k}=${v}`);
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
    { timeout: 45_000 },
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
  await page.screenshot({ path: `test-results/cf-${name}.png`, fullPage: true }).catch(() => {});
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
  return Array.isArray(arr) && arr.some((x) => JSON.stringify(x).toLowerCase().includes(needle.toLowerCase()));
}

async function findNode(page: Page): Promise<any | null> {
  const data = await pageGet(page, '/api/admin/access-nodes');
  const nodes = data?.accessNodes ?? data?.access_nodes ?? data ?? [];
  return Array.isArray(nodes) ? nodes.find((n: any) => n.name === NODE_NAME) || null : null;
}

test.describe.configure({ mode: 'serial' });

test.describe('CF 橙云真实流量（无 token，per-domain HTTP-01 穿 CF，不复用）server_4', () => {
  let nodeId = '';
  let planId = '';
  let groupId = '';
  let cfEntryId = '';

  test('步骤1：admin 真实登录面板', async ({ page }) => {
    await adminLogin(page);
    await expect(page.getByRole('heading', { name: /概览|运营|总览/ }).first()).toBeVisible();
    await shot(page, '01-admin-login');
  });

  test('步骤2：真实建测试套餐', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/plans');
    if (await adminListHas(page, '/api/admin/plans', 'plans', PLAN_NAME)) {
      recordArtifact('CF_PLAN_NAME', PLAN_NAME);
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
    recordArtifact('CF_PLAN_ID', planId);
    recordArtifact('CF_PLAN_NAME', PLAN_NAME);
    await shot(page, '02-plan-created');
    expect(planId).not.toBe('');
  });

  test('步骤3：真实建测试用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    recordArtifact('CF_USER_EMAIL', USER_EMAIL);
    recordArtifact('CF_USER_PASSWORD', USER_PASSWORD);
    await page.goto('/admin/users');
    if (await adminListHas(page, '/api/admin/users', 'users', USER_EMAIL)) return;
    await expect(page.getByRole('button', { name: '新增用户' })).toBeVisible();
    await page.getByRole('button', { name: '新增用户' }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('user@example.com').fill(USER_EMAIL);
    await dialog.getByPlaceholder('至少 8 位').fill(USER_PASSWORD);
    await pickSelectOption(page, formItemByLabel(page, '套餐').locator('.el-select'), PLAN_NAME);
    const resp = await waitWrite(page, /\/api\/admin\/users$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    recordArtifact('CF_USER_ID', await dataId(resp));
    await shot(page, '03-user-created');
  });

  async function waitNodeHealthy(page: Page): Promise<{ online: boolean; id: string }> {
    let online = false;
    let id = '';
    const deadline = Date.now() + HEARTBEAT_TIMEOUT_MS;
    while (Date.now() < deadline) {
      const found = await findNode(page);
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

  test('步骤4：一键安装 Agent（真实 SSH，无 token、per-domain HTTP-01 穿 CF）并等待心跳上线', async ({ page }) => {
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 180_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    const existing = (await findNode(page))?.id || '';
    if (existing) {
      nodeId = existing;
      recordArtifact('CF_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      recordArtifact('CF_NODE_ONLINE', String(r.online));
      await shot(page, '04-node-online');
      expect(r.online).toBe(true);
      return;
    }
    await expect(page.getByRole('button', { name: '新增中转节点', exact: true })).toBeVisible();
    await openNodeOnboarding(page, 'automatic');
    const dialog = page.getByRole('dialog', { name: '一键安装 Agent' });
    await expect(dialog).toBeVisible();

    await formItemByLabel(page, '节点名称').locator('input').fill(NODE_NAME);
    await formItemByLabel(page, 'SSH Host（IP）').locator('input').fill(SSH_HOST);
    await formItemByLabel(page, 'SSH Port').locator('input').fill(SSH_PORT);
    await formItemByLabel(page, 'SSH User').locator('input').fill(SSH_USER);
    await formItemByLabel(page, 'SSH 密码').locator('input').fill(SSH_PASSWORD);
    await formItemByLabel(page, '安装目录').locator('input').fill(INSTALL_DIR);
    await formItemByLabel(page, 'Compose Project').locator('input').fill(COMPOSE_PROJECT);
    // 三类对外地址全填：IP 直连 + 域名直连（sslip 灰云，HTTP-01）+ CF 直连（橙云）。
    await formItemByLabel(page, 'IP 直连地址').locator('input').fill(IP_DIRECT);
    await formItemByLabel(page, '域名直连地址').locator('input').fill(DIRECT_DOMAIN);
    await formItemByLabel(page, 'CF 直连地址').locator('input').fill(CF_DOMAIN);
    // 填了域名/CF → 需 ACME 邮箱签证书。
    await expect(formItemByLabel(page, 'ACME 邮箱')).toBeVisible();
    await formItemByLabel(page, 'ACME 邮箱').locator('input').fill(ACME_EMAIL);
    // 关键：CF API Token 留空 → 无 token，agent 经 CF 橙云 :80 反代做 HTTP-01，给 CF 域名签【自有】证书（per-domain，不复用直连灰云）。
    recordArtifact('CF_CERT_MODE', 'http01_through_cf_no_token_per_domain');
    await shot(page, '04a-oneclick-form');

    await waitWrite(page, /\/api\/admin\/access-nodes\/one-click-install$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '创建安装任务' }).click();
    });
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    await shot(page, '04b-task-created');

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
        recordArtifact('CF_INSTALL_FAILURE', reason.slice(0, 500));
        throw new Error(`一键安装失败：${reason.slice(0, 300)}`);
      }
      if (mine?.status === 'succeeded') {
        taskTerminal = 'succeeded';
        recordArtifact('CF_DEPLOY_TASK_STATUS', 'succeeded');
        break;
      }
      await page.waitForTimeout(8_000);
    }
    const result = await waitNodeHealthy(page);
    nodeId = result.id;
    recordArtifact('CF_NODE_ID', nodeId);
    recordArtifact('CF_NODE_ONLINE', String(result.online));
    recordArtifact('CF_DEPLOY_TASK_STATUS', taskTerminal || 'unknown');
    await shot(page, '04c-node-status');
    expect(nodeId).not.toBe('');
    expect(result.online).toBe(true);
  });

  test('步骤4.5：确认/补齐 node_domains（kind=direct sslip + kind=cf 橙云），等直连证书签发', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
    test.setTimeout(CERT_TIMEOUT_MS + 120_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');

    const domainsOf = (n: any): Array<{ kind: string; domain: string; cert_status?: string }> =>
      (n?.domains ?? []).map((d: any) => ({
        kind: d.kind, domain: d.domain,
        cert_status: d.certStatus ?? d.cert_status,
      }));

    let node = await findNode(page);
    let doms = domainsOf(node);
    const hasDirect = doms.some((d) => d.kind === 'direct' && d.domain === DIRECT_DOMAIN);
    const hasCf = doms.some((d) => d.kind === 'cf' && d.domain === CF_DOMAIN);
    recordArtifact('CF_INSTALL_DOMAINS', JSON.stringify(doms));
    recordArtifact('CF_INSTALL_HAS_DIRECT', String(hasDirect));
    recordArtifact('CF_INSTALL_HAS_CF', String(hasCf));

    // 若一键安装没把某个域名落成 node_domain → 用「编辑中转节点」弹窗 NodeDomainsField 真实 UI 补齐。
    if (!hasDirect || !hasCf) {
      const nodeCard = page.locator('.relay-card').filter({ hasText: NODE_NAME }).first();
      await expect(nodeCard).toBeVisible();
      // 编辑按钮（节点卡片上）。
      await nodeCard.getByRole('button', { name: /编辑/ }).first().click();
      const dialog = page.getByRole('dialog', { name: '编辑中转节点' });
      await expect(dialog).toBeVisible();
      // 补 direct 行。
      if (!hasDirect) {
        await dialog.getByRole('button', { name: '添加域名直连地址' }).click();
        const directInputs = dialog.locator('input[placeholder*="灰云域名"]');
        await directInputs.last().fill(DIRECT_DOMAIN);
      }
      // 补 cf 行。
      if (!hasCf) {
        await dialog.getByRole('button', { name: '添加 CF 直连地址' }).click();
        const cfInputs = dialog.locator('input[placeholder*="橙云域名"]');
        await cfInputs.last().fill(CF_DOMAIN);
      }
      // ACME 邮箱（签证书必填）。
      const acme = formItemByLabel(page, 'ACME 邮箱').locator('input').first();
      if (await acme.count() > 0) await acme.fill(ACME_EMAIL);
      await shot(page, '045a-edit-domains');
      await waitWrite(page, /\/api\/admin\/access-nodes\/[0-9a-f-]+$/, 'PUT', async () => {
        await dialog.getByRole('button', { name: /保存/ }).first().click();
      });
      recordArtifact('CF_DOMAINS_ADDED_VIA_EDIT', 'true');
    } else {
      recordArtifact('CF_DOMAINS_ADDED_VIA_EDIT', 'false_oneclick_persisted_both');
    }

    // 等直连域名（sslip）证书签发：HTTP-01 经节点 80 口，cert_status=valid。
    let certValid = false;
    const certDeadline = Date.now() + CERT_TIMEOUT_MS;
    while (Date.now() < certDeadline) {
      node = await findNode(page);
      doms = domainsOf(node);
      const direct = doms.find((d) => d.kind === 'direct' && d.domain === DIRECT_DOMAIN);
      if (direct && (direct.cert_status === 'valid' || direct.cert_status === 'active')) {
        certValid = true;
        break;
      }
      await page.waitForTimeout(8_000);
    }
    recordArtifact('CF_DIRECT_CERT_DOMAINS', JSON.stringify(doms));
    recordArtifact('CF_DIRECT_CERT_VALID', String(certValid));
    await shot(page, '045b-cert-status');
    expect(certValid, '直连域名 HTTP-01 证书应在超时内签发为 valid（CF 域名各签自有证书、不复用此证书）').toBe(true);
  });

  test('步骤5：建本机出口服务（IP 直连，作内部出口）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    if (await adminListHas(page, '/api/admin/exit-endpoints', 'exit_endpoints', NODE_NAME)) {
      recordArtifact('CF_EXIT_LINE_NAME', EXIT_LINE_NAME);
      return;
    }
    const nodeCard = page.locator('.relay-card').filter({ hasText: NODE_NAME }).first();
    await expect(nodeCard).toBeVisible();
    await nodeCard.getByRole('button', { name: '本机出口服务' }).click();
    const dialog = page.getByRole('dialog', { name: '本机出口服务' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: '添加线路' }).click();
    const nameInput = dialog.locator('input[placeholder*="香港自建"]').first();
    if (await nameInput.count() > 0) await nameInput.fill(EXIT_LINE_NAME);
    const addrInput = dialog.locator('input[placeholder*="自动读取中转节点地址"]').first();
    if (await addrInput.count() > 0 && ((await addrInput.inputValue().catch(() => '')) || '').trim() === '') {
      await addrInput.fill(IP_DIRECT);
    }
    await waitWrite(page, /\/api\/admin\/access-nodes\/[0-9a-f-]+\/local-exit-lines$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '添加到出口管理' }).click();
    });
    recordArtifact('CF_EXIT_LINE_NAME', EXIT_LINE_NAME);
    await shot(page, '05-local-exit-created');
  });

  test('步骤6：建 CF 直连（橙云）入口 VLESS/WS/TLS 监听 443', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    if (await adminListHas(page, '/api/admin/access-entries', 'access_entries', ENTRY_CF_NAME)) {
      recordArtifact('CF_LISTEN_PORT', String(CF_LISTEN_PORT));
      return;
    }
    await expect(page.getByRole('button', { name: '创建入口' })).toBeVisible();
    await page.getByRole('button', { name: '创建入口' }).click();
    const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: '创建入口' }) });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '入口名称').locator('input').first().fill(ENTRY_CF_NAME);
    await pickSelectOption(page, formItemByLabel(page, '接入节点').locator('.el-select'), new RegExp(NODE_NAME));
    await page.locator('.el-radio-button').filter({ hasText: 'CF 直连（橙云）' }).first()
      .locator('.el-radio-button__inner').click();
    await pickSelectOption(
      page,
      formItemByLabel(page, '选用域名').locator('.el-select'),
      new RegExp(CF_DOMAIN.replace(/\./g, '\\.')),
    );
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'VLESS');
    const cdnHost = formItemByLabel(page, 'CDN Hostname').locator('input').first();
    await expect(cdnHost).toBeVisible();
    await expect(cdnHost).toHaveValue(new RegExp(CF_DOMAIN.replace(/\./g, '\\.')));
    recordArtifact('CF_CDN_HOSTNAME_OK', 'true');
    await pickSelectOption(page, formItemByLabel(page, '监听端口').locator('.el-select'), String(CF_LISTEN_PORT));
    await shot(page, '06-entry-cf-form');
    const resp = await waitWrite(page, /\/api\/admin\/access-entries$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    cfEntryId = await dataId(resp);
    recordArtifact('CF_ENTRY_CF_ID', cfEntryId);
    recordArtifact('CF_LISTEN_PORT', String(CF_LISTEN_PORT));
    await expect(page.getByText(ENTRY_CF_NAME).first()).toBeVisible();
    expect(cfEntryId).not.toBe('');
  });

  test('步骤7：CF 入口绑定出口线路', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
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
      recordArtifact('CF_BINDING_OK', 'true');
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
    recordArtifact('CF_BINDING_OK', 'true');
    await shot(page, '07-binding');
  });

  test('步骤8：建分组并选绑定节点', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    if (await adminListHas(page, '/api/admin/line-groups', 'line_groups', GROUP_NAME)) {
      recordArtifact('CF_GROUP_NAME', GROUP_NAME);
      return;
    }
    await page.getByRole('button', { name: '创建分组' }).click();
    const dialog = page.getByRole('dialog', { name: '创建分组' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    await page.locator('.el-select-dropdown__item:visible').filter({ hasText: new RegExp(NODE_NAME) }).first().click();
    await page.keyboard.press('Escape');
    const resp = await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    groupId = await dataId(resp);
    recordArtifact('CF_GROUP_ID', groupId);
    recordArtifact('CF_GROUP_NAME', GROUP_NAME);
    await shot(page, '08-group-created');
  });

  test('步骤9：套餐授权分组', async ({ page }) => {
    test.skip(!nodeId, '节点未创建');
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
    recordArtifact('CF_PLAN_AUTHORIZED', 'true');
    await shot(page, '09-plan-authorized');
  });

  test('步骤10：用户端真实登录取订阅 token', async ({ page }) => {
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
    recordArtifact('CF_SUB_URL', subUrl);
    recordArtifact('CF_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl).not.toBe('');
    expect(token).not.toBe('');
  });
});
