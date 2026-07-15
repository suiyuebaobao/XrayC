/*
 * 用途：Phase5 给现有「压测-分组」补建 3 种新协议入口（全真实后台 UI，admin/admin123456，面板 127.0.0.1:8080）。
 * 复用 phase3-setup 的真实点击口径，零 API/DB/curl 造数；所有对象创建一律真实弹窗点击。
 * 建：Trojan（带域名节点 b4d05aad，灰云直连域名+TLS）+ HY2（b4d05aad，灰云直连域名+TLS，UDP）
 *     + Shadowsocks 2022（IP 直连免证书，挂 b4d05aad，后端自动注入 2022-blake3-aes-128-gcm）。
 * 每条入口绑定同节点本机出口（复用现有压测 VLESS 入口所用出口），再【编辑】「压测-分组」把 3 条新绑定加进去
 *     （组已存在，保留原 3 条 VLESS 绑定不动），这样 10 个压测用户自动拿到全部 4 种协议节点。
 * 验证：10 用户真实登录重下订阅，下载 Clash YAML 断言含 vless/trojan/hysteria2/ss 共 6 个节点。
 * 产出真实订阅信息（含新协议）落 PHASE3_SCENARIO_OUT（0600，真实 token 不进报告）。
 * 现有节点按 id 前缀解析：5b364d06=CF橙云（仅 cf 域名）、b4d05aad=带域名（coordcode.com 直连）。
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';

/* eslint-disable no-await-in-loop -- 轮询/逐用户顺序操作依赖顺序等待 */

const ADMIN = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const PASS = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const OUT = process.env.PHASE3_SCENARIO_OUT
  || '/tmp/claude-0/-root-suiyue/2b33ac0a-52d2-420f-830c-f512ac403e79/scratchpad/phase3-scenario.json';

const USER_PASSWORD = 'Press-Phase3-123456';
const GROUP_NAME = '压测-分组';

// 10 个压测用户（与 phase3 同口径）：L 系限速、U 系不限速；邮箱小写。
const USERS = [
  ...[1, 2, 3, 4, 5].map((i) => ({ username: `pressL${i}`, email: `pressl${i}@test.local`, plan: '压测-限速' })),
  ...[1, 2, 3, 4, 5].map((i) => ({ username: `pressU${i}`, email: `pressu${i}@test.local`, plan: '压测-不限速' })),
];

// 3 条新入口定义：protocolLabel=协议下拉文案；carriage=承载下拉文案（仅 SS 显式选，留空走默认）。
const ENTRIES = [
  { key: 'trojan', name: '压测-Trojan入口', nodePrefix: 'b4d05aad', mode: 'domain' as const,
    modeLabel: '域名直连（灰云）', protocolLabel: 'Trojan', protocolValue: 'trojan',
    port: 18443, carriage: '', clashType: 'trojan', desc: '灰云直连 Trojan+TLS(TCP)' },
  { key: 'hy2', name: '压测-HY2入口', nodePrefix: 'b4d05aad', mode: 'domain' as const,
    modeLabel: '域名直连（灰云）', protocolLabel: 'HY2', protocolValue: 'hysteria',
    port: 18444, carriage: '', clashType: 'hysteria2', desc: '灰云直连 HY2(UDP)+TLS' },
  { key: 'ss', name: '压测-SS2022入口', nodePrefix: 'b4d05aad', mode: 'ip' as const,
    modeLabel: 'IP 直连（免证书）', protocolLabel: 'Shadowsocks', protocolValue: 'shadowsocks',
    port: 28443, carriage: 'TCP + UDP', clashType: 'ss', desc: 'IP直连免证书 SS-2022(TCP+UDP)' },
];

type NodeInfo = { id: string; name: string; ip: string };
const nodes: Record<string, NodeInfo> = {}; // by prefix
// 每个节点上现有压测入口所用的出口名（供新入口绑定同一本机出口，保证线路一致就绪）。
const preferredExitByNode: Record<string, string> = {};
const createdEntries: Array<{ key: string; name: string; node: string; nodeId: string; nodeIp: string;
  protocol: string; mode: string; port: number; clashType: string; desc: string }> = [];

