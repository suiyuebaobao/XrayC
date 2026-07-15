/*
 * 用途：FT3-1 流1 修补——编辑分组把 none + reality 两条绑定都选上(原分组只含 reality)。
 * 真实 UI：/admin/line-groups → 编辑分组 → 选择绑定节点多选全选 → 保存(PUT replaceLineGroupBindingNodes)。
 */
import { expect, type Page, test } from '@playwright/test';

import { login } from './runtime-api';

const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const GROUP_NAME = process.env.FT3_GROUP_NAME || '';
const NS = process.env.FT3_NS || '';

async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

test('FT3-1 修补分组：选上 none 与 reality 两条绑定', async ({ page }) => {
  expect(GROUP_NAME, 'FT3_GROUP_NAME 必填').not.toBe('');
  await adminLogin(page);
  await page.goto('/admin/line-groups');
  const row = page.locator('tr').filter({ hasText: GROUP_NAME }).first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  await row.getByRole('button', { name: '编辑' }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  // 「选择绑定节点」多选：把所有属于本 NS 的绑定选项都勾上(none + reality)。
  const select = dialog.locator('.el-form-item').filter({
    has: page.locator('.el-form-item__label', { hasText: '选择绑定节点' }),
  }).locator('.el-select').first();
  await select.click();
  await page.waitForTimeout(500);
  // 勾选所有含 ip-none / ip-reality 的下拉项；Element Plus 选中类是 is-selected，只点未选中的。
  const opts = page.locator('.el-select-dropdown__item').filter({ hasText: NS });
  const n = await opts.count();
  // 调试：打印每个候选项文本与选中态
  for (let i = 0; i < n; i++) {
    const opt = opts.nth(i);
    const txt = (await opt.textContent())?.trim() || '';
    const cls = (await opt.getAttribute('class')) || '';
    const selected = cls.includes('is-selected') || cls.includes('selected');
    console.log(`OPTION[${i}] selected=${selected} text=${txt.slice(0, 60)}`);
    if (!selected) {
      await opt.click();
      await page.waitForTimeout(200);
    }
  }
  await page.keyboard.press('Escape');
  const respPromise = page.waitForResponse(
    (r) => /\/api\/admin\/line-groups\/[0-9a-f-]+\/binding-nodes$/.test(new URL(r.url()).pathname)
      && r.request().method() === 'PUT',
    { timeout: 30_000 },
  ).catch(() => null);
  await dialog.getByRole('button', { name: '保存' }).click();
  await respPromise;
  await page.waitForTimeout(1500);
  await page.screenshot({ path: 'test-results/ft3-fix-group.png', fullPage: true }).catch(() => {});
});
