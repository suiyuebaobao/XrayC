// 用途：覆盖后台订阅设置页线路过滤开关和保存字段 smoke 场景。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员保存订阅设置时提交屏蔽故障线路字段', async ({ page }) => {
  await mockAdminSession(page);

  const settings = {
    mixed_port: 7890,
    allow_lan: false,
    mode: 'rule',
    log_level: 'info',
    profile_name: 'XrayC Smoke',
    update_interval_hours: 24,
    default_rules: ['MATCH,DIRECT'],
    auto_test_enabled: false,
    auto_test_name: '自动选择',
    auto_test_url: 'http://cp.cloudflare.com/generate_204',
    auto_test_interval_seconds: 86400,
    block_unhealthy_lines: false,
  };
  let submittedPayload: Record<string, unknown> | null = null;

  await page.route('**/api/admin/subscription-settings', async (route) => {
    if (route.request().method() === 'PUT') {
      submittedPayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: submittedPayload }),
      });
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: settings }),
    });
  });

  await page.goto('/admin/subscription-settings');
  await expect(page.getByRole('heading', { name: '订阅设置' })).toBeVisible();
  await expect(page.getByText('线路过滤')).toBeVisible();

  const unhealthyLineFilter = page.locator('.el-form-item').filter({ hasText: '屏蔽故障线路' });
  await expect(unhealthyLineFilter).toBeVisible();
  await unhealthyLineFilter.locator('.el-switch').click();

  const saveResponse = page.waitForResponse(
    (response) => response.url().includes('/api/admin/subscription-settings')
      && response.request().method() === 'PUT',
  );
  await page.getByRole('button', { name: '保存配置' }).click();
  await saveResponse;

  expect(submittedPayload).toMatchObject({ block_unhealthy_lines: true });
});