// ---- 通用 UI 工具（与 phase3 同口径） ----
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
async function shot(page: Page, name: string) {
  await page.screenshot({ path: `test-results/phase5-${name}.png`, fullPage: true }).catch(() => {});
}
function groupBindings(routing: any): any[] {
  const arr = (routing?.line_groups ?? routing?.lineGroups ?? []) as any[];
  const g = arr.find((x) => x?.name === GROUP_NAME);
  return g ? (g.binding_node_ids ?? g.bindingNodeIds ?? []) : [];
}

// 累积 / 落盘场景产出：保留 phase3 既有结构，追加新入口、刷新 users。
const scenario: any = (() => {
  try { return JSON.parse(readFileSync(OUT, 'utf8')); } catch { return {}; }
})();
scenario.panel ||= process.env.E2E_BASE_URL || 'http://127.0.0.1:8080';
scenario.group ||= GROUP_NAME;
function persist() {
  writeFileSync(OUT, JSON.stringify(scenario, null, 2), { mode: 0o600 });
}

test.describe.configure({ mode: 'serial' });

test.describe('Phase5 压测-分组补建 3 种新协议入口（全真实 UI）', () => {
  test('步骤0：admin 登录、解析节点、读现有出口/绑定', async ({ page }) => {
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
      };
    }
    // 取现有压测入口在各节点所用的出口名（绑新入口时优先选同一本机出口）。
    const bindsRaw = await pageGet(page, '/api/admin/access-entry-exit-bindings');
    const binds = bindsRaw?.bindings ?? bindsRaw?.access_entry_exit_bindings ?? bindsRaw ?? [];
    for (const b of (Array.isArray(binds) ? binds : [])) {
      const entryName = String(b.accessEntryName ?? b.access_entry_name ?? '');
      const exitName = String(b.exitEndpointName ?? b.exit_endpoint_name ?? '');
      const nodeId = String(b.accessNodeId ?? b.access_node_id ?? '');
      if (entryName.startsWith('压测-') && exitName) {
        const nodeEntry = Object.values(nodes).find((nn) => nn.id === nodeId);
        if (nodeEntry) preferredExitByNode[nodeEntry.id] ||= exitName;
      }
    }
  });

  test('步骤1：建 3 入口（Trojan / HY2 / SS2022）', async ({ page }) => {
    test.setTimeout(150_000);
    await adminLogin(page);
    for (const e of ENTRIES) {
      const node = nodes[e.nodePrefix];
      await page.goto('/admin/access-entries');
      if (await listHas(page, '/api/admin/access-entries', 'access_entries', e.name)) {
        createdEntries.push({ key: e.key, name: e.name, node: node.name, nodeId: node.id, nodeIp: node.ip,
          protocol: e.protocolValue, mode: e.mode, port: e.port, clashType: e.clashType, desc: e.desc });
        continue;
      }
      try {
        await page.getByRole('button', { name: '创建入口' }).click();
        const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: '创建入口' }) });
        await expect(dialog).toBeVisible();
        await formItemByLabel(page, '入口名称').locator('input').first().fill(e.name);
        // 接入节点（按解析到的真实节点名精确选）。
        await pickSelectOption(page, formItemByLabel(page, '接入节点').locator('.el-select'),
          new RegExp(node.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
        await page.waitForTimeout(500);
        // 连接方式分段控件（域名直连 / IP 直连）。选域名会自动选用该节点 direct 域名。
        await page.locator('.el-radio-button').filter({ hasText: e.modeLabel }).first()
          .locator('.el-radio-button__inner').click();
        await page.waitForTimeout(500);
        // 协议（域名直连放全族含 Trojan/HY2/SS；IP 直连放 VLESS/SS）。
        await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), e.protocolLabel);
        await page.waitForTimeout(400);
        // 安全模式由前端按协议自动归一（Trojan/HY2→TLS，SS→None），无需手选。
        // 承载（仅 SS 显式选 TCP+UDP；Trojan 默认 TCP；HY2 无承载下拉）。
        if (e.carriage) {
          try {
            await pickSelectOption(page, formItemByLabel(page, '承载').locator('.el-select'), e.carriage);
          } catch { /* 承载选不上不致命，走默认 */ }
        }
        // 监听端口（非 CF 入口为数字输入框）。
        const portInput = formItemByLabel(page, '监听端口').locator('input').first();
        await portInput.click();
        await portInput.fill(String(e.port));
        await page.keyboard.press('Tab');
        await shot(page, `01-entry-${e.key}-form`);
        await waitWrite(page, /\/api\/admin\/access-entries$/, 'POST', async () => {
          await dialog.getByRole('button', { name: '保存' }).click();
        });
        await expect(page.getByText(e.name).first()).toBeVisible({ timeout: 10_000 });
        createdEntries.push({ key: e.key, name: e.name, node: node.name, nodeId: node.id, nodeIp: node.ip,
          protocol: e.protocolValue, mode: e.mode, port: e.port, clashType: e.clashType, desc: e.desc });
      } catch (err) {
        await shot(page, `01-entry-${e.key}-FAIL`);
        console.log(`ENTRY_CREATE_FAILED key=${e.key} name=${e.name} err=${String(err).slice(0, 300)}`);
      }
    }
    await shot(page, '01-entries');
    expect(createdEntries.length, '3 条新入口应全部建成').toBe(ENTRIES.length);
  });

  test('步骤2：每条新入口绑定同节点本机出口', async ({ page }) => {
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
        const binds = bj?.data?.bindings ?? bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
        return (Array.isArray(binds) ? binds : []).some(
          (b: any) => String(b.accessEntryId ?? b.access_entry_id) === String(ent.id),
        );
      }, e.name).catch(() => false);
      if (bound) continue;
      const row = page.locator('tr').filter({ hasText: e.name }).first();
      await expect(row).toBeVisible();
      await row.getByRole('button', { name: '绑定线路' }).click();
      const dialog = page.getByRole('dialog', { name: '绑定线路' });
      await expect(dialog).toBeVisible();
      await dialog.locator('.el-select').first().click();
      await page.waitForTimeout(300);
      // 优先选同节点既有压测入口所用出口；其次按节点 IP 匹配；再不行取第一个。
      const preferred = preferredExitByNode[e.nodeId];
      const items = page.locator('.el-select-dropdown__item:visible');
      const byName = preferred ? items.filter({ hasText: preferred }).first() : items.first();
      const byIp = e.nodeIp ? items.filter({ hasText: e.nodeIp }).first() : items.first();
      let target = items.first();
      if (preferred && await byName.count() > 0) target = byName;
      else if (e.nodeIp && await byIp.count() > 0) target = byIp;
      await expect(target).toBeVisible({ timeout: 10_000 });
      await target.click();
      await page.keyboard.press('Escape').catch(() => {});
      await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
        await dialog.getByRole('button', { name: '保存' }).click();
      });
    }
    await shot(page, '02-bindings');
  });

  test('步骤3：编辑「压测-分组」加进 3 条新绑定（保留原 VLESS）', async ({ page }) => {
    test.setTimeout(90_000);
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    const before = groupBindings(await pageGet(page, '/api/admin/access-routing')).length;
    const row = page.locator('tr').filter({ hasText: GROUP_NAME }).first();
    await expect(row).toBeVisible();
    await row.getByRole('button', { name: '编辑' }).first().click();
    const dialog = page.getByRole('dialog', { name: '编辑分组' });
    await expect(dialog).toBeVisible();
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    await page.waitForTimeout(600);
    // 勾选 3 条新入口对应的绑定项（option label 含入口名）；已选则不重复点。
    for (const e of createdEntries) {
      const opt = page.locator('.el-select-dropdown__item:visible').filter({ hasText: e.name }).first();
      if (await opt.count() > 0) {
        const cls = (await opt.getAttribute('class')) || '';
        if (!cls.includes('is-selected')) {
          await opt.click();
          await page.waitForTimeout(150);
        }
      } else {
        console.log(`GROUP_OPTION_MISSING name=${e.name}`);
      }
    }
    await page.keyboard.press('Escape');
    await shot(page, '03-group-form');
    // saveGroup 先 PUT /line-groups/{id} 再 PUT /line-groups/{id}/binding-nodes，等后者（真正写绑定）。
    await waitWrite(page, /\/api\/admin\/line-groups\/[0-9a-f-]+\/binding-nodes$/, 'PUT', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    await page.waitForTimeout(1_000);
    const after = groupBindings(await pageGet(page, '/api/admin/access-routing')).length;
    await shot(page, '03-group');
    expect(after, `分组绑定数应由 ${before} 增至 ≥6`).toBeGreaterThanOrEqual(6);
  });

  test('步骤4：10 用户真实重下订阅并断言含 4 协议 6 节点', async ({ page }) => {
    test.setTimeout(300_000);
    const users: any[] = [];
    for (const u of USERS) {
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
        if (!token) await page.waitForTimeout(1_500);
      }
      // 真实下载 Clash YAML（订阅端点按 token 公开，等同真客户端拉取）。
      const yamlResp = await page.request.get(subUrl.startsWith('http') ? subUrl : `${page.url().replace(/\/subscription$/, '')}${subUrl}`);
      const yaml = yamlResp.ok() ? await yamlResp.text() : '';
      const count = (re: RegExp) => (yaml.match(re) || []).length;
      const protocols = {
        vless: count(/type:\s*vless/g),
        trojan: count(/type:\s*trojan/g),
        hysteria2: count(/type:\s*hysteria2/g),
        ss: count(/type:\s*ss\b/g),
      };
      const proxyCount = protocols.vless + protocols.trojan + protocols.hysteria2 + protocols.ss;
      const ssCipher2022 = /cipher:\s*2022-blake3/.test(yaml);
      const planDef = scenario.plans?.find((p: any) => p.name === u.plan);
      users.push({
        username: u.username, email: u.email, password: USER_PASSWORD, plan: u.plan,
        uplinkMbps: planDef?.uplinkMbps ?? null, downlinkMbps: planDef?.downlinkMbps ?? null,
        subscriptionUrl: subUrl, subToken: token,
        proxyCount, protocols, ssCipher2022,
      });
      // 退出当前用户会话，保证下一个用户干净登录。
      await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }).catch(() => {});
      await page.context().clearCookies().catch(() => {});
      // 断言该用户订阅含全部 4 协议、共 6 节点。
      expect(token, `用户 ${u.username} 订阅 token 应解析到`).not.toBe('');
      expect(protocols.vless, `用户 ${u.username} 应保留 ≥3 个 VLESS`).toBeGreaterThanOrEqual(3);
      expect(protocols.trojan, `用户 ${u.username} 应含 Trojan`).toBeGreaterThanOrEqual(1);
      expect(protocols.hysteria2, `用户 ${u.username} 应含 HY2`).toBeGreaterThanOrEqual(1);
      expect(protocols.ss, `用户 ${u.username} 应含 SS`).toBeGreaterThanOrEqual(1);
      expect(proxyCount, `用户 ${u.username} 节点总数应为 6`).toBe(6);
    }
    // 追加新入口到场景 entries（按名去重），刷新 users。
    const existingEntries: any[] = Array.isArray(scenario.entries) ? scenario.entries : [];
    for (const c of createdEntries) {
      if (!existingEntries.some((x) => x.name === c.name)) {
        existingEntries.push({ name: c.name, node: c.node, connection: c.mode, protocol: c.protocol,
          clashType: c.clashType, port: c.port, desc: c.desc });
      }
    }
    scenario.entries = existingEntries;
    scenario.entriesReady = existingEntries.map((x) => x.name);
    scenario.users = users;
    scenario.phase5 = {
      addedEntries: createdEntries.map((c) => ({ name: c.name, protocol: c.protocol, clashType: c.clashType,
        node: c.node, connection: c.mode, port: c.port })),
      protocolsPerUser: '每个用户订阅含 vless×3 + trojan + hysteria2 + ss = 6 节点',
    };
    persist();
    await shot(page, '04-subscriptions');
    expect(users.length).toBe(10);
  });
});
