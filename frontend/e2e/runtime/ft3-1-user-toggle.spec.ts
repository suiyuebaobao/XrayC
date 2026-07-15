/*
 * 用途：FT3-1 流1 剔除验证——真实 UI 编辑用户切换「启用状态」(禁用/恢复)。
 * 真实 admin 登录 + 真实页面点击；通过 FT3_TOGGLE_ACTION=disable|enable 决定动作。
 * 触发点：编辑用户弹窗 → 启用状态开关(active/disabled) → 保存 → PUT /api/admin/users/{id}。
 */
import { expect, type Page, test } from '@playwright/test';

import { login } from './runtime-api';

const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const USER_EMAIL = process.env.FT3_USER_EMAIL || '';
const ACTION = (process.env.FT3_TOGGLE_ACTION || 'disable') as 'disable' | 'enable';

async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

test('FT3-1 剔除：真实 UI 切换用户启用状态', async ({ page }) => {
  expect(USER_EMAIL, 'FT3_USER_EMAIL 必填').not.toBe('');
  await adminLogin(page);
  await page.goto('/admin/users');
  const row = page.locator('tr').filter({ hasText: USER_EMAIL }).first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  // 打开编辑用户弹窗
  await row.getByRole('button', { name: '编辑用户' }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  // 「启用状态」开关：active-value=active(启用) / inactive-value=disabled(禁用)
  const statusItem = dialog.locator('.el-form-item').filter({
    has: page.locator('.el-form-item__label', { hasText: '启用状态' }),
  }).first();
  const sw = statusItem.locator('.el-switch').first();
  const isChecked = (await sw.getAttribute('class'))?.includes('is-checked');
  // disable: 需要切到非 checked(禁用); enable: 需要切到 checked(启用)
  const want = ACTION === 'enable';
  if (isChecked !== want) {
    await sw.click();
  }
  // 保存并等待 PUT /api/admin/users/{id}
  const respPromise = page.waitForResponse(
    (r) => /\/api\/admin\/users\/[0-9a-f-]+$/.test(new URL(r.url()).pathname) && r.request().method() === 'PUT',
    { timeout: 30_000 },
  );
  await dialog.getByRole('button', { name: '保存' }).click();
  const resp = await respPromise;
  expect(resp.status(), 'PUT users status').toBeGreaterThanOrEqual(200);
  expect(resp.status()).toBeLessThan(300);
  await page.screenshot({ path: `test-results/ft3-toggle-${ACTION}.png`, fullPage: true }).catch(() => {});
});
