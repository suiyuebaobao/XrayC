//! 真实浏览器验证:套餐表单只剩上行/下行限速框(无「默认限速」),建套餐端到端工作、列表显示上下行。
//! 跑在已部署的主面板(caddy 服务的真前端 + 真后端 + 真 DB),非 mock。
import { expect, test } from '@playwright/test';

import { login } from './runtime-api';

const ADMIN = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const PASS = process.env.E2E_ADMIN_PASSWORD || 'admin123456';

test('套餐表单只剩上行/下行限速、建套餐端到端工作', async ({ page }) => {
  await login(page, '/admin/login', ADMIN, PASS, /\/(overview|dashboard)$/);
  await page.goto('/admin/plans');

  // 打开新增套餐弹窗。
  await page.getByRole('button', { name: '新增套餐' }).click();
  const dialog = page.getByRole('dialog', { name: '新增套餐' });
  await expect(dialog).toBeVisible();

  // 断言:有「上行限速」「下行限速」表单项,且【没有】「默认限速」。
  await expect(dialog.locator('.el-form-item__label', { hasText: '上行限速' })).toBeVisible();
  await expect(dialog.locator('.el-form-item__label', { hasText: '下行限速' })).toBeVisible();
  expect(
    await dialog.locator('.el-form-item__label', { hasText: '默认限速' }).count(),
    '套餐表单不应再有「默认限速」框',
  ).toBe(0);

  // 填套餐名 + 上行 2 / 下行 10。
  const name = `ui-updown-${Date.now()}`;
  await dialog.locator('.el-form-item', { hasText: '套餐名称' }).locator('input').first().fill(name);
  const up = dialog.locator('.el-form-item', { hasText: '上行限速' }).locator('input').first();
  const down = dialog.locator('.el-form-item', { hasText: '下行限速' }).locator('input').first();
  await up.fill('2');
  await down.fill('10');
  await dialog.getByRole('button', { name: '创建套餐' }).click();
  await expect(dialog).toBeHidden({ timeout: 10_000 });

  // 列表里出现该套餐,且「限速 上/下」列显示 2/10。
  const row = page.locator('tr', { hasText: name });
  await expect(row).toBeVisible({ timeout: 10_000 });
  await expect(row).toContainText('2 Mbps');
  await expect(row).toContainText('10 Mbps');

  // 清理:删除该测试套餐(找到行内删除按钮)。
  await row.getByRole('button', { name: /删除/ }).click();
  const confirm = page.locator('.el-message-box');
  if (await confirm.count()) {
    await confirm.locator('button', { hasText: /确定|确认|删除/ }).first().click();
  }
  await expect(page.locator('tr', { hasText: name })).toHaveCount(0, { timeout: 10_000 });
});
