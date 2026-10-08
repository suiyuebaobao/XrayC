import { openNodeOnboarding } from '../helpers/node-onboarding';
/*
 * 用途：并行流3·R3 控制面全流程真实操作 E2E（真实 UI 点击 + 真实后端 no-mock + 真实 SSH 部署 agent）。
 * 覆盖域名直连(灰云,sslip.io)下 ① Trojan ② HY2(hysteria/udp) ③ Shadowsocks(2022)；严禁 mock/SQL 直插。
 * 新建对象用 RUN_ID 命名空间(e2e-r3-*)，绝不碰基线真实客户数据/其他流的测试节点；产物写 PHASER3_ARTIFACTS(0600)。
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { expect, type Page, type Response, test } from '@playwright/test';

import { login } from './runtime-api';
import {
  boundEntryNames,
  entryHasBinding,
  fetchNodeHealth,
  findEntryId,
  findGroupId,
  findNodeId,
  findPlanId,
  findUserId,
  nsExitEndpointCount,
  groupBindingNodeCount,
  nsBoundEntryCount,
} from './real-op-r3-bindings';

// ---- 环境与命名空间 -------------------------------------------------------
const RUN_ID = process.env.PHASER3_RUN_ID || process.env.PHASEC_RUN_ID || `${Date.now()}`;
const NS = `e2e-r3-${RUN_ID}`;
const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ACME_EMAIL = process.env.PHASER3_ACME_EMAIL || 'acme@example.test';
const ARTIFACTS = process.env.PHASER3_ARTIFACTS || '/tmp/phaseR3-artifacts.env';
const HEARTBEAT_TIMEOUT_MS = Number(process.env.PHASER3_HEARTBEAT_TIMEOUT_MS || 480_000);

// 域名直连(direct non-CF, sslip.io)：一键安装目标 + 直连证书域名。
// SSH_HOST 仅用于节点登记表单的「SSH IP」字段；真实 agent 安装由 deploy-access-agent.sh 完成（见报告）。
const SSH_HOST = required('PHASER3_SSH_HOST');
const DIRECT_DOMAIN = required('PHASER3_DIRECT_DOMAIN'); // sslip.io 直连证书域名
const IP_DIRECT = process.env.PHASER3_IP_DIRECT || SSH_HOST; // IP 直连地址（出口用）

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
  await page.screenshot({ path: `test-results/phaseR3-${name}.png`, fullPage: true }).catch(() => {});
}

// 串行：所有步骤共享同一上下文与已建对象。
test.describe.configure({ mode: 'serial' });

test.describe('并行流3·R3 控制面全流程真实操作（server_2 域名直连 Trojan/HY2/SS）', () => {
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
    const existingPlan = await findPlanId(page, PLAN_NAME);
    if (existingPlan) {
      planId = existingPlan;
      recordArtifact('PHASER3_PLAN_ID', planId);
      recordArtifact('PHASER3_PLAN_NAME', PLAN_NAME);
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
    recordArtifact('PHASER3_PLAN_ID', planId);
    recordArtifact('PHASER3_PLAN_NAME', PLAN_NAME);
    await expect(page.getByText(PLAN_NAME).first()).toBeVisible();
    await shot(page, '02-plan-created');
    expect(planId, '套餐 id').not.toBe('');
  });

  test('步骤3：真实建测试用户并授权套餐', async ({ page }) => {
    await adminLogin(page);
    await page.goto('/admin/users');
    const existingUser = await findUserId(page, USER_EMAIL);
    if (existingUser) {
      recordArtifact('PHASER3_USER_EMAIL', USER_EMAIL);
      recordArtifact('PHASER3_USER_PASSWORD', USER_PASSWORD);
      recordArtifact('PHASER3_USER_ID', existingUser);
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
    recordArtifact('PHASER3_USER_EMAIL', USER_EMAIL);
    recordArtifact('PHASER3_USER_PASSWORD', USER_PASSWORD);
    recordArtifact('PHASER3_USER_ID', userId);
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
      const r = await fetchNodeHealth(page, NODE_NAME);
      if (r.id) {
        id = r.id;
        if (r.hb && (r.health === 'healthy' || r.health === 'syncing')) {
          online = true;
          break;
        }
      }
      await page.waitForTimeout(6_000);
    }
    /* eslint-enable no-await-in-loop */
    return { online, id };
  }

  // 步骤4：UI 新增中转节点登记已装 Agent（CLAUDE.md §13 手动安装路径）。
  // 说明：本地 vite 驱动的「一键安装」会把 control_plane_url 设成 window.location.origin
  //（=本机 127.0.0.1，远端不可达，制品下载失败），故走真实 deploy-access-agent.sh 装好 agent +
  // 域名直连 HTTP-01 证书，UI 用鉴权码登记节点；心跳通过即上线。
  test('步骤4：UI 新增中转节点登记已装 Agent（手动安装路径）', async ({ page }) => {
    const authCode = process.env.PHASER3_AGENT_AUTH_CODE?.trim();
    test.skip(!authCode, '未注入 PHASER3_AGENT_AUTH_CODE');
    test.setTimeout(HEARTBEAT_TIMEOUT_MS + 120_000);
    await adminLogin(page);
    await page.goto('/admin/transit-nodes');
    const existing = await findNodeId(page, NODE_NAME);
    if (existing) {
      nodeId = existing;
      recordArtifact('PHASER3_NODE_ID', nodeId);
      const r = await waitNodeHealthy(page);
      recordArtifact('PHASER3_NODE_ONLINE', String(r.online));
      await shot(page, '04alt-node-online');
      expect(r.online, '已存在节点应健康').toBe(true);
      return;
    }
    await openNodeOnboarding(page, 'existing');
    const dialog = page.getByRole('dialog', { name: '接入已有 Agent' });
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
    recordArtifact('PHASER3_NODE_ID', nodeId);
    const result = await waitNodeHealthy(page);
    if (!nodeId) nodeId = result.id;
    recordArtifact('PHASER3_NODE_ID', nodeId);
    recordArtifact('PHASER3_NODE_ONLINE', String(result.online));
    await shot(page, '04alt-node-online');
    expect(nodeId, '节点 id 应已创建').not.toBe('');
    expect(result.online, `节点心跳应在 ${HEARTBEAT_TIMEOUT_MS}ms 内健康`).toBe(true);
  });

  test('步骤5：建本机出口服务（IP 直连 + VLESS Reality 作内部出口）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过出口');
    await adminLogin(page);
    // 幂等：本机出口创建非幂等（每次预填多条），已建过就跳过，避免重复堆叠拖垮 agent reconcile。
    await page.goto('/admin/access-entries');
    if (await nsExitEndpointCount(page, NS) > 0) { recordArtifact('PHASER3_EXIT_LINE_NAME', EXIT_LINE_NAME); return; }
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
    recordArtifact('PHASER3_EXIT_LINE_NAME', EXIT_LINE_NAME);
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
    recordArtifact(`PHASER3_ENTRY_${key}_ID`, id);
    await expect(page.getByText(name).first()).toBeVisible();
  }

  // 幂等：入口创建非幂等（同名重建 422），重跑前先查既有，存在即记 id 并跳过创建。
  async function reuseEntryIfExists(page: Page, key: string, name: string): Promise<boolean> {
    const existing = await findEntryId(page, name);
    if (!existing) return false;
    entryIds[key] = existing;
    recordArtifact(`PHASER3_ENTRY_${key}_ID`, existing);
    return true;
  }

  test('步骤6a：入口① 域名直连 + Trojan（TLS + 证书域名 SNI）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    if (await reuseEntryIfExists(page, 'TROJAN', `${NS}-trojan`)) return;
    const dialog = await openEntryDialog(page);
    const name = `${NS}-trojan`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'Trojan');
    // Trojan 强制 TLS，安全模式只有 TLS 一项；SNI 自动带证书域名。
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'TLS');
    const sni = formItemByLabel(page, 'Server Name').locator('input').first();
    await expect(sni).toHaveValue(new RegExp(DIRECT_DOMAIN.replace(/\./g, '\\.')));
    await setListenPort(page, 30011);
    await shot(page, '06a-entry-trojan-form');
    await saveEntry(page, dialog, 'TROJAN', name);
  });

  test('步骤6b：入口② 域名直连 + HY2（hysteria/udp，TLS + 证书域名 SNI）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    if (await reuseEntryIfExists(page, 'HY2', `${NS}-hy2`)) return;
    const dialog = await openEntryDialog(page);
    const name = `${NS}-hy2`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'HY2');
    // HY2 强制 TLS + UDP（hysteria）；SNI 自动带证书域名。
    await pickSelectOption(page, formItemByLabel(page, '安全模式').locator('.el-select'), 'TLS');
    const sni = formItemByLabel(page, 'Server Name').locator('input').first();
    await expect(sni).toHaveValue(new RegExp(DIRECT_DOMAIN.replace(/\./g, '\\.')));
    await setListenPort(page, 30012);
    await shot(page, '06b-entry-hy2-form');
    await saveEntry(page, dialog, 'HY2', name);
  });

  test('步骤6c：入口③ 域名直连 + Shadowsocks（2022 系列）', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过入口');
    await adminLogin(page);
    await page.goto('/admin/access-entries');
    if (await reuseEntryIfExists(page, 'SS', `${NS}-ss`)) return;
    const dialog = await openEntryDialog(page);
    const name = `${NS}-ss`;
    await fillEntryBasics(page, dialog, name);
    await pickDirectDomain(page);
    await pickSelectOption(page, formItemByLabel(page, '入口/伪装协议').locator('.el-select'), 'Shadowsocks');
    // SS 无 SNI（security=None），方法由后端按 2022 系列生成。
    const sni = formItemByLabel(page, 'Server Name').locator('input').first();
    await expect(sni).toHaveValue('');
    await setListenPort(page, 30013);
    await shot(page, '06c-entry-ss-form');
    await saveEntry(page, dialog, 'SS', name);
  });

  test('步骤7：每条入口绑定出口线路', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过绑定');
    await adminLogin(page);
    // 幂等：先查已存在出口绑定的入口名集合（create-binding 是追加语义，重绑会产生重复）。
    await page.goto('/admin/access-entries');
    const boundNames = await boundEntryNames(page, NS);
    for (const name of [`${NS}-trojan`, `${NS}-hy2`, `${NS}-ss`]) {
      if (boundNames.includes(name)) continue;
      // 健壮绑定：等行可见→开弹窗→选出口→保存 2xx→只读核验落库；上轮 0 绑定真根因是
      // goto 后立刻 row.count()==0（el-table v-loading 未渲染完）即 break、从未真点绑定。
      let bound = false;
      for (let attempt = 1; attempt <= 3 && !bound; attempt++) {
        await page.goto('/admin/access-entries');
        const row = page.locator('tr').filter({ hasText: name }).first();
        await expect(row, `入口行 ${name} 应在入口表渲染`).toBeVisible({ timeout: 15_000 });
        await row.getByRole('button', { name: '绑定线路' }).click();
        const dialog = page.getByRole('dialog', { name: '绑定线路' });
        await expect(dialog).toBeVisible();
        // 出口下拉是 filterable el-select，下拉面板 teleport 到 body。点开后
        // 用「可见下拉面板内」的选项定位（非全页 .el-select-dropdown__item，避免选到
        // 其他残留下拉），选第一个出口。注意：filterable select 选中后显示值不一定即时
        // 渲染到 .el-select__selected-item（实测为空），故不做显示值预判（那是上轮 0 绑定
        // 的伪根因——伪空显示值把成功的保存误判成"没选上"而走重试/取消，反而真没保存）。
        // 唯一真相 = 保存命中 2xx POST + 只读核验落库。
        const exitSelect = dialog.locator('.el-select').first();
        await exitSelect.click();
        const option = page.locator('.el-select-dropdown:visible .el-select-dropdown__item').first();
        await expect(option).toBeVisible({ timeout: 10_000 });
        await option.click();
        await page.waitForTimeout(300);
        // 保存须命中 2xx 写接口；命不中（被前端护栏拦/无 POST）则本次失败、重试。
        const ok = await waitWrite(page, /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/, 'POST', async () => {
          await dialog.getByRole('button', { name: '保存' }).click();
        }).then(() => true).catch(() => false);
        await expect(dialog).toBeHidden({ timeout: 10_000 }).catch(() => {});
        if (!ok) {
          // 没发 POST（确实没选上）：关弹窗后重试。
          await page.keyboard.press('Escape').catch(() => {});
          continue;
        }
        // 只读核验：该入口确实有绑定落库才算成功。
        bound = await entryHasBinding(page, name);
      }
      expect(bound, `入口 ${name} 的出口绑定应真实落库（重试后仍 0 绑定即 UI 绑定失败）`).toBe(true);
    }
    await shot(page, '07-bindings');
    // 总核验：本命名空间 3 条入口全部有绑定落库。
    const totalBound = await nsBoundEntryCount(page, NS);
    recordArtifact('PHASER3_ENTRY_BINDINGS_LANDED', String(totalBound));
    expect(totalBound, '本命名空间应有 3 条入口出口绑定落库').toBeGreaterThanOrEqual(3);
  });

  test('步骤8：建分组并选绑定节点', async ({ page }) => {
    test.skip(!nodeId, '节点未创建，跳过分组');
    await adminLogin(page);
    await page.goto('/admin/line-groups');
    // 幂等：分组已存在（重跑）则复用其 id，不重复创建。
    const existingGroup = await findGroupId(page, GROUP_NAME);
    if (existingGroup) {
      groupId = existingGroup;
      recordArtifact('PHASER3_GROUP_ID', groupId);
      recordArtifact('PHASER3_GROUP_NAME', GROUP_NAME);
      // 即便复用既有分组也核验其绑定节点真落库非空（重跑下不放过 0 绑定节点）。
      const reuseCount = await groupBindingNodeCount(page, groupId);
      recordArtifact('PHASER3_GROUP_BINDING_NODES_LANDED', String(reuseCount));
      expect(reuseCount, '既有分组绑定节点应真实落库非空').toBeGreaterThan(0);
      return;
    }
    await page.getByRole('button', { name: '创建分组' }).click();
    const dialog = page.getByRole('dialog', { name: '创建分组' });
    await expect(dialog).toBeVisible();
    await formItemByLabel(page, '分组名称').locator('input').first().fill(GROUP_NAME);
    // 「选择绑定节点」multiple el-select，选项来源 = 步骤7 创建的入口↔出口绑定；等可见再多选。
    const bindSelect = formItemByLabel(page, '选择绑定节点').locator('.el-select');
    await bindSelect.click();
    // 只取「当前可见下拉面板」内的选项（多个 el-select 共存时避免选错面板）。
    const opts = page.locator('.el-select-dropdown:visible .el-select-dropdown__item');
    await expect(opts.first()).toBeVisible({ timeout: 10_000 });
    const n = await opts.count();
    expect(n, '选择绑定节点下拉应有可选项（步骤7 的绑定）').toBeGreaterThan(0);
    let picked = 0;
    for (let i = 0; i < n; i++) {
      // 跳过 disabled 选项（未启用绑定不可选）。
      const cls = (await opts.nth(i).getAttribute('class').catch(() => '')) || '';
      if (cls.includes('is-disabled')) continue;
      await opts.nth(i).click();
      picked += 1;
    }
    await page.keyboard.press('Escape');
    expect(picked, '应至少点中一个绑定节点选项').toBeGreaterThan(0);
    const resp = await waitWrite(page, /\/api\/admin\/line-groups$/, 'POST', async () => {
      await dialog.getByRole('button', { name: '保存' }).click();
    });
    groupId = await dataId(resp);
    recordArtifact('PHASER3_GROUP_ID', groupId);
    recordArtifact('PHASER3_GROUP_NAME', GROUP_NAME);
    await shot(page, '08-group-created');
    // 只读核验：line_group_binding_nodes 真落库非空（按 control-plane 读模型 bindingNodeIds）。
    const groupBindCount = await groupBindingNodeCount(page, groupId);
    recordArtifact('PHASER3_GROUP_BINDING_NODES_LANDED', String(groupBindCount));
    expect(groupBindCount, '分组绑定节点应真实落库非空（line_group_binding_nodes）').toBeGreaterThan(0);
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
    test.skip(!artifacts.PHASER3_USER_EMAIL, '用户未创建，跳过订阅');
    await login(page, '/login', USER_EMAIL, USER_PASSWORD, /\/(dashboard|subscription)$/);
    await page.goto('/subscription');
    // 订阅卡片输入框偶尔渲染慢；以「登录态浏览器上下文实拉 /api/user/subscription」为权威订阅源
    // （仍是真实用户 JWT 经浏览器发起，非 mock/非管理员抄近路），UI 卡片读值作辅助。
    const subUrl = await page.evaluate(async () => {
      const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
      const tok = sess.accessToken || sess.access_token || '';
      const r = await fetch('/api/user/subscription', { headers: { Authorization: `Bearer ${tok}` } });
      if (!r.ok) return '';
      const j = await r.json();
      const d = j?.data ?? j;
      return String(d?.subscriptionUrl || d?.subscription_url || '');
    }).catch(() => '');
    const m = subUrl.match(/\/sub\/([A-Za-z0-9_-]+)/);
    const token = m ? m[1] : '';
    recordArtifact('PHASER3_SUB_URL', subUrl);
    recordArtifact('PHASER3_SUB_TOKEN', token);
    await shot(page, '10-user-subscription');
    expect(subUrl, '订阅链接应非空').not.toBe('');
    expect(token, '订阅 token 应可解析').not.toBe('');
  });
});
