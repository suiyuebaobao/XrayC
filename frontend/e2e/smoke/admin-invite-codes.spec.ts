// 用途：覆盖管理员邀请码页面的菜单、列表、生成弹窗和删除按钮 smoke。
// 测试使用 mock 管理员会话，不依赖真实邀请码或真实管理员密码。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员可以打开邀请码管理页面并看到生成入口', async ({ page }) => {
  await mockAdminSession(page);
  await page.route('**/api/admin/invite-codes', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: {
            items: [
              {
                code: 'INV-SMOKE-001',
                inviter_email: 'admin@example.test',
                is_used: false,
                used_by_email: '',
                used_at: '',
                created_at: '2026-06-01T00:00:00Z',
              },
              {
                code: 'INV-SMOKE-USED',
                inviter_email: 'admin@example.test',
                is_used: true,
                used_by_email: 'user@example.test',
                used_at: '2026-06-01T01:00:00Z',
                created_at: '2026-06-01T00:00:00Z',
              },
            ],
          },
        }),
      });
      return;
    }

    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: {
            items: [
              {
                code: 'INV-SMOKE-NEW',
                inviter_email: 'admin@example.test',
                is_used: false,
                used_by_email: '',
                used_at: '',
                created_at: '2026-06-01T02:00:00Z',
              },
            ],
          },
        }),
      });
      return;
    }

    await route.fallback();
  });

  await page.goto('/admin/invite-codes');
  await expect(page.getByRole('heading', { name: '邀请码管理' })).toBeVisible();
  await expect(page.getByRole('link', { name: '邀请注册', exact: true })).toBeVisible();
  await expect(page.getByText('INV-SMOKE-001')).toBeVisible();
  await expect(page.getByText('INV-SMOKE-USED')).toBeVisible();
  await expect(page.getByText('user@example.test')).toBeVisible();
  await expect(page.getByRole('button', { name: '删除' }).last()).toBeDisabled();

  await page.getByRole('button', { name: '批量生成' }).click();
  const dialog = page.getByRole('dialog', { name: '批量生成邀请码' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText('每次最多 100 个')).toBeVisible();
  await dialog.getByRole('button', { name: '取消' }).click();
  await expect(dialog).toBeHidden();
});
