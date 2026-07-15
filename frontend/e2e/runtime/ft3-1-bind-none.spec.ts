/*
 * 用途：FT3-1 流1 补绑——为 IP-none 入口真实 UI 绑定出口线路(步骤7漏绑修补)。
 */
import { expect, type Page, test } from '@playwright/test';

import { login } from './runtime-api';

const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const ENTRY_NAME = process.env.FT3_BIND_ENTRY || '';

async function adminLogin(page: Page) {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
}

test('FT3-1 补绑 IP-none 入口出口线路', async ({ page }) => {
  expect(ENTRY_NAME, 'FT3_BIND_ENTRY 必填').not.toBe('');
  await adminLogin(page);
  await page.goto('/admin/access-entries');
  const row = page.locator('tr').filter({ hasText: ENTRY_NAME }).first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  await row.getByRole('button', { name: '绑定线路' }).click();
  const dialog = page.getByRole('dialog', { name: '绑定线路' });
  await expect(dialog).toBeVisible();
  await dialog.locator('.el-select').first().click();
  await page.locator('.el-select-dropdown__item:visible').first().click();
  const respPromise = page.waitForResponse(
    (r) => /\/api\/admin\/access-entries\/[0-9a-f-]+\/exit-bindings$/.test(new URL(r.url()).pathname)
      && r.request().method() === 'POST',
    { timeout: 30_000 },
  );
  await dialog.getByRole('button', { name: '保存' }).click();
  const resp = await respPromise;
  expect(resp.status()).toBeGreaterThanOrEqual(200);
  expect(resp.status()).toBeLessThan(300);
  await page.screenshot({ path: 'test-results/ft3-bind-none.png', fullPage: true }).catch(() => {});
});
